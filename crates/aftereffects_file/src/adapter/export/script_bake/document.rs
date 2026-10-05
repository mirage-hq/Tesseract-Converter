//! Keep generated Path keys encoded until the final checked document is read.
//!
//! A `Value` object for every coordinate/property has much more allocation
//! overhead than the JSON itself. Only the objects needed to replace animators
//! are opened here; unrelated fields and completed tracks stay as raw JSON.

use std::collections::BTreeMap;

use fx_schema::{EditableFxCompositionDocument, EditableFxDocumentError, PropertyAnimator};
use serde_json::value::{RawValue, to_raw_value};

type Object = BTreeMap<String, Box<RawValue>>;

pub(super) struct BakedDocument {
    document: Object,
    composition: Object,
    dynamics: Object,
    entries: Vec<Object>,
}

impl BakedDocument {
    pub(super) fn new(document: &EditableFxCompositionDocument) -> serde_json::Result<Self> {
        let mut document: Object = serde_json::from_slice(&serde_json::to_vec(document)?)?;
        // The source is a validated document with at least one script entry.
        let mut composition: Object = serde_json::from_str(
            document
                .remove("composition")
                .expect("validated document has a composition")
                .get(),
        )?;
        let mut dynamics: Object = serde_json::from_str(
            composition
                .remove("dynamics")
                .expect("script-bearing composition has dynamics")
                .get(),
        )?;
        let entries = serde_json::from_str(
            dynamics
                .remove("entries")
                .expect("script-bearing graph has entries")
                .get(),
        )?;
        Ok(Self {
            document,
            composition,
            dynamics,
            entries,
        })
    }

    pub(super) fn replace_animator(
        &mut self,
        index: usize,
        animator: &PropertyAnimator,
    ) -> serde_json::Result<()> {
        // Match the previous known_value() replacement, including its treatment
        // of unknown fields on a successfully replaced animator.
        self.entries[index].insert("animator".into(), to_raw_value(animator.data())?);
        Ok(())
    }

    pub(super) fn replace_script(
        &mut self,
        index: usize,
        animator: &PropertyAnimator,
    ) -> serde_json::Result<()> {
        self.replace_animator(index, animator)?;
        // Generated keys are standalone: these fields described the original
        // script's evaluation, not the new editable track. Source stays intact.
        self.entries[index].remove("dependencies");
        self.entries[index].remove("layerRefs");
        self.entries[index].remove("randomSeedTarget");
        Ok(())
    }

    pub(super) fn finish(
        mut self,
    ) -> Result<EditableFxCompositionDocument, EditableFxDocumentError> {
        let entries = to_raw_value(&self.entries)?;
        drop(self.entries);
        self.dynamics.insert("entries".into(), entries);
        let dynamics = to_raw_value(&self.dynamics)?;
        drop(self.dynamics);
        self.composition.insert("dynamics".into(), dynamics);
        let composition = to_raw_value(&self.composition)?;
        drop(self.composition);
        self.document.insert("composition".into(), composition);
        // The slice reader runs the same structural checks without first
        // rebuilding a recursive Value tree for all generated Path points.
        EditableFxCompositionDocument::from_json_slice(&serde_json::to_vec(&self.document)?)
    }
}
