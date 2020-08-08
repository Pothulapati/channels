use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};

// Inner is Arc, because sneder and receiver need to share the same instance of Inner
// Sender should be clonable as its mspc, but if it is done using derive proc macro
// then it expects T to be also clonable, thus restricting our channel to only clonable
// T's. In our case, T is wrapped around in Arc, which is clonable (obvv, thats what it is for)
// So, Let's implement clone ourselves manually
pub struct Sender<T> {
    inner: Arc<Inner<T>>,
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        Sender {
            // this is technically legal, but not the right way
            // imagine if self.innner also implemented clone, rust won't
            // know whether this call is for Arc or inside of Arc
            // as Arc derefs into the inner type
            // So, use Arc::clone() to specifically call the clone of the arc but not inside
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<T> Sender<T> {
    // In the current design, there are no waiting senders as when a lock is acquired
    // the sending will work
    pub fn send(&mut self, t: T) {
        let mut queue = self.inner.queue.lock().unwrap();
        queue.push_back(t);
        // drop the lock, so that the reciever can wake up and acquere
        drop(queue);
        // notify reciever after send happens
        // notify_one notifies one of the thread waiting for available condvar
        self.inner.available.notify_one();
    }
}
pub struct Receiver<T> {
    inner: Arc<Inner<T>>,
}

impl<T> Receiver<T> {
    // receive should be blocking, as when there is no element
    // the receive function should make the thread wait
    pub fn receive(&mut self) -> T {
        // loop because when ever we are woken up by wait i.e when a new element is added
        // we want to pop the value.
        // loop also helps because there are no guarantees from OS that this will be woken
        // up only when there is a element, so having loop helps us wait again if there is
        // no element added
        let mut queue = self.inner.queue.lock().unwrap();
        loop {
            match queue.pop_front() {
                Some(t) => return t,
                None => {
                    // wait takes a mutex, because the assumption is
                    // we can't wait while still holding the mutex,
                    // because otherwise whover woke you up can't get the mutex
                    // wait gives up the lock before waiting and it also gives
                    // back the handle acquiring it so that we don't have to
                    queue = self.inner.available.wait(queue).unwrap();
                }
            }
        }
    }
}

// queue is Mutex, because it has to mutually be accessed by both sender and reciever without racing
struct Inner<T> {
    queue: Mutex<VecDeque<T>>,
    // Condvar uses OS based condvar internally
    // this condvar should be outside the mutex, so that when we notify using condvar the other side
    // should be able to acqure the mutex, and they can't do that if condvar is part of the mutex.
    available: Condvar,
}

pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
    let inner = Inner {
        queue: Mutex::default(),
        available: Condvar::new(),
    };
    let inner = Arc::new(inner);

    (
        Sender {
            inner: inner.clone(),
        },
        Receiver {
            inner: inner.clone(),
        },
    )
}
