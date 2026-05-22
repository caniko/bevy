//! Per-world mutation logs for restricted component write pipelines.

use alloc::{vec, vec::Vec};
use core::marker::PhantomData;

use crate::{resource::Resource, world::World};

/// A marker trait for the component set recorded by a [`MutationLog`].
///
/// The initial prototype uses a manual implementation rather than a derive
/// macro. A future upstream-ready version can derive this from a marker struct
/// that lists participating restricted components.
pub trait MutationLogSet: Send + Sync + 'static {
    /// Stable domain tag mixed into the genesis hash.
    const DOMAIN_TAG: &'static [u8];

    /// Stable domain version mixed into the genesis hash.
    const DOMAIN_VERSION: u32 = 1;

    /// Stable entry tag mixed into every appended entry.
    const ENTRY_TAG: &'static [u8] = b"entry";
}

/// A single mutation-log entry for a [`MutationLogSet`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogEntry<S: MutationLogSet> {
    /// Reflect-backed component bytes produced by the default prototype path.
    Reflect {
        /// A stable component label for diagnostics and log readers.
        component: &'static str,
        /// Serialized post-mutation component state.
        bytes: Vec<u8>,
        /// Marker tying this entry to its set.
        marker: PhantomData<fn() -> S>,
    },
    /// Consumer-supplied canonical bytes.
    ///
    /// This lets downstream crates preserve an existing wire format while still
    /// routing their rolling hash and history through the primitive.
    CanonicalBytes {
        /// Stable bytes that define this mutation for hashing and replay.
        bytes: Vec<u8>,
        /// Marker tying this entry to its set.
        marker: PhantomData<fn() -> S>,
    },
}

impl<S: MutationLogSet> LogEntry<S> {
    /// Creates a reflect-backed log entry.
    pub fn reflect(component: &'static str, bytes: impl Into<Vec<u8>>) -> Self {
        Self::Reflect {
            component,
            bytes: bytes.into(),
            marker: PhantomData,
        }
    }

    /// Creates a consumer-supplied canonical byte entry.
    pub fn canonical_bytes(bytes: impl Into<Vec<u8>>) -> Self {
        Self::CanonicalBytes {
            bytes: bytes.into(),
            marker: PhantomData,
        }
    }

    /// Returns the stable bytes carried by this entry.
    pub fn bytes(&self) -> &[u8] {
        match self {
            Self::Reflect { bytes, .. } | Self::CanonicalBytes { bytes, .. } => bytes,
        }
    }
}

/// Rolling mutation log for a declared component set.
#[derive(Resource, Debug, Clone, PartialEq, Eq)]
pub struct MutationLog<S: MutationLogSet> {
    current_hash: [u8; 32],
    history: Vec<[u8; 32]>,
    entries: Vec<LogEntry<S>>,
}

impl<S: MutationLogSet> MutationLog<S> {
    /// Creates a log with the default blake3 genesis hash for `S`.
    pub fn new() -> Self {
        Self::with_genesis_hash(blake3_digest(&[
            S::DOMAIN_TAG,
            &S::DOMAIN_VERSION.to_le_bytes(),
            b"blake3",
        ]))
    }

    /// Creates a log with a caller-provided genesis hash.
    ///
    /// This is intended for downstream compatibility wrappers with existing
    /// domain tags or archived-byte policies.
    pub fn with_genesis_hash(current_hash: [u8; 32]) -> Self {
        Self {
            current_hash,
            history: vec![current_hash],
            entries: Vec::new(),
        }
    }

    /// Returns the rolling hash after the most recent entry.
    pub fn current_hash(&self) -> [u8; 32] {
        self.current_hash
    }

    /// Returns all entries in append order.
    pub fn entries(&self) -> &[LogEntry<S>] {
        &self.entries
    }

    /// Returns the hash at `offset`, where `0` is the genesis hash.
    pub fn hash_at(&self, offset: usize) -> Option<[u8; 32]> {
        self.history.get(offset).copied()
    }

    /// Appends an entry and folds it with the prototype blake3 algorithm.
    pub fn append(&mut self, entry: LogEntry<S>) {
        let next_hash = blake3_digest(&[
            &self.current_hash,
            S::ENTRY_TAG,
            &(entry.bytes().len() as u64).to_le_bytes(),
            entry.bytes(),
        ]);
        self.append_with_hash(entry, next_hash);
    }

    /// Appends an entry with a caller-computed rolling hash.
    pub fn append_with_hash(&mut self, entry: LogEntry<S>, next_hash: [u8; 32]) {
        self.current_hash = next_hash;
        self.entries.push(entry);
        self.history.push(next_hash);
    }
}

impl<S: MutationLogSet> Default for MutationLog<S> {
    fn default() -> Self {
        Self::new()
    }
}

/// Prototype initializer for a [`MutationLog`] resource.
///
/// `bevy_ecs` cannot depend on `bevy_app`, so this type intentionally exposes
/// direct world initialization. Higher-level Bevy crates can wrap it in a
/// `Plugin` without changing the consumer-facing log API.
pub struct MutationLogPlugin<S: MutationLogSet> {
    marker: PhantomData<fn() -> S>,
}

impl<S: MutationLogSet> MutationLogPlugin<S> {
    /// Inserts [`MutationLog<S>`] if it is not already present.
    pub fn init_world(&self, world: &mut World) {
        if !world.contains_resource::<MutationLog<S>>() {
            world.init_resource::<MutationLog<S>>();
        }
    }
}

impl<S: MutationLogSet> Default for MutationLogPlugin<S> {
    fn default() -> Self {
        Self {
            marker: PhantomData,
        }
    }
}

fn blake3_digest(parts: &[&[u8]]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    for part in parts {
        hasher.update(part);
    }
    hasher.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestSet;

    impl MutationLogSet for TestSet {
        const DOMAIN_TAG: &'static [u8] = b"bevy_ecs.mutation_log.test.v1";
    }

    #[test]
    fn genesis_and_append_are_deterministic() {
        let mut left = MutationLog::<TestSet>::new();
        let mut right = MutationLog::<TestSet>::new();

        assert_eq!(left.current_hash(), right.current_hash());
        assert_eq!(left.hash_at(0), Some(left.current_hash()));

        left.append(LogEntry::canonical_bytes([1, 2, 3]));
        right.append(LogEntry::canonical_bytes([1, 2, 3]));

        assert_eq!(left.current_hash(), right.current_hash());
        assert_eq!(left.hash_at(1), Some(left.current_hash()));
        assert_eq!(left.entries().len(), 1);
    }
}
