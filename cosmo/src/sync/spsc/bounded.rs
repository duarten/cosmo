//! A bounded single-producer, single-consumer concurrent queue.

use std::{
    cell::{Cell, UnsafeCell},
    mem::MaybeUninit,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
};

use crate::sync::{TryRecvError, TrySendError};

#[derive(Debug)]
#[repr(align(128))]
struct SenderCacheline {
    index: AtomicUsize,
    lookahead_limit: Cell<usize>,
    receiver_disconnected: AtomicBool,
}

// SAFETY: The `lookahead_limit` field is confined to a single thread.
unsafe impl Sync for SenderCacheline {}

#[derive(Debug)]
#[repr(align(128))]
struct ReceiverCacheline {
    index: AtomicUsize,
    producer_disconnected: AtomicBool,
}

/// A slot in the queue. When `has_value` is set to true,
/// `value` contains an instance of type `T`.
#[derive(Debug)]
pub(crate) struct Slot<T: Send> {
    value: UnsafeCell<MaybeUninit<T>>,
    has_value: AtomicBool,
}

// SAFETY: The queue semantics and the `has_value` field ensure no two threads
// will access the value of the same slot concurrently.
unsafe impl<T: Send> Sync for Slot<T> {}

impl<T: Send> Drop for Slot<T> {
    fn drop(&mut self) {
        let _ = self.take();
    }
}

impl<T: Send> Slot<T> {
    fn take(&self) -> Option<T> {
        self.has_value.load(Ordering::Acquire).then(|| {
            // SAFETY: We know the value is initialized and valid for reads.
            let item = unsafe { (*self.value.get()).assume_init_read() };
            self.has_value.store(false, Ordering::Release);
            item
        })
    }

    /// Writes the given value into this slot.
    fn write(&self, val: T) {
        // SAFETY: The slot is always valid of writes.
        unsafe {
            (*self.value.get()) = MaybeUninit::new(val);
        }
        self.has_value.store(true, Ordering::Release);
    }
}

/// The internal memory buffer used by the queue.
///
/// `Buffer` holds a pointer to allocated memory which represents the bounded
/// ring buffer, as well as the producer and consumer indexes.
pub(crate) struct Buffer<T: Send> {
    storage: Box<[Slot<T>]>,
    mask: usize,
    lookahead: usize,

    sender: SenderCacheline,
    receiver: ReceiverCacheline,
}

impl<T: Send> Buffer<T> {
    fn has_space(&self, tail: usize) -> bool {
        let index = (tail + self.lookahead) & self.mask;
        if !self.storage[index].has_value.load(Ordering::Acquire) {
            self.sender.lookahead_limit.set(tail + self.lookahead);
            true
        } else if tail < self.capacity() {
            !self.storage[tail].has_value.load(Ordering::Acquire)
        } else {
            false
        }
    }

    /// Returns true if the channel is empty.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns true if the channel is full.
    fn is_full(&self) -> bool {
        self.len() == self.capacity()
    }

    /// Returns the number of values in the queue.
    fn len(&self) -> usize {
        self.sender
            .index
            .load(Ordering::Acquire)
            .saturating_sub(self.receiver.index.load(Ordering::Acquire))
    }

    /// Returns the capacity of the queue.
    fn capacity(&self) -> usize {
        self.storage.len()
    }
}

/// A handle to the queue which allows sending values.
pub struct Sender<T: Send> {
    buffer: Arc<Buffer<T>>,
}

impl<T: Send> Drop for Sender<T> {
    fn drop(&mut self) {
        self.buffer
            .receiver
            .producer_disconnected
            .store(true, Ordering::Relaxed);
    }
}

impl<T: Send> Sender<T> {
    /// Attempt to push a value onto the queue. Returns an error
    /// if the receiver has been disconnected or the queue is full.
    #[inline]
    pub fn try_send(&self, val: T) -> Result<(), TrySendError<T>> {
        let buf = &self.buffer;
        let sender = &buf.sender;

        // Check if there is a connected receiver.
        if sender.receiver_disconnected.load(Ordering::Relaxed) {
            return Err(TrySendError::Disconnected(val));
        }

        // Check if there's space to insert the new item.
        let tail = sender.index.load(Ordering::Relaxed);
        if tail >= sender.lookahead_limit.get() && !buf.has_space(tail) {
            return Err(TrySendError::Full(val));
        }

        // There's a receiver and we have space, so update the buffer.
        buf.storage[tail & buf.mask].write(val);
        // Update the index with [`Ordering::Release`] for `size()`.
        sender.index.store(tail + 1, Ordering::Release);
        Ok(())
    }

    /// Returns true if the channel is empty.
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// Returns true if the channel is full.
    pub fn is_full(&self) -> bool {
        self.buffer.is_full()
    }

    /// Returns the number of values in the queue.
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    /// Returns the capacity of the queue.
    pub fn capacity(&self) -> usize {
        self.buffer.capacity()
    }
}

/// A handle to the queue which allows receiving values.
pub struct Receiver<T: Send> {
    buffer: Arc<Buffer<T>>,
}

impl<T: Send> Drop for Receiver<T> {
    fn drop(&mut self) {
        self.buffer
            .sender
            .receiver_disconnected
            .store(true, Ordering::Relaxed);
    }
}

impl<T: Send> Receiver<T> {
    /// Attempt to receive a value from the queue. Returns an error if the
    /// sender has been disconnected or the queue is empty.
    #[inline]
    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        let buf = &self.buffer;
        let receiver = &buf.receiver;

        // Try to load the value.
        let head = receiver.index.load(Ordering::Relaxed);
        // There's a receiver and we have space, so update the buffer.
        if let Some(val) = buf.storage[head & buf.mask].take() {
            // Update the index with [`Ordering::Release`] for `size()`.
            receiver.index.store(head + 1, Ordering::Release);
            Ok(val)
        } else {
            // Check if there is a connected sender.
            if receiver.producer_disconnected.load(Ordering::Relaxed) {
                Err(TryRecvError::Disconnected)
            } else {
                Err(TryRecvError::Empty)
            }
        }
    }

    // /// Attempt to receive as many as `limit` values from the queue.
    // /// Returns an error if the sender has been disconnected.
    // #[inline]
    // pub fn try_recv_many(
    //     &self,
    //     mut out: impl Extend<T>,
    //     limit: usize,
    // ) -> Result<usize, TryRecvError> {
    //     let buf = &self.buffer;
    //     let receiver = &buf.receiver;

    //     // Receive min(limit, self.len()) values.
    //     let head = receiver.index.load(Ordering::Relaxed);
    //     let mut received = 0;
    //     for idx in head..(head + limit) {
    //         if let Some(val) = buf.storage[idx & buf.mask].take() {
    //             out.extend(Some(val));
    //         } else {
    //             break;
    //         }
    //         received += 1;
    //     }

    //     // Check if there is a connected sender.
    //     if received == 0 &&
    // receiver.producer_disconnected.load(Ordering::Relaxed) {         return
    // Err(TryRecvError::Disconnected);     }
    //     receiver.index.store(head + received, Ordering::Release);
    //     Ok(received)
    // }

    /// Returns true if all senders for this channel have been dropped.
    pub fn is_disconnected(&self) -> bool {
        self.buffer
            .receiver
            .producer_disconnected
            .load(Ordering::Relaxed)
    }

    /// Returns true if the channel is empty.
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// Returns true if the channel is full.
    pub fn is_full(&self) -> bool {
        self.buffer.is_full()
    }

    /// Returns the number of values in the queue.
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    /// Returns the capacity of the queue.
    pub fn capacity(&self) -> usize {
        self.buffer.capacity()
    }
}

/// Creates a queue with at least the given capacity.
pub fn bounded<T: Send>(capacity: usize) -> (Sender<T>, Receiver<T>) {
    let capacity = capacity.next_power_of_two();
    let storage: Box<[Slot<T>]> = (0..capacity)
        .map(|_| Slot {
            has_value: AtomicBool::default(),
            value: UnsafeCell::new(MaybeUninit::uninit()),
        })
        .collect();
    let buffer = Arc::new(Buffer {
        storage,
        mask: capacity - 1,
        lookahead: capacity / 4,
        sender: SenderCacheline {
            index: AtomicUsize::default(),
            lookahead_limit: Cell::new(capacity),
            receiver_disconnected: AtomicBool::default(),
        },
        receiver: ReceiverCacheline {
            index: AtomicUsize::default(),
            producer_disconnected: AtomicBool::default(),
        },
    });
    (
        Sender {
            buffer: buffer.clone(),
        },
        Receiver { buffer },
    )
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, thread};

    use super::*;

    #[test]
    fn test_capacity_is_power_of_two() {
        assert_eq!(16, bounded::<usize>(10).0.capacity());
    }

    #[test]
    fn adds_and_removes_a_value() {
        let (tx, rx) = bounded::<usize>(2);
        assert_eq!(Ok(()), tx.try_send(34));
        assert_eq!(Ok(34), rx.try_recv());
    }

    #[test]
    fn becomes_full() {
        let (tx, _rx) = bounded::<usize>(2);
        assert!(tx.is_empty());
        assert_eq!(Ok(()), tx.try_send(1));
        assert_eq!(Ok(()), tx.try_send(2));
        assert!(matches!(tx.try_send(3), Err(TrySendError::Full(3))));
        assert!(tx.is_full());
    }

    #[test]
    fn becomes_empty() {
        let (_tx, rx) = bounded::<usize>(2);
        assert!(matches!(rx.try_recv(), Err(TryRecvError::Empty)));
    }

    #[test]
    fn lost_items_are_dropped() {
        let item = Arc::new(10);
        {
            let (tx, _rx) = bounded(2);
            assert!(tx.try_send(item.clone()).is_ok());
            assert_eq!(2, Arc::strong_count(&item));
        }
        assert_eq!(1, Arc::strong_count(&item));
    }

    #[test]
    fn disconnecting_a_receiver_produces_an_error_when_sending() {
        let (tx, _) = bounded(2);
        assert!(matches!(
            tx.try_send(10),
            Err(TrySendError::Disconnected(10))
        ));
    }

    #[test]
    fn disconnecting_a_sender_produces_an_error_when_receiving() {
        let (tx, rx) = bounded(2);
        {
            let tx = tx;
            let _ = tx.try_send(10);
        }
        assert!(matches!(rx.try_recv(), Ok(10)));
        assert!(matches!(rx.try_recv(), Err(TryRecvError::Disconnected)));
    }

    #[test]
    fn two_threads_can_add_and_remove() {
        const REPETITIONS: u64 = 10_000_000;
        let (tx, rx) = bounded(1024);
        let c = thread::spawn(move || {
            for i in 0..REPETITIONS {
                let mut opt: Result<u64, TryRecvError>;
                while {
                    opt = rx.try_recv();
                    opt.is_err()
                } {
                    thread::yield_now();
                }
                assert_eq!(i, opt.unwrap());
            }
        });
        let p = thread::spawn(move || {
            for i in 0..REPETITIONS {
                while tx.try_send(i).is_err() {
                    thread::yield_now();
                }
            }
        });
        c.join().unwrap();
        p.join().unwrap();
    }
}
