/// Trait that concurrent, non-blocking queues implement.
pub trait ConcurrentQueue<T> {

    /// Tries to put a value onto the queue.
    ///
    /// If the queue is not full, the method returns `None`, signifying success.
    /// If the queue is full, it returns `Some(v)`, where `v` is the original, 
    /// specified value.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmo::collection::{ConcurrentQueue, SpscConcurrentQueue};
    /// let queue = SpscConcurrentQueue::<u64>::with_capacity(16);
    /// match queue.offer(10) {
    ///     Some(v) => println!("Queue is full"),
    ///     None => println!("Value added to the queue")
    /// }
    /// ```
    fn offer(&self, val: T) -> Option<T>;

    /// Tries to remove a value from the queue.
    ///
    /// If the queue is not empty, the method returns `Some(v)`, effectively
    /// removing `v` from the queue. If the queue is empty, it returns `None`.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmo::collection::{ConcurrentQueue, SpscConcurrentQueue};
    /// let queue = SpscConcurrentQueue::<u64>::with_capacity(16);
    /// match queue.poll() {
    ///     Some(v) => println!("Removed item {}", v),
    ///     None => println!("Queue is empty")
    /// }
    /// ```
    fn poll(&self) -> Option<T>;
    
    /// Tries to peek a value from the queue.
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
    fn peek(&self) -> Option<T>;

    /// Returns the capacity of the queue.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmo::collection::{ConcurrentQueue, SpscConcurrentQueue};
    /// let queue = SpscConcurrentQueue::<u64>::with_capacity(16);
    /// assert_eq!(16, queue.capacity());
    /// queue.offer(10);
    /// assert_eq!(16, queue.capacity());
    /// ```
    fn capacity(&self) -> usize;

    /// Returns how many items are in the queue.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmo::collection::{ConcurrentQueue, SpscConcurrentQueue};
    /// let queue = SpscConcurrentQueue::<u64>::with_capacity(16);
    /// assert_eq!(0, queue.size());
    /// queue.offer(10);
    /// assert_eq!(1, queue.size());
    /// ```
    fn size(&self) -> usize;

    /// Tells whether the queue is empty or not.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmo::collection::{ConcurrentQueue, SpscConcurrentQueue};
    /// let queue = SpscConcurrentQueue::<u64>::with_capacity(16);
    /// if queue.is_empty() {
    ///     queue.offer(10);
    /// }
    /// assert_eq!(Some(10), queue.poll());
    /// ```
    fn is_empty(&self) -> bool {
        self.size() == 0
    }

    /// Tells whether the queue is full or not.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmo::collection::{ConcurrentQueue, SpscConcurrentQueue};
    /// let queue = SpscConcurrentQueue::<u64>::with_capacity(16);
    /// if !queue.is_full() {
    ///     queue.offer(10);
    /// }
    /// assert_eq!(Some(10), queue.poll());
    /// ```
    fn is_full(&self) -> bool {
        self.capacity() == self.size()
    }
}

