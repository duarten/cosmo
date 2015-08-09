extern crate core;

use self::core::ptr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::buffer::{Buffer, value_ptr};
use super::concurrent_queue::ConcurrentQueue;

/// A bounded queue allowing a single producer and multiple consumers.
pub struct SpmcConcurrentQueue<T> {
    buffer: Buffer<T>,
    _pad: [u64; 15],
    /// This is separate from the `head` index, which is expected to
    /// be highly contented, in the hope it stays in a Shared cache 
    /// line that's rarely invalidated.
    tail_cache: AtomicUsize 
}

impl<T> SpmcConcurrentQueue<T> {
    /// Creates a single producer multiple consumer queue with the specified 
    /// capacity.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmo::collection::{ConcurrentQueue, SpmcConcurrentQueue};
    /// let q = SpmcConcurrentQueue::<u64>::with_capacity(1024);
    /// assert_eq!(1024, q.capacity());
    /// ```
    pub fn with_capacity(initial_capacity: usize) -> Arc<SpmcConcurrentQueue<T>> {
        Arc::new(SpmcConcurrentQueue { 
            buffer: Buffer::with_capacity(initial_capacity),
            tail_cache: AtomicUsize::new(0),
            _pad: [0; 15]
        })
    }
}

impl<T> ConcurrentQueue<T> for SpmcConcurrentQueue<T> {
    /// Puts an item in the queue. This method only reads and modifies the 
    /// `tail` index, thus avoiding cache line ping-ponging.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmo::collection::{ConcurrentQueue, SpmcConcurrentQueue};
    /// let q = SpmcConcurrentQueue::<u64>::with_capacity(1024);
    /// assert_eq!(None, q.offer(10));
    /// ```
    fn offer(&self, val: T) -> Option<T> {
        let index = self.buffer.tail.load(Ordering::Relaxed);
        unsafe { 
            let item = self.buffer.item(index);
            if item.is_defined.load(Ordering::Acquire) {
                return Some(val)
            }
            ptr::write(value_ptr(item), val);
            item.is_defined.store(true, Ordering::Relaxed);
            self.buffer.tail.store(index + 1, Ordering::Release);
            None
        }
    }

    /// Takes an item from the queue. This method uses the `head` index to
    /// synchronize multiple consumers. The tail position is read from the
    /// `tail_cache` to avoid cache line ping-ponging between the producer 
    /// and the consumers.
    ///
    /// # Example
    ///
    /// ```
    /// use cosmo::collection::{ConcurrentQueue, SpmcConcurrentQueue};
    /// let q = SpmcConcurrentQueue::<u64>::with_capacity(1024);
    /// q.offer(10);
    /// assert_eq!(Some(10), q.poll());
    /// ```
    fn poll(&self) -> Option<T> {
        loop {
            let index = self.buffer.head.load(Ordering::Acquire);
            // We do a relaxed load, which means a speculative load of 
            // `self.buffer.tail` is allowed and harmless. 
            let current_tail_cache = self.tail_cache.load(Ordering::Relaxed);
            if index >= current_tail_cache {
                let current_tail = self.buffer.tail.load(Ordering::Relaxed);
                if index >= current_tail {
                    return None
                }
                self.tail_cache.store(current_tail, Ordering::Relaxed);
            }
            if cas_head(self, index, Ordering::Release) {
                unsafe {
                    let item = self.buffer.item(index);
                    let res = ptr::read(value_ptr(item));
                    item.is_defined.store(false, Ordering::Release);
                    return Some(res)
                }
            }
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

#[inline]
fn cas_head<T>(q: &SpmcConcurrentQueue<T>, head: usize, order: Ordering) -> bool {
    q.buffer.head.compare_and_swap(head, head + 1, order) == head
}

#[cfg(test)]
mod test {
    use super::core::mem;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Barrier};
    use std::thread;
    use super::SpmcConcurrentQueue;
    use super::super::concurrent_queue::ConcurrentQueue;

    #[test]
    fn capacity_is_next_power_of_two() {
        assert_eq!(16, SpmcConcurrentQueue::<i32>::with_capacity(10).capacity());
    }

    #[test]
    fn adds_and_removes_a_value() {
        let q = SpmcConcurrentQueue::<i32>::with_capacity(2);
        assert_eq!(None, q.offer(34));
        assert_eq!(Some(34), q.poll());
    }
    
    #[test]
    fn gets_full() {
        let q = SpmcConcurrentQueue::<i32>::with_capacity(2);
        assert_eq!(None, q.offer(1));
        assert_eq!(None, q.offer(2));
        assert_eq!(Some(3), q.offer(3));
        assert!(q.is_full()); 
    }
    
    #[test]
    fn gets_empty() {
        let q = SpmcConcurrentQueue::<i32>::with_capacity(2);
        assert_eq!(None, q.poll());
        assert!(q.is_empty()); 
    }

    #[derive(Debug)]
    struct Payload {
        value: u64,
        dropped: Arc<AtomicBool>
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
        let q = SpmcConcurrentQueue::<Payload>::with_capacity(2);
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
    fn lost_items_are_dropped()  {
        let q = SpmcConcurrentQueue::<Payload>::with_capacity(2);
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
        const REPETITIONS: u64 = 5 * 1000 * 1000;
        let q = SpmcConcurrentQueue::<u64>::with_capacity(1024);
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

    #[test]
    fn one_producer_two_consumers() {
        const REPETITIONS: usize = 5 * 1000 * 1000;
        let q = SpmcConcurrentQueue::<usize>::with_capacity(1024);
        let barrier = Arc::new(Barrier::new(3));
        let done = Arc::new(AtomicBool::new(false));
        let mut consumers = Vec::<thread::JoinHandle<Vec<usize>>>::with_capacity(2);
        for _ in 0..2 {
            let barrier = barrier.clone();
            let q = q.clone();
            let done= done.clone();
            consumers.push(thread::spawn(move|| {
                let mut vals = Vec::<usize>::with_capacity(REPETITIONS);
                barrier.wait();
                loop { 
                    match q.poll() {
                        Some(val) => vals.push(val),
                        None => {
                            if done.load(Ordering::Acquire) && q.is_empty() {
                                break;
                            }
                            thread::yield_now();
                        }
                    }
                }
                vals
            }));
        }
        thread::spawn(move|| {
            barrier.wait();
            for i in 0..REPETITIONS {
                while q.offer(i).is_some() {
                    thread::yield_now();
                }
            }
            done.store(true, Ordering::Release);
        }).join().unwrap();

        let mut all_vals = Vec::<usize>::with_capacity(REPETITIONS);
        for c in consumers { 
            all_vals.extend(&c.join().unwrap()); 
        }

        all_vals.sort();
        for i in 0..REPETITIONS {
            assert_eq!(i, *all_vals.get(i).unwrap());
        }
    }
}

