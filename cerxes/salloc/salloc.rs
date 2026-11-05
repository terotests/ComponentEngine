//! A small-block allocator for a program that runs CErXes: blocks of up to
//! 256 bytes come from per-thread free lists by size class, the rest (and a
//! list that is empty) from the system allocator.
//!
//! The engine allocates and frees many short-lived small blocks (strings,
//! property and element vectors, argument lists). With the system
//! allocator each of those costs about a hundred instructions both ways;
//! from a free list it is a handful.
//!
//! A freed small block goes on its thread's list for its class, up to
//! KEEP_BYTES per class; past that it goes back to the system. So what is
//! kept is bounded and reused block for block: memory plateaus as it does
//! with the system allocator. A block freed on another thread than the one
//! that made it joins that thread's list (both lists hold blocks the system
//! allocator made, of the same class size).
//!
//! A program opts in with
//!
//! ```ignore
//! #[path = ".../salloc/salloc.rs"] mod salloc;
//! #[global_allocator]
//! static GLOBAL: salloc::SmallAlloc = salloc::SmallAlloc;
//! ```

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::ptr;

/// Class sizes in bytes; a request takes the smallest that fits.
const CLASS: [usize; 8] = [16, 32, 48, 64, 96, 128, 192, 256];
const NCLASS: usize = 8;
/// The largest block the lists hold.
const SMALL: usize = 256;
/// What one class's list may hold before frees go back to the system.
const KEEP_BYTES: usize = 1 << 19;

/// Class of a size of 1..=256 bytes: an index into CLASS.
#[inline(always)]
fn class_of(size: usize) -> usize {
    // 16-byte steps up to 64, then 96, 128, 192, 256
    if size <= 64 {
        (size.max(1) - 1) >> 4
    } else if size <= 96 {
        4
    } else if size <= 128 {
        5
    } else if size <= 192 {
        6
    } else {
        7
    }
}

struct Lists {
    head: [Cell<*mut u8>; NCLASS],
    count: [Cell<usize>; NCLASS],
}

thread_local! {
    // const-initialised and without Drop: no lazy init and no destructor
    // to register, so the allocator never allocates to reach its lists
    static LISTS: Lists = const {
        Lists {
            head: [const { Cell::new(ptr::null_mut()) }; NCLASS],
            count: [const { Cell::new(0) }; NCLASS],
        }
    };
}

pub struct SmallAlloc;

#[inline(always)]
fn small(l: &Layout) -> bool {
    l.size() <= SMALL && l.align() <= 16
}

unsafe impl GlobalAlloc for SmallAlloc {
    #[inline]
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        if !small(&l) {
            return System.alloc(l);
        }
        let c = class_of(l.size());
        let p = LISTS
            .try_with(|s| {
                let p = s.head[c].get();
                if !p.is_null() {
                    // SAFETY: a listed block holds the next block's address
                    s.head[c].set(*(p as *mut *mut u8));
                    s.count[c].set(s.count[c].get() - 1);
                }
                p
            })
            .unwrap_or(ptr::null_mut());
        if !p.is_null() {
            return p;
        }
        // SAFETY: the class size is a multiple of 16 and at least the request
        System.alloc(Layout::from_size_align_unchecked(CLASS[c], 16))
    }

    #[inline]
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        if !small(&l) {
            System.dealloc(p, l);
            return;
        }
        let c = class_of(l.size());
        let kept = LISTS
            .try_with(|s| {
                let n = s.count[c].get();
                if (n + 1) * CLASS[c] > KEEP_BYTES {
                    return false;
                }
                // SAFETY: the block is at least 16 bytes and 16-aligned
                *(p as *mut *mut u8) = s.head[c].get();
                s.head[c].set(p);
                s.count[c].set(n + 1);
                true
            })
            .unwrap_or(false);
        if !kept {
            System.dealloc(p, Layout::from_size_align_unchecked(CLASS[c], 16));
        }
    }

    #[inline]
    unsafe fn realloc(&self, p: *mut u8, l: Layout, new_size: usize) -> *mut u8 {
        let was_small = small(&l);
        let now_small = new_size <= SMALL && l.align() <= 16;
        if was_small && now_small && class_of(l.size()) == class_of(new_size) {
            // the block already has room
            return p;
        }
        if !was_small && !now_small {
            return System.realloc(p, l, new_size);
        }
        let nl = Layout::from_size_align_unchecked(new_size, l.align());
        let q = self.alloc(nl);
        if !q.is_null() {
            ptr::copy_nonoverlapping(p, q, l.size().min(new_size));
            self.dealloc(p, l);
        }
        q
    }
}
