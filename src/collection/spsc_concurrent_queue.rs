extern crate core;

use self::core::ptr;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use super::buffer::{Buffer, value_ptr};
use super::concurrent_queue::ConcurrentQueue;

/// A bounded queue allowing a single producer and a single consumer.
pub struct SpscConcurrentQueue<T> {
    buffer: Buffer<T>
}

impl<T> SpscConcurrentQueue<T> {
    /// Creates a single producer single consumer queue with the specified 
    /// capacity.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmo::collection::{ConcurrentQueue, SpscConcurrentQueue};
    /// let q = SpscConcurrentQueue::<u64>::with_capacity(1024);
    /// assert_eq!(1024, q.capacity());
    /// ```
    pub fn with_capacity(initial_capacity: usize) -> Arc<SpscConcurrentQueue<T>> {
        Arc::new(SpscConcurrentQueue { 
            buffer: Buffer::with_capacity(initial_capacity) 
        })
    }
}

impl<T: Clone> SpscConcurrentQueue<T> {
    /// Tries to peek a value from the queue. The value needs to be Clone 
    /// since the value in the queue can't be moved out.
    ///
    /// If the queue is not empty, the method returns `Some(v)`, where `v` is
    /// the value at the head of the queue. If the queue is empty, it returns
    /// `None`.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmo::collection::{ConcurrentQueue, SpscConcurrentQueue};
    /// let queue = SpscConcurrentQueue::<u64>::with_capacity(16);
    /// match queue.peek() {
    ///     Some(v) => println!("Peeked value {}", v),
    ///     None => println!("Queue is empty")
    /// }
    /// ```
    pub fn peek(&self) -> Option<T> {
        let index = self.buffer.head.load(Ordering::Relaxed);
        unsafe {
            let item = self.buffer.item(index);
            if item.is_defined.load(Ordering::Acquire) {
                Some((&*value_ptr(item)).clone())
            } else {
                None
            }
        }
    }
}

impl<T> ConcurrentQueue<T> for SpscConcurrentQueue<T> {
    /// Puts an item in the queue. This method only reads and modifies the 
    /// `tail` index, thus avoiding cache line ping-ponging.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmo::collection::{ConcurrentQueue, SpscConcurrentQueue};
    /// let q = SpscConcurrentQueue::<u64>::with_capacity(1024);
    /// assert_eq!(None, q.offer(10));
    /// ```
    fn offer(&self, val: T) -> Option<T> {
        let index = self.buffer.tail.load(Ordering::Relaxed);
        unsafe { 
            let item = self.buffer.item(index);
            if item.is_defined.load(Ordering::Acquire) {
                return Some(val)
            }
            self.buffer.tail.store(index + 1, Ordering::Relaxed);
            ptr::write(value_ptr(item), val);
            item.is_defined.store(true, Ordering::Release);
            None
        }
    }

    /// Takes an item from the queue. This method only reads and modifies the
    /// `head` index.
    ///
    /// # Example
    ///
    /// ```
    /// use cosmo::collection::{ConcurrentQueue, SpscConcurrentQueue};
    /// let q = SpscConcurrentQueue::<u64>::with_capacity(1024);
    /// q.offer(10);
    /// assert_eq!(Some(10), q.poll());
    /// ```
    fn poll(&self) -> Option<T> {
        let index = self.buffer.head.load(Ordering::Relaxed);
        unsafe {
            let item = self.buffer.item(index);
            if !item.is_defined.load(Ordering::Acquire) {
                return None;
            }
            self.buffer.head.store(index + 1, Ordering::Relaxed);
            let res = ptr::read(value_ptr(item));
            item.is_defined.store(false, Ordering::Release);
            Some(res)
        }
    }

    /// Returns the capacity of the queue.
    fn capacity(&self) -> usize {
        self.buffer.capacity
    }

    /// Returns how many items are in the queue.
    fn size(&self) -> usize {
        self.buffer.size()
    }
}

#[cfg(test)]
mod test {
    use super::core::mem;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Barrier};
    use std::thread;
    use super::SpscConcurrentQueue;
    use super::super::concurrent_queue::ConcurrentQueue;

    #[test]
    fn capacity_is_next_power_of_two() {
        assert_eq!(16, SpscConcurrentQueue::<i32>::with_capacity(10).capacity());
    }

    #[test]
    fn adds_and_removes_a_value() {
        let q = SpscConcurrentQueue::<i32>::with_capacity(2);
        assert_eq!(None, q.offer(34));
        assert_eq!(Some(34), q.poll());
    }
    
    #[test]
    fn gets_full() {
        let q = SpscConcurrentQueue::<i32>::with_capacity(2);
        assert_eq!(None, q.offer(1));
        assert_eq!(None, q.offer(2));
        assert_eq!(Some(3), q.offer(3));
        assert!(q.is_full()); 
    }
    
    #[test]
    fn gets_empty() {
        let q = SpscConcurrentQueue::<i32>::with_capacity(2);
        assert_eq!(None, q.poll());
        assert!(q.is_empty()); 
    }
    
    #[test]
    fn peeks_a_value() {
        let q = SpscConcurrentQueue::<i32>::with_capacity(2);
        assert_eq!(None, q.offer(34));
        assert_eq!(Some(34), q.peek());
        assert_eq!(Some(34), q.poll());
        assert_eq!(None, q.poll());
    }

    #[derive(Debug)]
    struct Payload {
        value: u64,
        dropped: Arc<AtomicBool>
    }

    impl Clone for Payload {
        fn clone(&self) -> Payload {
            let is_dropped = self.dropped.load(Ordering::Relaxed);
            Payload { 
                value: self.value, 
                dropped: Arc::new(AtomicBool::new(is_dropped))
            }
        }
    }

    impl Drop for Payload {
        fn drop(&mut self) {
            self.dropped.store(true, Ordering::Relaxed);
        }
    }

    impl PartialEq<Payload> for Payload {
        fn eq(&self, other: &Payload) -> bool {
            self.value == other.value
        }
    }

    #[test]
    fn items_are_moved() {
        let q = SpscConcurrentQueue::<Payload>::with_capacity(2);
        let dropped = Arc::new(AtomicBool::new(false));
        let p1 = Payload { value: 67, dropped: dropped.clone() };
        assert!(q.is_empty());
        assert_eq!(None, q.offer(p1));
        let p2 = q.poll().unwrap();
        assert_eq!(67, p2.value);
        assert!(!dropped.load(Ordering::Relaxed));
        mem::drop(p2);
        assert!(dropped.load(Ordering::Relaxed));
    }

    #[test]
    fn peeked_items_are_cloned() {
        let q = SpscConcurrentQueue::<Payload>::with_capacity(2);
        let dropped = Arc::new(AtomicBool::new(false));
        let p1 = Payload { value: 67, dropped: dropped.clone() };
        assert!(q.is_empty());
        assert_eq!(None, q.offer(p1));
        let p2 = q.peek().unwrap();
        let dropped2 = p2.dropped.clone();
        assert_eq!(67, p2.value);
        assert!(!dropped.load(Ordering::Relaxed));
        assert!(!dropped2.load(Ordering::Relaxed));
        mem::drop(p2);
        assert!(!dropped.load(Ordering::Relaxed));
        assert!(dropped2.load(Ordering::Relaxed));
    }

    #[test]
    fn lost_items_are_dropped()  {
        let q = SpscConcurrentQueue::<Payload>::with_capacity(2);
        let dropped = Arc::new(AtomicBool::new(false));
        let p = Payload { value: 67, dropped: dropped.clone() };
        assert_eq!(None, q.offer(p));
        assert_eq!(1, q.size());
        assert!(!dropped.load(Ordering::Relaxed));
        mem::drop(q);
        assert!(dropped.load(Ordering::Relaxed));
    }

    #[test]
    fn two_threads_can_add_and_remove() {
        const REPETITIONS: u64 = 10 * 1000 * 1000;
        let q = SpscConcurrentQueue::<u64>::with_capacity(1024);
        let barrier = Arc::new(Barrier::new(2));
        let cb = barrier.clone();
        let cq = q.clone();
        let c = thread::spawn(move|| {
            cb.wait();
            for i in 0..REPETITIONS {
                let mut opt: Option<u64>;
                while { 
                    opt = cq.poll();
                    opt.is_none()
                } {
                    thread::yield_now();
                }
                assert_eq!(i, opt.unwrap());
            }
        });
        let pc = barrier.clone();
        let pq = q.clone();
        let p = thread::spawn(move|| {
            pc.wait();
            for i in 0..REPETITIONS {
                while pq.offer(i).is_some() {
                    thread::yield_now();
                }
            }
        });
        c.join().unwrap();
        p.join().unwrap();
    }
}

