// SPDX-License-Identifier: AGPL-3.0-or-later
// An object's elements (value.rs Elems.v) as a Vec<Val> that can drop its
// first element in O(1): Array.prototype.shift moves the vector's start
// instead of every element, and unshift puts a value into the room shifts
// left, or into room it makes before the values. Rust only (lib.rs
// includes it for the rust target); the other targets keep their arrays.
//
// `inner` is a Vec whose pointer may lie `head` values past the start of
// its allocation (and whose capacity is the allocation's less `head`).
// Such a Vec must never reallocate or free: `&inner` (Deref) is handed
// out as it is, since reading cannot do either, and the methods below
// only use Vec operations that neither grow nor shrink the buffer. Every
// `&mut` that could (DerefMut, Drop) first moves the values back to the
// allocation's start (`flat`), making `inner` an ordinary Vec again.
// Val is Copy: the values shifted out own nothing to drop.

use crate::value::Val;
use std::ops::{Deref, DerefMut};

#[repr(C)]
pub struct EVec {
    /// first: the JIT reads its pointer and length (wjit.rs layout)
    pub inner: Vec<Val>,
    /// values dropped from the allocation's start
    head: usize,
}

impl EVec {
    #[inline]
    pub fn new() -> EVec {
        EVec { inner: Vec::new(), head: 0 }
    }

    #[inline]
    pub fn from_vec(v: Vec<Val>) -> EVec {
        EVec { inner: v, head: 0 }
    }

    /// The values back at the allocation's start: `inner` an ordinary Vec.
    #[inline]
    fn flat(&mut self) {
        if self.head > 0 {
            self.flat_slow();
        }
    }

    #[inline(never)]
    fn flat_slow(&mut self) {
        let h = self.head;
        let len = self.inner.len();
        let cap = self.inner.capacity() + h;
        let p = self.inner.as_mut_ptr();
        let v = std::mem::replace(&mut self.inner, Vec::new());
        std::mem::forget(v);
        // SAFETY: p - h is the allocation's start (shift moved p by h
        // values within it); the len values at p move down into it, cap is
        // the allocation's capacity in values.
        unsafe {
            let base = p.sub(h);
            std::ptr::copy(p, base, len);
            self.inner = Vec::from_raw_parts(base, len, cap);
        }
        self.head = 0;
    }

    #[inline(always)]
    pub fn push(&mut self, x: Val) {
        let n = self.inner.len();
        if n < self.inner.capacity() {
            // SAFETY: slot n is inside the capacity; one compare, as
            // Vec::push's own
            unsafe {
                std::ptr::write(self.inner.as_mut_ptr().add(n), x);
                self.inner.set_len(n + 1);
            }
        } else {
            self.push_grow(x);
        }
    }

    /// push past the capacity: growing reallocates, so an ordinary Vec
    /// first (the room shifts left then serves this push).
    #[inline(never)]
    fn push_grow(&mut self, x: Val) {
        self.flat();
        self.inner.push(x);
    }

    /// `self[i] = x` (in place: no flat).
    #[inline(always)]
    pub fn put(&mut self, i: usize, x: Val) {
        self.inner[i] = x;
    }

    /// `self[i] = x` for an `i` the caller knows is in range.
    #[inline(always)]
    pub fn put_unchecked(&mut self, i: usize, x: Val) {
        debug_assert!(i < self.inner.len(), "element index past its vector");
        // SAFETY: the caller's bounds check; Val is Copy (no drop)
        unsafe {
            *self.inner.get_unchecked_mut(i) = x;
        }
    }

    #[inline]
    pub fn pop(&mut self) -> Option<Val> {
        self.inner.pop()
    }

    #[inline]
    pub fn truncate(&mut self, n: usize) {
        self.inner.truncate(n);
    }

    #[inline]
    pub fn clear(&mut self) {
        self.inner.clear();
    }

    /// Takes out value `i`; the first in O(1) (shift).
    pub fn remove(&mut self, i: usize) -> Val {
        let len = self.inner.len();
        if i != 0 || len < 16 {
            // within the buffer: no allocation change
            return self.inner.remove(i);
        }
        let x = self.inner[0];
        let cap = self.inner.capacity();
        let p = self.inner.as_mut_ptr();
        let v = std::mem::replace(&mut self.inner, Vec::new());
        std::mem::forget(v);
        // SAFETY: p + 1 stays inside the allocation (len >= 16 values are
        // there); the Vec keeps the len - 1 values after the first and the
        // cap - 1 slots from p + 1 on.
        unsafe {
            self.inner = Vec::from_raw_parts(p.add(1), len - 1, cap - 1);
        }
        self.head += 1;
        // the dropped room back once it is twice what is left: each value
        // moves at most once per value shifted out, and shifts after an
        // unshift (front_room leaves head = len) do not move them at once
        if self.head >= 64 && self.head > 2 * self.inner.len() {
            self.flat_slow();
        }
        x
    }

    /// Puts `x` at `i`; at 0 into the room shifts left when there is, and
    /// with none (a vector of 16 or more) first into a new buffer with as
    /// much room before the values as there are values: unshift in O(1)
    /// amortised, as shift is.
    pub fn insert(&mut self, i: usize, x: Val) {
        if i == 0 && self.head == 0 && self.inner.len() >= 16 {
            self.front_room();
        }
        if i == 0 && self.head > 0 {
            let len = self.inner.len();
            let cap = self.inner.capacity();
            let p = self.inner.as_mut_ptr();
            let v = std::mem::replace(&mut self.inner, Vec::new());
            std::mem::forget(v);
            // SAFETY: head > 0 slots lie before p in the allocation; p - 1
            // takes x and the Vec grows by that slot.
            unsafe {
                let q = p.sub(1);
                std::ptr::write(q, x);
                self.inner = Vec::from_raw_parts(q, len + 1, cap + 1);
            }
            self.head -= 1;
            return;
        }
        if self.inner.len() == self.inner.capacity() {
            self.flat();
        }
        self.inner.insert(i, x);
    }

    /// Moves the values into a new buffer `len` values past its start
    /// (and with len / 2 slots after them): `head` = len.
    #[inline(never)]
    fn front_room(&mut self) {
        let len = self.inner.len();
        let room = len;
        let mut nv: Vec<Val> = Vec::with_capacity(room + len + len / 2);
        let cap = nv.capacity();
        let p = nv.as_mut_ptr();
        std::mem::forget(nv);
        let old = std::mem::replace(&mut self.inner, Vec::new());
        // SAFETY: the new buffer holds cap >= room + len values; the len
        // values go to p + room; `inner` then starts there with the cap -
        // room slots from it, as remove leaves it. The old buffer is an
        // ordinary Vec (head 0) and frees as one; Val owns nothing.
        unsafe {
            std::ptr::copy_nonoverlapping(old.as_ptr(), p.add(room), len);
            self.inner = Vec::from_raw_parts(p.add(room), len, cap - room);
        }
        drop(old);
        self.head = room;
    }

    /// The values as an ordinary Vec, taken out (this one left empty).
    pub fn take(&mut self) -> Vec<Val> {
        self.flat();
        std::mem::replace(&mut self.inner, Vec::new())
    }

    /// Replaces the values with `v`.
    pub fn set(&mut self, v: Vec<Val>) {
        self.flat();
        self.inner = v;
    }
}

impl Deref for EVec {
    type Target = Vec<Val>;
    #[inline]
    fn deref(&self) -> &Vec<Val> {
        &self.inner
    }
}

impl DerefMut for EVec {
    #[inline]
    fn deref_mut(&mut self) -> &mut Vec<Val> {
        self.flat();
        &mut self.inner
    }
}

impl Drop for EVec {
    fn drop(&mut self) {
        self.flat();
    }
}
