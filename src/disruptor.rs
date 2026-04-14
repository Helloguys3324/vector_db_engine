use std::sync::atomic::{AtomicUsize, Ordering};
use crossbeam_utils::CachePadded;

const MAX_PAYLOAD_SIZE: usize = 1024;

/// A lock-free, zero-allocation Ring Buffer using the Disruptor pattern.
/// Prevents false sharing using Cache-Line padding.
pub struct HandoffQueue {
    // Array of pre-allocated buffers.
    buffer: Vec<QueueItem>,
    capacity: usize,
    mask: usize,
    
    // Sequence barriers
    head: CachePadded<AtomicUsize>,
    tail: CachePadded<AtomicUsize>,
}

#[derive(Clone)]
struct QueueItem {
    data: [u8; MAX_PAYLOAD_SIZE],
    len: usize,
}

impl HandoffQueue {
    pub fn new(capacity: usize) -> Self {
        // Enforce power of 2 for fast modulo tracking
        assert!(capacity.is_power_of_two());
        
        let buffer = vec![QueueItem { data: [0; MAX_PAYLOAD_SIZE], len: 0 }; capacity];

        Self {
            buffer,
            capacity,
            mask: capacity - 1,
            head: CachePadded::new(AtomicUsize::new(0)),
            tail: CachePadded::new(AtomicUsize::new(0)),
        }
    }

    /// Lock-free enqueue. O(1).
    #[inline]
    pub fn enqueue(&self, msg: &str) -> bool {
        let current_tail = self.tail.load(Ordering::Relaxed);
        let current_head = self.head.load(Ordering::Acquire);

        // Queue full check
        if current_tail.wrapping_sub(current_head) >= self.capacity {
            return false;
        }

        let idx = current_tail & self.mask;
        
        // Zero-copy byte map
        let bytes = msg.as_bytes();
        let len = bytes.len().min(MAX_PAYLOAD_SIZE);
        
        // Convert safe pointer to mutable pointer for ring buffer internal access
        // (In a real bare-metal framework, we use UnsafeCell wrapped items)
        unsafe {
            let item_ptr = self.buffer.as_ptr().add(idx) as *mut QueueItem;
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), (*item_ptr).data.as_mut_ptr(), len);
            (*item_ptr).len = len;
        }

        // Commit sequence
        self.tail.store(current_tail.wrapping_add(1), Ordering::Release);
        true
    }

    /// L2 Consumes messages from here.
    #[inline]
    pub fn dequeue<F>(&self, mut callback: F) -> bool 
    where 
        F: FnMut(&[u8]) 
    {
        let current_head = self.head.load(Ordering::Relaxed);
        let current_tail = self.tail.load(Ordering::Acquire);

        // Empty check
        if current_head == current_tail {
            return false;
        }

        let idx = current_head & self.mask;
        
        unsafe {
            let item_ptr = self.buffer.as_ptr().add(idx) as *const QueueItem;
            let slice = std::slice::from_raw_parts((*item_ptr).data.as_ptr(), (*item_ptr).len);
            callback(slice);
        }

        // Commit consume
        self.head.store(current_head.wrapping_add(1), Ordering::Release);
        true
    }
}
