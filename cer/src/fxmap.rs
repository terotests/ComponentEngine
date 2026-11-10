// SPDX-License-Identifier: AGPL-3.0-or-later
// A HashMap with a multiplicative hash (rustc-hash 2's FxHasher) for the
// maps whose keys are machine-made: a Map's or Set's index (value.rs). Rust
// only (lib.rs includes it for the rust target); the other targets keep
// their maps.

use std::hash::{BuildHasherDefault, Hasher};
use std::ops::{Deref, DerefMut};

#[derive(Default, Clone, Copy)]
pub struct FxHasher {
    hash: u64,
}

const K: u64 = 0xf1357aea2e62a9c5;

impl FxHasher {
    #[inline]
    fn add(&mut self, w: u64) {
        self.hash = self.hash.wrapping_add(w).wrapping_mul(K);
    }
}

impl Hasher for FxHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let mut b = bytes;
        while b.len() >= 8 {
            self.add(u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]));
            b = &b[8..];
        }
        if b.len() >= 4 {
            self.add(u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as u64);
            b = &b[4..];
        }
        for &x in b {
            self.add(x as u64);
        }
    }
    #[inline]
    fn write_u8(&mut self, i: u8) {
        self.add(i as u64);
    }
    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.add(i as u64);
    }
    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.add(i);
    }
    #[inline]
    fn write_i64(&mut self, i: i64) {
        self.add(i as u64);
    }
    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.add(i as u64);
    }
    #[inline]
    fn finish(&self) -> u64 {
        // the product's high bits are its well mixed ones: brought down to
        // where the table takes its bucket
        self.hash.rotate_left(26)
    }
}

pub type FxBuild = BuildHasherDefault<FxHasher>;

/// std's HashMap with FxHasher, made with `new()` as std's is.
#[derive(Clone, Default)]
pub struct HashMap<K, V>(std::collections::HashMap<K, V, FxBuild>);

impl<K, V> HashMap<K, V> {
    #[inline]
    pub fn new() -> HashMap<K, V> {
        HashMap(std::collections::HashMap::with_hasher(FxBuild::default()))
    }
    #[inline]
    pub fn with_capacity(n: usize) -> HashMap<K, V> {
        HashMap(std::collections::HashMap::with_capacity_and_hasher(n, FxBuild::default()))
    }
}

impl<K, V> Deref for HashMap<K, V> {
    type Target = std::collections::HashMap<K, V, FxBuild>;
    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<K, V> DerefMut for HashMap<K, V> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<'a, K, V> IntoIterator for &'a HashMap<K, V> {
    type Item = (&'a K, &'a V);
    type IntoIter = std::collections::hash_map::Iter<'a, K, V>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}
