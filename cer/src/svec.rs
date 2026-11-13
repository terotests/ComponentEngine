// SPDX-License-Identifier: AGPL-3.0-or-later
// An object's property values from slot 2 on (value.rs Slots.v) as a thin
// vector: one pointer in the object instead of a Vec's three words. The
// length and capacity are in a header just before the values, so the
// pointer points at value 0 as a Vec's does (the JIT reads it so,
// wjit.rs) and the header is at pointer - 8 (len, then cap, as u32).
// An empty vector points past a shared static header of zeros: it never
// writes there, since a push into capacity 0 allocates first. Rust only
// (lib.rs includes it for the rust target); the other targets keep a Vec.
// Val is Copy: the values own nothing to drop.

use crate::value::Val;
use std::alloc::{alloc, dealloc, realloc, Layout};
use std::ops::{Index, IndexMut};
use std::ptr::NonNull;

#[repr(C, align(8))]
struct Empty([u32; 2]);

static EMPTY: Empty = Empty([0, 0]);

const HDR: usize = 8;

#[repr(C)]
pub struct SVec {
    /// value 0; the header (len: u32, cap: u32) at p - 8
    p: NonNull<Val>,
}

// SAFETY: an SVec owns its buffer alone, as a Vec does (EMPTY is never
// written).
unsafe impl Send for SVec {}
unsafe impl Sync for SVec {}

#[inline(always)]
fn layout(cap: usize) -> Layout {
    // the header keeps the values 8-aligned (Val's alignment is at most 8)
    Layout::from_size_align(HDR + cap * std::mem::size_of::<Val>(), 8).unwrap()
}

impl SVec {
    #[inline(always)]
    pub fn new() -> SVec {
        // SAFETY: one past EMPTY's 8 bytes, never written or read as values
        let p = unsafe { (&EMPTY as *const Empty as *mut u8).add(HDR) as *mut Val };
        SVec { p: unsafe { NonNull::new_unchecked(p) } }
    }

    #[inline(always)]
    fn hdr(&self) -> *mut u32 {
        // SAFETY: the header is the 8 bytes before p (EMPTY's, or the
        // allocation's start)
        unsafe { (self.p.as_ptr() as *mut u8).sub(HDR) as *mut u32 }
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        unsafe { *self.hdr() as usize }
    }

    #[inline(always)]
    pub fn capacity(&self) -> usize {
        unsafe { *self.hdr().add(1) as usize }
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline(always)]
    fn set_len(&mut self, n: usize) {
        // only called with capacity > 0: the header is the allocation's
        unsafe {
            *self.hdr() = n as u32;
        }
    }

    /// The buffer resized to `cap` values (>= len); 0 frees it.
    fn resize(&mut self, cap: usize) {
        let old = self.capacity();
        if cap == old {
            return;
        }
        let n = self.len();
        debug_assert!(cap >= n);
        // SAFETY: the allocation (when old > 0) is layout(old) at p - 8;
        // the header is written before p moves to the new one
        unsafe {
            if cap == 0 {
                dealloc(self.hdr() as *mut u8, layout(old));
                // not `*self = SVec::new()`: that would drop the freed one
                self.p = SVec::new().p;
                return;
            }
            let base = if old == 0 { alloc(layout(cap)) } else { realloc(self.hdr() as *mut u8, layout(old), layout(cap).size()) };
            if base.is_null() {
                std::alloc::handle_alloc_error(layout(cap));
            }
            let h = base as *mut u32;
            *h = n as u32;
            *h.add(1) = cap as u32;
            self.p = NonNull::new_unchecked(base.add(HDR) as *mut Val);
        }
    }

    #[inline(always)]
    pub fn push(&mut self, x: Val) {
        let n = self.len();
        if n == self.capacity() {
            self.grow_one();
        }
        // SAFETY: slot n is inside the capacity
        unsafe {
            std::ptr::write(self.p.as_ptr().add(n), x);
        }
        self.set_len(n + 1);
    }

    #[inline(never)]
    fn grow_one(&mut self) {
        let c = self.capacity();
        self.resize(if c == 0 { 4 } else { 2 * c });
    }

    /// Room for `add` more values past the length, exactly.
    pub fn reserve_exact(&mut self, add: usize) {
        let want = self.len() + add;
        if want > self.capacity() {
            self.resize(want);
        }
    }

    pub fn truncate(&mut self, n: usize) {
        if n < self.len() {
            self.set_len(n);
        }
    }

    pub fn shrink_to_fit(&mut self) {
        let n = self.len();
        self.resize(n);
    }

    /// Takes value `i` out, the later ones moving down one.
    pub fn remove(&mut self, i: usize) -> Val {
        let n = self.len();
        assert!(i < n, "slot index past its vector");
        // SAFETY: i < n; the n - i - 1 values after it move down one
        unsafe {
            let at = self.p.as_ptr().add(i);
            let v = *at;
            std::ptr::copy(at.add(1), at, n - i - 1);
            self.set_len(n - 1);
            v
        }
    }

    /// Value `i` for an `i` the caller has checked.
    #[inline(always)]
    pub fn get_unchecked(&self, i: usize) -> &Val {
        debug_assert!(i < self.len(), "slot index past its vector");
        unsafe { &*self.p.as_ptr().add(i) }
    }

    /// Value `i`, writable, for an `i` the caller has checked.
    #[inline(always)]
    pub fn get_unchecked_mut(&mut self, i: usize) -> &mut Val {
        debug_assert!(i < self.len(), "slot index past its vector");
        unsafe { &mut *self.p.as_ptr().add(i) }
    }

    /// The buffer goes (an empty vector again).
    pub fn free(&mut self) {
        self.resize_free();
    }

    #[inline]
    fn resize_free(&mut self) {
        if self.capacity() > 0 {
            self.set_len(0);
            self.resize(0);
        }
    }
}

impl Drop for SVec {
    fn drop(&mut self) {
        self.resize_free();
    }
}

impl Index<usize> for SVec {
    type Output = Val;
    #[inline(always)]
    fn index(&self, i: usize) -> &Val {
        assert!(i < self.len(), "slot index past its vector");
        unsafe { &*self.p.as_ptr().add(i) }
    }
}

impl IndexMut<usize> for SVec {
    #[inline(always)]
    fn index_mut(&mut self, i: usize) -> &mut Val {
        assert!(i < self.len(), "slot index past its vector");
        unsafe { &mut *self.p.as_ptr().add(i) }
    }
}
