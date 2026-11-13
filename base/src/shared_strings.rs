//! Where each text is among the workbook's shared strings.
//!
//! A cell that shows a text holds its index in `Workbook::shared_strings`,
//! and typing a text needs the index of a text already there. A map from
//! the text to the index held every text a second time; a sheet of data
//! with a hundred thousand names in it held them all twice. This index
//! holds a hash of each text and reads the text itself from the list to
//! tell apart the rare two of the same hash.

use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::hash::{BuildHasher, Hasher, RandomState};

/// A hasher for keys that are hashes already.
#[derive(Default, Clone, Copy)]
struct HashOfHash(u64);

impl Hasher for HashOfHash {
    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 = self.0.rotate_left(8) ^ *byte as u64;
        }
    }

    fn write_u64(&mut self, i: u64) {
        self.0 = i;
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

type HashOfHashBuild = std::hash::BuildHasherDefault<HashOfHash>;

/// Where each text is among a list of texts, by its hash: the first text of
/// each hash in one map, sixteen bytes a slot, and the rare others of the
/// same hash in a second. The hash is keyed at random, as the standard
/// map's is, so that texts chosen to collide do not slow it down.
#[derive(Debug, Clone, Default)]
pub(crate) struct SharedStringIndex {
    keys: RandomState,
    first: HashMap<u64, u32, HashOfHashBuild>,
    others: HashMap<u64, Vec<u32>, HashOfHashBuild>,
}

impl SharedStringIndex {
    /// The index of every text of `strings`. Of two equal texts the first
    /// is found.
    pub(crate) fn of(strings: &[String]) -> SharedStringIndex {
        let mut index = SharedStringIndex::default();
        index.first.reserve(strings.len());
        for (position, text) in strings.iter().enumerate() {
            if index.get(strings, text).is_none() {
                index.insert(text, position);
            }
        }
        index
    }

    fn hash(&self, text: &str) -> u64 {
        self.keys.hash_one(text)
    }

    /// Where `text` is in `strings`, the list this index was made for.
    pub(crate) fn get(&self, strings: &[String], text: &str) -> Option<usize> {
        let is_it = |position: &u32| {
            strings
                .get(*position as usize)
                .is_some_and(|candidate| candidate == text)
        };
        let hash = self.hash(text);
        let first = self.first.get(&hash)?;
        if is_it(first) {
            return Some(*first as usize);
        }
        self.others
            .get(&hash)?
            .iter()
            .find(|p| is_it(p))
            .map(|p| *p as usize)
    }

    /// Says that `text` is at `position`.
    pub(crate) fn insert(&mut self, text: &str, position: usize) {
        let position = position as u32;
        let hash = self.hash(text);
        match self.first.entry(hash) {
            Entry::Occupied(_) => self.others.entry(hash).or_default().push(position),
            Entry::Vacant(slot) => {
                slot.insert(position);
            }
        }
    }

    /// The memory the index takes, in bytes: its slots and a byte of control
    /// for each.
    pub(crate) fn heap_size(&self) -> usize {
        self.first.capacity() * (std::mem::size_of::<(u64, u32)>() + 1)
            + self.others.capacity() * (std::mem::size_of::<(u64, Vec<u32>)>() + 1)
            + self
                .others
                .values()
                .map(|positions| positions.capacity() * 4)
                .sum::<usize>()
    }
}
