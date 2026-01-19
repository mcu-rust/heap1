#![doc = include_str!("../README.md")]
#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(feature = "std"))]
extern crate alloc;

#[cfg(not(feature = "std"))]
use alloc::{boxed::Box, vec::Vec};
use core::{
    alloc::{GlobalAlloc, Layout},
    cell::UnsafeCell,
    mem::MaybeUninit,
    ptr::{self, NonNull},
};
use portable_atomic::{AtomicUsize, Ordering};

/// The simplest possible heap.
pub struct Heap<S: Storage> {
    storage: UnsafeCell<S>,
    remained: AtomicUsize,
}

unsafe impl<S: Storage> Sync for Heap<S> {}

impl<S: Storage> Heap<S> {
    /// Create a new heap allocator
    pub const fn new_with_storage(storage: S, size: usize) -> Self {
        Self {
            storage: UnsafeCell::new(storage),
            remained: AtomicUsize::new(size),
        }
    }

    /// Returns the amount of unused bytes.
    pub fn remained(&self) -> usize {
        self.remained.load(Ordering::Relaxed)
    }
}

#[allow(clippy::new_without_default)]
impl<S: ConstStorage> Heap<S> {
    /// Create a new heap allocator
    pub const fn new() -> Self {
        Self::new_with_storage(S::INIT, S::SIZE)
    }
}

impl Heap<BoxedSlice> {
    /// Create a new heap allocator from global heap.
    pub fn new_boxed(size: usize) -> Self {
        Self::new_with_storage(BoxedSlice::new(size), size)
    }
}

impl Heap<Pointer> {
    /// Create an empty heap allocator
    pub const fn empty() -> Self {
        Self::new_with_storage(Pointer::empty(), 0)
    }

    /// # Safety
    ///
    /// This function is safe if the following invariants hold:
    ///
    /// - `start_addr` points to valid memory.
    /// - `size` is correct.
    /// - Call it only once.
    pub unsafe fn init_with_ptr(&self, address: usize, size: usize) {
        let s = unsafe { &mut *self.storage.get() };
        s.ptr = unsafe { NonNull::new_unchecked(address as *mut u8) };
        self.remained.store(size, Ordering::Release);
    }
}

unsafe impl<S: Storage> GlobalAlloc for Heap<S> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // `Layout` contract forbids making a `Layout` with align=0, or align not power of 2.
        // So we can safely use a mask to ensure alignment without worrying about UB.
        let align_mask_to_round_down = !(layout.align() - 1);

        let mut old_remained = self.remained.load(Ordering::Relaxed);
        loop {
            if layout.size() > old_remained {
                return ptr::null_mut();
            }

            let remained = (old_remained - layout.size()) & align_mask_to_round_down;
            match self.remained.compare_exchange_weak(
                old_remained,
                remained,
                Ordering::SeqCst,
                Ordering::Relaxed,
            ) {
                Err(x) => old_remained = x,
                Ok(_) => {
                    return unsafe { ((&mut *self.storage.get()).ptr().as_ptr()).add(remained) };
                }
            }
        }
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}

#[cfg(feature = "allocator-api")]
mod allocator_api {
    use super::*;
    use core::alloc::{AllocError, Allocator};

    unsafe impl Allocator for Heap {
        fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
            match layout.size() {
                0 => Ok(NonNull::slice_from_raw_parts(layout.dangling(), 0)),
                size => self.alloc(layout).map_or(Err(AllocError), |allocation| {
                    Ok(NonNull::slice_from_raw_parts(allocation, size))
                }),
            }
        }

        unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
            if layout.size() != 0 {
                self.dealloc(ptr.as_ptr(), layout);
            }
        }
    }
}

// ------------------------------------------------------------------

/// Trait for providing access to the storage
pub trait Storage {
    /// Return a pointer of the underlying storage.
    ///
    /// ## Safety
    ///
    /// Implementations of this function MUST always return the same pointer
    /// for all calls.
    unsafe fn ptr(&mut self) -> NonNull<u8>;
}

pub trait ConstStorage: Storage {
    /// The default value of this type
    const INIT: Self;
    /// The size of storage
    const SIZE: usize;
}

// ------------------------------------------------------------------

pub struct Inline<const SIZE: usize> {
    buf: [MaybeUninit<u8>; SIZE],
}

#[allow(clippy::new_without_default)]
impl<const SIZE: usize> Inline<SIZE> {
    pub const fn new() -> Self {
        Self {
            buf: [MaybeUninit::uninit(); SIZE],
        }
    }
}

impl<const SIZE: usize> ConstStorage for Inline<SIZE> {
    const INIT: Self = Self::new();
    const SIZE: usize = SIZE;
}

impl<const SIZE: usize> Storage for Inline<SIZE> {
    #[inline]
    unsafe fn ptr(&mut self) -> NonNull<u8> {
        unsafe { NonNull::new_unchecked(self.buf.as_mut_ptr().cast()) }
    }
}

// ------------------------------------------------------------------

pub struct Pointer {
    ptr: NonNull<u8>,
}

impl Pointer {
    const fn empty() -> Self {
        Self {
            ptr: NonNull::dangling(),
        }
    }
}

impl Storage for Pointer {
    #[inline]
    unsafe fn ptr(&mut self) -> NonNull<u8> {
        self.ptr
    }
}

// ------------------------------------------------------------------

pub struct BoxedSlice {
    buf: Box<[MaybeUninit<u8>]>,
}

impl BoxedSlice {
    /// Create a new BoxedSlice with capacity `len`.
    fn new(size: usize) -> Self {
        let mut v = Vec::with_capacity(size);
        unsafe { v.set_len(size) }
        Self {
            buf: v.into_boxed_slice(),
        }
    }
}

impl Storage for BoxedSlice {
    #[inline]
    unsafe fn ptr(&mut self) -> NonNull<u8> {
        unsafe { NonNull::new_unchecked(self.buf.as_mut_ptr().cast()) }
    }
}

// ------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    static HEAP: Heap<Inline<100>> = Heap::new();
    static HEAP_P: Heap<Pointer> = Heap::empty();

    #[test]
    fn test_heap() {
        assert_eq!(HEAP.remained.load(Ordering::Relaxed), 100);
        let p0 = unsafe { (&mut *HEAP.storage.get()).buf.as_ptr() as usize };
        let p1 = unsafe { (&mut *HEAP.storage.get()).ptr() }.as_ptr();
        assert_eq!(p0, p1 as usize);
        let p2 = unsafe { HEAP.alloc(Layout::new::<u64>()) };
        assert_eq!(HEAP.remained.load(Ordering::Relaxed), 88);
        assert_eq!(unsafe { p2.offset_from(p1) }, 88);

        unsafe { HEAP.alloc(Layout::new::<u32>()) };
        assert_eq!(HEAP.remained.load(Ordering::Relaxed), 84);
    }

    #[test]
    fn test_heap_pointer() {
        const HEAP_SIZE: usize = 100;
        static mut HEAP_MEM: [MaybeUninit<u8>; HEAP_SIZE] = [MaybeUninit::uninit(); HEAP_SIZE];
        unsafe { HEAP_P.init_with_ptr(&raw mut HEAP_MEM as usize, HEAP_SIZE) }
        let p0 = &raw mut HEAP_MEM as usize;
        let p1 = unsafe { (&mut *HEAP_P.storage.get()).ptr() }.as_ptr();
        assert_eq!(p0, p1 as usize);
        let p2 = unsafe { HEAP_P.alloc(Layout::new::<u64>()) };
        assert_eq!(HEAP_P.remained.load(Ordering::Relaxed), 88);
        assert_eq!(unsafe { p2.offset_from(p1) }, 88);
    }

    #[test]
    fn test_heap_dynamic() {
        let heap = Heap::new_boxed(100);
        assert_eq!(heap.remained.load(Ordering::Relaxed), 100);
        let p0 = unsafe { (&mut *heap.storage.get()).buf.as_ptr() as usize };
        let p1 = unsafe { (&mut *heap.storage.get()).ptr() }.as_ptr();
        assert_eq!(p0, p1 as usize);
        let p2 = unsafe { heap.alloc(Layout::new::<u64>()) };
        assert_eq!(heap.remained.load(Ordering::Relaxed), 88);
        assert_eq!(unsafe { p2.offset_from(p1) }, 88);

        unsafe { heap.alloc(Layout::new::<u32>()) };
        assert_eq!(heap.remained.load(Ordering::Relaxed), 84);
    }

    #[test]
    fn test_heap_local() {
        let _heap: Heap<Inline<100>> = Heap::new();
    }
}
