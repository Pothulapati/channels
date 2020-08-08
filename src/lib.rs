use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};

// Inner is Arc, because sneder and receiver need to share the same instance of Inner
pub struct Sender<T> {
    inner: Arc<Inner<T>>,
}

impl<T> Sender<T> {
    pub fn send(&mut self, t: T) {
        let mut queue = self.inner.queue.lock().unwrap();
        queue.push(t);
    }
}
pub struct Receiver<T> {
    inner: Arc<Inner<T>>,
}

impl<T> Receiver<T> {
    pub fn recieve(&mut self) -> T {
        let mut queue = self.inner.queue.lock().unwrap();
        queue.pop().unwrap()
    }
}

// queue is Mutex, because it has to mutually be accessed by both sender and reciever without racing
struct Inner<T> {
    queue: Mutex<Vec<T>>,
}

pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
    let inner = Inner {
        queue: Mutex::default(),
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
