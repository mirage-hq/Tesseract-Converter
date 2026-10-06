use std::collections::BTreeSet;

use fx_schema::LayerId;

/// Insertion-only reservations whose nested attempts can undo their own additions.
/// Initial reservations are never journaled, and successful children retain their
/// entries so a failed outer attempt can still undo them.
#[derive(Debug)]
pub(super) struct IdReservations {
    values: BTreeSet<LayerId>,
    inserted: Vec<LayerId>,
}

impl IdReservations {
    pub(super) fn new(values: BTreeSet<LayerId>) -> Self {
        Self {
            values,
            inserted: Vec::new(),
        }
    }

    pub(super) fn contains(&self, id: &LayerId) -> bool {
        self.values.contains(id)
    }

    pub(super) fn insert(&mut self, id: LayerId) -> bool {
        if !self.values.insert(id) {
            return false;
        }
        self.inserted.push(id);
        true
    }

    pub(super) fn extend(&mut self, ids: impl IntoIterator<Item = LayerId>) {
        for id in ids {
            self.insert(id);
        }
    }

    pub(super) fn checkpoint(&self) -> usize {
        self.inserted.len()
    }

    pub(super) fn rollback(&mut self, checkpoint: usize) {
        while self.inserted.len() > checkpoint {
            if let Some(id) = self.inserted.pop() {
                self.values.remove(&id);
            }
        }
    }

    pub(super) fn as_set(&self) -> &BTreeSet<LayerId> {
        &self.values
    }

    pub(super) fn into_set(self) -> BTreeSet<LayerId> {
        self.values
    }

    /// A probe owns independent values, not the caller's rollback history.
    pub(super) fn fork(&self) -> Self {
        Self::new(self.values.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(values: &[u64]) -> BTreeSet<LayerId> {
        values.iter().copied().map(LayerId::new).collect()
    }

    #[test]
    fn initial_and_duplicate_insertions_survive_rollback() {
        let initial = ids(&[1, 2]);
        let mut reservations = IdReservations::new(initial.clone());
        let checkpoint = reservations.checkpoint();
        assert!(!reservations.insert(LayerId::new(1)));
        assert!(reservations.insert(LayerId::new(3)));
        assert!(!reservations.insert(LayerId::new(3)));
        reservations.extend(ids(&[2, 3, 4]));
        assert_eq!(reservations.as_set(), &ids(&[1, 2, 3, 4]));
        reservations.rollback(checkpoint);
        assert_eq!(reservations.as_set(), &initial);
        reservations.rollback(checkpoint);
        assert_eq!(reservations.into_set(), initial);
    }

    #[test]
    fn nested_attempts_match_snapshot_rollback() {
        let mut oracle = ids(&[1]);
        let mut reservations = IdReservations::new(oracle.clone());
        // A successful sibling precedes the outer attempt.
        oracle.insert(LayerId::new(2));
        reservations.insert(LayerId::new(2));
        let outer_snapshot = oracle.clone();
        let outer = reservations.checkpoint();
        oracle.insert(LayerId::new(3));
        reservations.insert(LayerId::new(3));
        let inner_snapshot = oracle.clone();
        let inner = reservations.checkpoint();
        oracle.extend(ids(&[1, 3, 4, 5]));
        reservations.extend(ids(&[1, 3, 4, 5]));
        assert_eq!(reservations.as_set(), &oracle);
        oracle = inner_snapshot;
        reservations.rollback(inner);
        assert_eq!(reservations.as_set(), &oracle);
        // The next child succeeds; its additions still belong to the outer attempt.
        oracle.extend(ids(&[4, 6]));
        reservations.extend(ids(&[4, 6]));
        assert_eq!(reservations.as_set(), &oracle);
        oracle = outer_snapshot;
        reservations.rollback(outer);
        assert_eq!(reservations.as_set(), &oracle);
        assert!(reservations.insert(LayerId::new(4)));
        oracle.insert(LayerId::new(4));
        assert_eq!(reservations.into_set(), oracle);
    }

    #[test]
    fn forks_and_published_sets_start_independent_histories() {
        let mut original = IdReservations::new(ids(&[1]));
        original.insert(LayerId::new(2));
        let mut probe = original.fork();
        assert_eq!(probe.checkpoint(), 0);
        probe.extend(ids(&[2, 3]));
        original.insert(LayerId::new(4));
        assert_eq!(probe.as_set(), &ids(&[1, 2, 3]));
        assert_eq!(original.as_set(), &ids(&[1, 2, 4]));
        probe.rollback(0);
        assert_eq!(probe.as_set(), &ids(&[1, 2]));
        probe.insert(LayerId::new(5));
        let mut published = IdReservations::new(probe.into_set());
        assert_eq!(published.checkpoint(), 0);
        published.insert(LayerId::new(6));
        published.rollback(0);
        assert_eq!(published.as_set(), &ids(&[1, 2, 5]));
        original.rollback(0);
        assert_eq!(original.as_set(), &ids(&[1]));
    }
}
