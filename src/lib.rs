use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};

// Decision Decisions (as per this commit):
// - Sends contented with one another i.e senders are also synced btw themselves.
// - We don't have a bounded variant of sender, which can allow some sort of sync
//   by using a fixed size buffer. There is no back-pressure in our implementation.

// Flavours:
// - Syncrhonous channels: Channel where send() can bloc, and has limited capacity.
// : Mutex + CondVar: VecDeque with Bounded
// : (Cross beam) Atomic VecDeque + thread::park + thread::Thread::notify (for the notification system btw senders and recievers)
// - Asynchronous channels: Channel where send() cannot bloc, unbounded.
// : Mutex + Condvar + VecDeque (Our approach)
// : Atomic Linked List or Atomic Queue
// : Atomic block Linkerd List, linked list of atomic VecDequeue<T> used in cross beam
// : Mutex + Condvar + LinkedList (so that we can mitigate the resizing problem)
// - Rendezvous channels: Synchronous Channel with capacity = 0, Used for thread synchronization
// : Condvar + Mutex
// - Oneshot channels: Any capacity, In practice, only one call to send()

// async/await
// Hard to write a channel implemention to write a channel that works for both
// If you do a send and the channel is full, in async world you want to yield to the executor,
// and at some point you will be woken up to send.
// Similar with Condvar but not the same as async functions have to return.

// Inner is Arc, because sneder and receiver need to share the same instance of Inner
// Sender should be clonable as its mspc, but if it is done using derive proc macro
// then it expects T to be also clonable, thus restricting our channel to only clonable
// T's. In our case, T is wrapped around in Arc, which is clonable (obvv, thats what it is for)
// So, Let's implement clone ourselves manually
pub struct Sender<T> {
    shared: Arc<Shared<T>>,
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        // udate senders after cloning
        let mut inner = self.shared.inner.lock().unwrap();
        inner.senders += 1;
        drop(inner);
        Sender {
            // this is technically legal, but not the right way
            // imagine if self.innner also implemented clone, rust won't
            // know whether this call is for Arc or inside of Arc
            // as Arc derefs into the inner type
            // So, use Arc::clone() to specifically call the clone of the arc but not inside
            shared: Arc::clone(&self.shared),
        }
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        let mut inner = self.shared.inner.lock().unwrap();
        inner.senders -= 1;
        let was_last = inner.senders == 0;
        drop(inner);
        // check and notify if last
        // we can wakeup everytime but unnecessary perf
        if was_last {
            self.shared.available.notify_one();
        }
    }
}

impl<T> Sender<T> {
    // In the current design, there are no waiting senders as when a lock is acquired
    // the sending will work
    pub fn send(&mut self, t: T) {
        let mut inner = self.shared.inner.lock().unwrap();
        inner.queue.push_back(t);
        // drop the lock, so that the reciever can wake up and acquere
        drop(inner);
        // notify reciever after send happens
        // notify_one notifies one of the thread waiting for available condvar
        self.shared.available.notify_one();
    }
}
pub struct Receiver<T> {
    shared: Arc<Shared<T>>,

    // As we know there is only one reciever, so why take a lock everytime?
    // why not just pop all the elements into this buffer when you acquire a lock
    // so that we can directly reply from this bufffer until its empty. Cool Right?
    buffer: VecDeque<T>,
}

impl<T> Receiver<T> {
    // receive should be blocking, as when there is no element
    // the receive function should make the thread wait
    pub fn receive(&mut self) -> Option<T> {
        // When there are elements in the buffer directly reply
        if let Some(t) = self.buffer.pop_front() {
            return Some(t);
        }

        // loop because when ever we are woken up by wait i.e when a new element is added
        // we want to pop the value.
        // loop also helps because there are no guarantees from OS that this will be woken
        // up only when there is a element, so having loop helps us wait again if there is
        // no element added
        let mut inner = self.shared.inner.lock().unwrap();
        loop {
            match inner.queue.pop_front() {
                Some(t) => {
                    // Just take all the elements into the buffer
                    // so that we don't block for the next n recieve requests.
                    if !inner.queue.is_empty() {
                        // swap the empty self.buffer with the inner queue
                        // self.buffer is emtpy as it this code-path is reached
                        std::mem::swap(&mut self.buffer, &mut inner.queue);
                    }

                    return Some(t);
                }
                None if inner.senders == 0 => return None,
                None => {
                    // wait takes a mutex, because the assumption is
                    // we can't wait while still holding the mutex,
                    // because otherwise whover woke you up can't get the mutex
                    // wait gives up the lock before waiting and it also gives
                    // back the handle acquiring it so that we don't have to
                    inner = self.shared.available.wait(inner).unwrap();
                }
            }
        }
    }
}

// queue is Mutex, because it has to mutually be accessed by both sender and reciever without racing
struct Shared<T> {
    inner: Mutex<Inner<T>>,
    // Condvar uses OS based condvar internally
    // this condvar should be outside the mutex, so that when we notify using condvar the other side
    // should be able to acqure the mutex, and they can't do that if condvar is part of the mutex.
    available: Condvar,
}

struct Inner<T> {
    queue: VecDeque<T>,
    senders: i32,
}

pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
    let inner = Inner {
        queue: VecDeque::default(),
        senders: 1,
    };

    let shared = Shared {
        inner: Mutex::new(inner),
        available: Condvar::new(),
    };
    let shared = Arc::new(shared);

    (
        Sender {
            shared: shared.clone(),
        },
        Receiver {
            shared: shared.clone(),
            buffer: VecDeque::new(),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ping_pong() {
        let (mut tx, mut rx) = channel();
        tx.send(42);
        assert_eq!(rx.receive(), Some(42));
    }

    #[test]
    fn closed_tx() {
        let (tx, mut rx) = channel::<i32>();
        drop(tx);
        assert_eq!(rx.receive(), None)
    }

    #[test]
    fn closed_rx() {
        let (mut tx, rx) = channel::<i32>();
        drop(rx);
        tx.send(42);
    }

    #[test]
    fn threaded_ping_pong() {}
}
