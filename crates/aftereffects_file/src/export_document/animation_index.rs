use std::{collections::BTreeMap, ops::Deref};

use fx_schema::{LayerId, animator::AnimationGraphEntry, property::Property};

/// Borrows the source graph so indexing never clones stored animator payloads.
#[derive(Debug)]
pub(super) struct AnimationIndex<'data> {
    entries: &'data [AnimationGraphEntry],
    first: BTreeMap<Property, usize>,
    layers: BTreeMap<LayerId, Vec<usize>>,
}

impl<'data> AnimationIndex<'data> {
    pub(super) fn new(entries: &'data [AnimationGraphEntry]) -> Self {
        let mut first = BTreeMap::new();
        let mut layers: BTreeMap<LayerId, Vec<usize>> = BTreeMap::new();
        for (position, entry) in entries.iter().enumerate() {
            if let Some(property) = entry.target.as_property() {
                // Match the original find(), even when the first entry is invalid.
                first.entry(property).or_insert(position);
            }
            if let Some(layer_id) = entry.target.layer_id() {
                layers.entry(layer_id).or_default().push(position);
            }
        }
        Self {
            entries,
            first,
            layers,
        }
    }

    pub(super) fn first(&self, property: Property) -> Option<&'data AnimationGraphEntry> {
        self.first
            .get(&property)
            .map(|&position| &self.entries[position])
    }

    pub(super) fn for_layer(
        &self,
        layer_id: LayerId,
    ) -> impl Iterator<Item = &'data AnimationGraphEntry> + '_ {
        self.layers
            .get(&layer_id)
            .into_iter()
            .flatten()
            .map(|&position| &self.entries[position])
    }
}

impl Deref for AnimationIndex<'_> {
    type Target = [AnimationGraphEntry];

    fn deref(&self) -> &Self::Target {
        self.entries
    }
}

impl<'a> IntoIterator for &'a AnimationIndex<'_> {
    type Item = &'a AnimationGraphEntry;
    type IntoIter = std::slice::Iter<'a, AnimationGraphEntry>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.iter()
    }
}

#[cfg(test)]
mod tests;
