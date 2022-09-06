use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    task::{Context, Poll, Waker},
};

use futures_util::{future::poll_fn, task::AtomicWaker, Stream};

use crate::sync::{RecvError, SendError, TryRecvError, TrySendError};

#[repr(align(128))]
struct ChannelWaker {
    waker: AtomicWaker,
    should_wake: AtomicBool,
}

impl ChannelWaker {
    #[inline]
    fn register(&self, w: &Waker) {
        self.waker.register(w);
        self.should_wake.store(true, Ordering::Release);
    }

    #[inline]
    fn wake(&self) {
        if self.should_wake.swap(false, Ordering::Acquire) {
            self.waker.wake();
        }
    }

    #[inline]
    fn take(&self) {
        self.should_wake.store(false, Ordering::Relaxed);
    }
}

struct Shared {
    sender_waker: ChannelWaker,
    receiver_waker: ChannelWaker,
}

/// A handle to the channel which allows sending values.
pub struct Sender<T: Send> {
    inner: crate::sync::queue::spsc::Sender<T>,
    shared: Arc<Shared>,
}

impl<T: Send> Sender<T> {
    /// Attempt to push a value onto the channel. Returns an error
    /// if the receiver has been disconnected or the queue is full.
    #[inline]
    pub fn try_send(&self, val: T) -> Result<(), TrySendError<T>> {
        match self.inner.try_send(val) {
            Ok(()) => {
                // Maybe wake the receiver since the channel might have become non-empty.
                self.shared.receiver_waker.wake();
                Ok(())
            }
            Err(err) => Err(err),
        }
    }

    /// Checks whether a value can be pushed onto the queue. Returns
    /// `Poll::Ready` if there are free slots, otherwise, returns
    /// `Poll::Pending`.
    #[inline]
    pub fn poll_ready(&self, cx: &mut Context<'_>) -> Poll<()> {
        self.shared.sender_waker.register(cx.waker());
        if self.inner.is_full() {
            Poll::Pending
        } else {
            // Cancel our waker.
            self.shared.sender_waker.take();
            Poll::Ready(())
        }
    }

    /// Pushes a value onto the channel, waiting if the channel is full.
    pub async fn send(&self, mut val: T) -> Result<(), SendError<T>> {
        loop {
            match self.try_send(val) {
                Ok(()) => return Ok(()),
                Err(TrySendError::Disconnected(val)) => return Err(SendError::Disconnected(val)),
                Err(TrySendError::Full(pending)) => {
                    poll_fn(|cx| self.poll_ready(cx)).await;
                    val = pending;
                }
            }
        }
    }
}

/// A handle to the channel which allows receiving values.
pub struct Receiver<T: Send> {
    inner: crate::sync::queue::spsc::Receiver<T>,
    shared: Arc<Shared>,
}

impl<T: Send> Receiver<T> {
    /// Attempt to receive a value from the queue. Returns an error if the
    /// sender has been disconnected or the queue is empty.
    #[inline]
    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        match self.inner.try_recv() {
            Ok(val) => {
                // Maybe wake the sender since the channel might have become non-full.
                self.shared.sender_waker.wake();
                Ok(val)
            }
            Err(err) => Err(err),
        }
    }

    /// Attempt to receive a value from the queue. Returns `Poll::Ready` if
    /// there's an error or a value is received. Otherwise, returns
    /// `Poll::Pending`.
    #[inline]
    pub fn poll_recv(&self, cx: &mut Context<'_>) -> Poll<Result<T, RecvError>> {
        match self.try_recv() {
            Ok(item) => Poll::Ready(Ok(item)),
            Err(TryRecvError::Disconnected) => Poll::Ready(Err(RecvError::Disconnected)),
            Err(TryRecvError::Empty) => {
                // The queue is empty, so register the waker.
                self.shared.receiver_waker.register(cx.waker());
                // Maybe we raced with a send, so try again.
                match self.try_recv() {
                    Ok(item) => {
                        // Cancel our waker (which may have been signaled in the meanwhile).
                        self.shared.receiver_waker.take();
                        Poll::Ready(Ok(item))
                    }
                    Err(TryRecvError::Disconnected) => Poll::Ready(Err(RecvError::Disconnected)),
                    Err(TryRecvError::Empty) => Poll::Pending,
                }
            }
        }
    }

    /// Receives a value from the queue, waiting if the channel is empty.
    pub async fn recv(&self) -> Result<T, RecvError> {
        poll_fn(|cx| self.poll_recv(cx)).await
    }
}

impl<T: Send> Stream for Receiver<T> {
    type Item = Result<T, RecvError>;

    fn poll_next(self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.poll_recv(cx) {
            Poll::Ready(Ok(item)) => Poll::Ready(Some(Ok(item))),
            Poll::Ready(Err(err)) => Poll::Ready(Some(Err(err))),
            Poll::Pending => Poll::Pending,
        }
    }
}

/// Creates a channel with at least the given capacity.
pub fn bounded<T: Send>(capacity: usize) -> (Sender<T>, Receiver<T>) {
    let shared = Arc::new(Shared {
        sender_waker: ChannelWaker {
            waker: AtomicWaker::new(),
            should_wake: AtomicBool::new(false),
        },
        receiver_waker: ChannelWaker {
            waker: AtomicWaker::new(),
            should_wake: AtomicBool::new(false),
        },
    });
    let (tx, rx) = crate::sync::queue::spsc::bounded(capacity);
    (
        Sender {
            inner: tx,
            shared: shared.clone(),
        },
        Receiver { inner: rx, shared },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn two_threads_can_add_and_remove() {
        const REPETITIONS: u64 = 10_000_000;
        let (tx, rx) = bounded(1024);
        let c = tokio::task::spawn(async move {
            for i in 0..REPETITIONS {
                let val = rx.recv().await.unwrap();
                assert_eq!(i, val);
            }
        });
        let p = tokio::task::spawn(async move {
            for i in 0..REPETITIONS {
                tx.send(i).await.unwrap();
            }
        });
        c.await.unwrap();
        p.await.unwrap();
    }
}
