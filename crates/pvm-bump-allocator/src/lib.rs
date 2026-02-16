//! A simple bump allocator for PVM smart contracts.
//!
//! This allocator is designed for use in `no_std` environments where memory is never freed
//! (e.g., short-lived smart contract executions). It simply bumps a pointer forward for each
//! allocation and never reclaims memory on `dealloc`.
//!
//! Adapted for the PVM contract toolchain.
//!
//! # Usage
//!
//! ```ignore
//! #[global_allocator]
//! static ALLOC: pvm_bump_allocator::BumpAllocator<{ 1024 * 1024 }> =
//!     pvm_bump_allocator::BumpAllocator::new();
//! ```

#![no_std]

use core::alloc::{GlobalAlloc, Layout};
use core::sync::atomic::{AtomicUsize, Ordering};

/// A bump allocator backed by a fixed-size heap.
///
/// `HEAP_SIZE` is the total number of bytes available for allocation.
/// Memory is never freed — `dealloc` is a no-op.
pub struct BumpAllocator<const HEAP_SIZE: usize> {
    offset: AtomicUsize,
    heap: core::cell::UnsafeCell<[u8; HEAP_SIZE]>,
}

// SAFETY: The allocator uses atomic operations for the offset, so it is safe to share
// across threads (though PVM contracts are single-threaded, this satisfies the
// `GlobalAlloc` requirement).
unsafe impl<const HEAP_SIZE: usize> Sync for BumpAllocator<HEAP_SIZE> {}

impl<const HEAP_SIZE: usize> Default for BumpAllocator<HEAP_SIZE> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const HEAP_SIZE: usize> BumpAllocator<HEAP_SIZE> {
    /// Creates a new bump allocator with a zeroed heap of `HEAP_SIZE` bytes.
    pub const fn new() -> Self {
        Self {
            offset: AtomicUsize::new(0),
            heap: core::cell::UnsafeCell::new([0u8; HEAP_SIZE]),
        }
    }
}

unsafe impl<const HEAP_SIZE: usize> GlobalAlloc for BumpAllocator<HEAP_SIZE> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let align = layout.align();
        let size = layout.size();

        let mut current = self.offset.load(Ordering::Relaxed);

        loop {
            let aligned = (current + align - 1) & !(align - 1);
            let Some(next) = aligned.checked_add(size) else {
                return core::ptr::null_mut();
            };

            if next > HEAP_SIZE {
                return core::ptr::null_mut();
            }

            match self.offset.compare_exchange_weak(
                current,
                next,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => {
                    let heap_ptr = self.heap.get() as *mut u8;
                    return unsafe { heap_ptr.add(aligned) };
                }
                Err(observed) => current = observed,
            }
        }
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::alloc::Layout;

    #[test]
    fn alloc_single_byte() {
        let alloc = BumpAllocator::<1024>::new();
        let layout = Layout::from_size_align(1, 1).unwrap();
        let ptr = unsafe { alloc.alloc(layout) };
        assert!(!ptr.is_null());
    }

    #[test]
    fn alloc_aligned() {
        let alloc = BumpAllocator::<1024>::new();

        // Allocate 1 byte to misalign the offset
        let layout1 = Layout::from_size_align(1, 1).unwrap();
        unsafe { alloc.alloc(layout1) };

        // Next allocation with 8-byte alignment must be properly aligned
        let layout2 = Layout::from_size_align(8, 8).unwrap();
        let ptr = unsafe { alloc.alloc(layout2) };
        assert!(!ptr.is_null());
        assert_eq!(ptr as usize % 8, 0);
    }

    #[test]
    fn alloc_fills_exactly() {
        let alloc = BumpAllocator::<64>::new();
        let layout = Layout::from_size_align(64, 1).unwrap();
        assert!(!unsafe { alloc.alloc(layout) }.is_null());

        // Heap is full — next alloc must fail
        assert!(unsafe { alloc.alloc(Layout::from_size_align(1, 1).unwrap()) }.is_null());
    }

    #[test]
    fn alloc_oom_returns_null() {
        let alloc = BumpAllocator::<16>::new();
        let layout = Layout::from_size_align(17, 1).unwrap();
        assert!(unsafe { alloc.alloc(layout) }.is_null());
    }

    #[test]
    fn alloc_oom_due_to_alignment_padding() {
        // 9 bytes of heap: alloc 1 byte, then try 8 bytes with align 8
        // offset=1, aligned=8, 8+8=16 > 9 → OOM
        let alloc = BumpAllocator::<9>::new();
        unsafe { alloc.alloc(Layout::from_size_align(1, 1).unwrap()) };
        assert!(unsafe { alloc.alloc(Layout::from_size_align(8, 8).unwrap()) }.is_null());
    }

    #[test]
    fn multiple_allocations_dont_overlap() {
        let alloc = BumpAllocator::<1024>::new();
        let layout = Layout::from_size_align(32, 8).unwrap();

        let p1 = unsafe { alloc.alloc(layout) };
        let p2 = unsafe { alloc.alloc(layout) };
        let p3 = unsafe { alloc.alloc(layout) };
        assert!(!p1.is_null() && !p2.is_null() && !p3.is_null());

        // Bump allocator returns sequential non-overlapping regions
        assert!(p1 as usize + 32 <= p2 as usize);
        assert!(p2 as usize + 32 <= p3 as usize);
    }

    #[test]
    fn write_and_read_back() {
        let alloc = BumpAllocator::<256>::new();
        let ptr = unsafe { alloc.alloc(Layout::from_size_align(4, 4).unwrap()) } as *mut u32;
        assert!(!ptr.is_null());
        unsafe {
            ptr.write(0xDEAD_BEEF);
            assert_eq!(ptr.read(), 0xDEAD_BEEF);
        }
    }

    #[test]
    fn no_out_of_bounds_writes() {
        #[repr(C)]
        struct Guarded {
            alloc: BumpAllocator<64>,
            sentinel: [u8; 64],
        }

        let guarded = Guarded {
            alloc: BumpAllocator::new(),
            sentinel: [0xAA; 64],
        };

        let ptr = unsafe { guarded.alloc.alloc(Layout::from_size_align(64, 1).unwrap()) };
        assert!(!ptr.is_null());

        for i in 0..64 {
            unsafe { ptr.add(i).write(0xFF) };
        }

        for byte in &guarded.sentinel {
            assert_eq!(*byte, 0xAA, "out-of-bounds write detected");
        }
    }
}
