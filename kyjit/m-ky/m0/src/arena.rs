//! Simple bump allocator for M0.
//! One buffer per thread. Bump pointer. Reset at batch boundaries.
//! All allocations are zero-cost (pointer increment). No free.
//! Suitable for short-lived batch processing where all data is discarded together.

use std::cell::UnsafeCell;

pub struct Arena {
    buf: UnsafeCell<Vec<u8>>,
    offset: UnsafeCell<usize>,
}

impl Arena {
    pub fn with_capacity(capacity: usize) -> Self {
        Arena {
            buf: UnsafeCell::new(vec![0u8; capacity]),
            offset: UnsafeCell::new(0),
        }
    }

    /// Reset the arena. All previous allocations become invalid.
    /// Call this between batches.
    pub fn reset(&self) {
        unsafe { *self.offset.get() = 0; }
    }

    /// Allocate space for a single value and copy it in.
    /// Returns a reference with lifetime tied to &self.
    pub fn alloc<T: Copy>(&self, value: T) -> &mut T {
        let size = std::mem::size_of::<T>();
        let align = std::mem::align_of::<T>().max(1);
        let offset = unsafe { *self.offset.get() };
        let aligned = (offset + align - 1) & !(align - 1);
        let new_offset = aligned + size;

        let buf = unsafe { &mut *self.buf.get() };
        if new_offset > buf.len() {
            panic!("arena overflow: need {}, have {}", new_offset, buf.len());
        }
        unsafe { *self.offset.get() = new_offset; }

        let ptr = unsafe { buf.as_mut_ptr().add(aligned) as *mut T };
        unsafe { std::ptr::write(ptr, value); }
        unsafe { &mut *ptr }
    }

    /// Allocate space for a slice and copy it in.
    pub fn alloc_slice<T: Copy>(&self, values: &[T]) -> &mut [T] {
        if values.is_empty() {
            return &mut [];
        }
        let size = std::mem::size_of::<T>() * values.len();
        let align = std::mem::align_of::<T>().max(1);
        let offset = unsafe { *self.offset.get() };
        let aligned = (offset + align - 1) & !(align - 1);
        let new_offset = aligned + size;

        let buf = unsafe { &mut *self.buf.get() };
        if new_offset > buf.len() {
            panic!("arena overflow: need {}, have {}", new_offset, buf.len());
        }
        unsafe { *self.offset.get() = new_offset; }

        let ptr = unsafe { buf.as_mut_ptr().add(aligned) as *mut T };
        unsafe { std::ptr::copy_nonoverlapping(values.as_ptr(), ptr, values.len()); }
        unsafe { std::slice::from_raw_parts_mut(ptr, values.len()) }
    }

    /// Allocate an empty Vec-like slice of capacity `n`.
    pub fn alloc_uninit_slice<T: Copy>(&self, n: usize) -> &mut [T] {
        let size = std::mem::size_of::<T>() * n;
        let align = std::mem::align_of::<T>().max(1);
        let offset = unsafe { *self.offset.get() };
        let aligned = (offset + align - 1) & !(align - 1);
        let new_offset = aligned + size;

        let buf = unsafe { &mut *self.buf.get() };
        if new_offset > buf.len() {
            panic!("arena overflow: need {}, have {}", new_offset, buf.len());
        }
        unsafe { *self.offset.get() = new_offset; }

        let ptr = unsafe { buf.as_mut_ptr().add(aligned) as *mut T };
        unsafe { std::slice::from_raw_parts_mut(ptr, n) }
    }

    /// Current used bytes.
    pub fn used(&self) -> usize {
        unsafe { *self.offset.get() }
    }

    /// Total capacity in bytes.
    pub fn capacity(&self) -> usize {
        unsafe { (*self.buf.get()).len() }
    }
}

// Safety: Arena is used per-thread. Each thread owns its own Arena.
// The UnsafeCell is fine as long as there's no cross-thread sharing.
unsafe impl Send for Arena {}
