extern crate alloc;
extern crate core;

use self::alloc::heap::{allocate, deallocate};
use self::core::{mem, ptr};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// A data item, containing a value of type T and a flag indicating whether 
/// the value is valid and can be consumed.
#[repr(C)]
pub struct Item<T> {
    pub is_defined: AtomicBool,
    value: T
}

/// The implementation-agnostic representation of a bounded concurrent queue.
///
/// It holds the producer and consumer indexes into the ring buffer, as well 
/// as a contiguous block of memory that contains the data items. It includes
/// padding between producer and consumer fields to avoid false sharing.
#[repr(C)]
pub struct Buffer<T> {
    _pad1: [u64; 15],
    pub tail: AtomicUsize,
    _pad2: [u64; 15],
    pub head: AtomicUsize,
    _pad3: [u64; 15],
    pub capacity: usize,
    mask: usize,
    items: *mut Item<T>
}

impl<T> Buffer<T> {
    /// Creates a Buffer with the specified capacity, which is rounded to
    /// the next power of two.
    pub fn with_capacity(min_capacity: usize) -> Buffer<T> {
        let capacity = min_capacity.next_power_of_two();
        Buffer { 
            tail: AtomicUsize::new(0),
            head: AtomicUsize::new(0), 
            capacity: capacity,
            mask: capacity - 1,
            items: unsafe { allocate_buffer(capacity) },
            _pad1: [0; 15],
            _pad2: [0; 15],
            _pad3: [0; 15],
        }
    }

    /// Returns the data item at the position specified by the index.
    #[inline]
    pub unsafe fn item(&self, index: usize) -> &mut Item<T> {
        &mut*self.items.offset((index & self.mask) as isize)
    }

    /// Uses the head and tail indexes to return the number of items 
    /// in the buffer.
    pub fn size(&self) -> usize {
        let mut head_after = self.head.load(Ordering::Acquire);
        loop {
            let head_before = head_after;
            let current_tail = self.tail.load(Ordering::Acquire);
            head_after = self.head.load(Ordering::Relaxed);
            if head_before == head_after {
                return current_tail - head_after;
            }
        }
    }
}

/// Allocates and initializes a block of heap memory for the data items.
unsafe fn allocate_buffer<T>(capacity: usize) -> *mut Item<T> {
    let size = capacity
        .checked_mul(mem::size_of::<Item<T>>())
        .expect("capacity exceeded");
    let ptr = allocate(size, mem::align_of::<Item<T>>()) as *mut Item<T>;
    if ptr.is_null() { self::alloc::oom() }
    for index in 0..capacity as isize {
        (*ptr.offset(index)).is_defined.store(false, Ordering::Relaxed);
    }
    ptr
}

/// Returns a pointer to the value of a data item.
#[inline]
pub unsafe fn value_ptr<T>(item: *mut Item<T>) -> *mut T {
    (item as *mut u8).offset(mem::size_of::<AtomicBool>() as isize) as *mut T
}

unsafe impl<T: Send> Send for Buffer<T> { }
unsafe impl<T> Sync for Buffer<T> { }

/// Deallocates the heap memory when the buffer is dropped, ensuring
/// the items still in the queue are themselves dropped.
impl<T> Drop for Buffer<T> {
    fn drop(&mut self) {
        unsafe {
            let head = self.head.load(Ordering::Relaxed);
            let tail = self.tail.load(Ordering::Relaxed);
            for index in head..tail {
                let ptr = value_ptr(self.item(index));
                mem::drop(ptr::read(ptr));
            }
            deallocate(
                self.items as *mut u8,
                self.capacity * mem::size_of::<Item<T>>(),
                mem::align_of::<Item<T>>());
        }
    }
}

