//! Saved Essential Graphics text-controller resolution without an Adobe runtime.
//!
//! Typed source and current editable snapshots feed the ordinary graphic consumer.
//! Every request binds a UUID to its native composition/layer/Source Text path;
//! names and positions never substitute for missing controller identity.

use fx_schema::{PositiveProperty, TextDocument};
use thiserror::Error;

use crate::structure::{ItemKind, StructuralProject};

mod controls;
pub use controls::SavedGraphicNumeric;

/// A malformed or unsupported saved controller cannot select another text layer.
#[derive(Debug, Error)]
pub enum GraphicTemplateError {
    /// Native project framing or item graph is malformed.
    #[error(transparent)]
    Source(#[from] crate::structure::StructureError),
    /// Source Text framing or character-style decoding failed.
    #[error(transparent)]
    Text(#[from] SavedGraphicTextError),
    /// A controller is absent, ambiguous, or outside the static text subset.
    #[error("saved graphic template: {0}")]
    Controller(String),
}

/// Opaque typed Source Text error; its native property/COS cause stays available.
#[derive(Debug, Error)]
#[error("saved Source Text: {source}")]
pub struct SavedGraphicTextError {
    #[source]
    source: crate::structure_document::text::TextError,
}

/// Immutable decoded source. Instance edits never change the template or siblings.
#[derive(Debug, Clone)]
pub struct SavedGraphicTemplate {
    project: StructuralProject,
}

/// Current text/style and exact native identity of one saved text controller.
#[derive(Debug, Clone)]
pub struct SavedGraphicText {
    /// Native controller UUID from the Essential Graphics declaration.
    pub controller_uuid: String,
    /// Native source composition ID, not a name-based selection.
    pub composition_id: u32,
    /// Native source layer ID, not the layer's index or display name.
    pub layer_id: u32,
    /// Current editable single-style document. No scripts or hidden source payload.
    pub document: TextDocument,
    /// Exact saved PostScript identity; no installed-font availability claim.
    pub postscript_font: String,
    /// Static-source limitations inherited from the ordinary AEP text decoder.
    pub diagnostics: Vec<String>,
}

impl SavedGraphicTemplate {
    /// Decode native AEP bytes using the ordinary typed AEP reader.
    ///
    /// # Errors
    /// Returns the reader's framing/item-graph errors without choosing a fallback.
    pub fn decode(aep: &[u8]) -> Result<Self, GraphicTemplateError> {
        Ok(Self {
            project: crate::structure::read_project(aep)?,
        })
    }

    /// Read the immutable typed source for geometry/clock conversion.
    pub fn source(&self) -> &StructuralProject {
        &self.project
    }

    /// Decode current independent Text/Shape contents for the ordinary graphic
    /// consumer. Authoring dependencies and responsive width have contextual
    /// static-approximation diagnostics, never destination scripts.
    ///
    /// # Errors
    /// Rejects forged/mismatched bindings and invalid native Text content.
    pub fn editable_layers(
        &self,
        values: &[SavedGraphicText],
    ) -> Result<(Vec<fx_schema::GroupLayer>, Vec<String>), GraphicTemplateError> {
        let composition_id = values
            .first()
            .ok_or_else(|| GraphicTemplateError::Controller("no resolved Text controllers".into()))?
            .composition_id;
        if values
            .iter()
            .any(|value| value.composition_id != composition_id)
        {
            return Err(GraphicTemplateError::Controller(
                "controllers span several compositions".into(),
            ));
        }
        self.validate_text_bindings(values)?;
        crate::structure_document::graphic_template::layers(&self.project, composition_id, values)
    }

    /// Decode the known template composition even when optional overrides cannot
    /// map. Selection is established by controller UUIDs, never display names.
    ///
    /// # Errors
    /// Rejects ambiguous controller/composition identities or corrupt structure.
    pub fn instantiate(
        &self,
        uuids: &[&str],
    ) -> Result<(Self, u32, Vec<String>), GraphicTemplateError> {
        controls::instantiate(self, uuids)
    }

    /// Apply an instance numeric value through its real Essential Property path.
    /// Failed optional overrides leave the immutable template value untouched.
    ///
    /// # Errors
    /// Rejects unresolved/ambiguous paths and incompatible/nonfinite values.
    pub fn apply_numeric(
        &mut self,
        uuid: &str,
        value: &SavedGraphicNumeric,
    ) -> Result<Vec<String>, GraphicTemplateError> {
        controls::apply_numeric(self, uuid, value)
    }

    /// Retain known Text/Shape children from an explicitly UUID-bound template.
    /// Optional unrepresentable instance controls are diagnosed by the caller.
    ///
    /// # Errors
    /// Rejects a missing required composition or mismatched Text binding.
    pub fn editable_layers_in_composition(
        &self,
        composition_id: u32,
        values: &[SavedGraphicText],
    ) -> Result<(Vec<fx_schema::GroupLayer>, Vec<String>), GraphicTemplateError> {
        if values
            .iter()
            .any(|value| value.composition_id != composition_id)
        {
            return Err(GraphicTemplateError::Controller(
                "controller outside requested composition".into(),
            ));
        }
        self.validate_text_bindings(values)?;
        crate::structure_document::graphic_template::layers(&self.project, composition_id, values)
    }

    fn validate_text_bindings(
        &self,
        values: &[SavedGraphicText],
    ) -> Result<(), GraphicTemplateError> {
        for value in values {
            let original = self.text(&value.controller_uuid)?;
            if (original.composition_id, original.layer_id)
                != (value.composition_id, value.layer_id)
            {
                return Err(GraphicTemplateError::Controller(
                    "mismatched Text binding".into(),
                ));
            }
        }
        Ok(())
    }

    /// Resolve one UUID to the ordinary AEP decoder's static editable document.
    ///
    /// # Errors
    /// Rejects missing/duplicate UUIDs, incomplete source references, a non-Text
    /// path, malformed source metadata, and keyed/mixed-style Source Text.
    pub fn text(&self, uuid: &str) -> Result<SavedGraphicText, GraphicTemplateError> {
        let error =
            |message: &str| GraphicTemplateError::Controller(format!("{uuid:?}: {message}"));
        let mut candidates = self
            .project
            .items
            .iter()
            .filter_map(|item| {
                let ItemKind::Composition(composition) = &item.kind else {
                    return None;
                };
                Some((item.id, composition))
            })
            .flat_map(|(id, composition)| {
                composition
                    .essential_properties
                    .values
                    .iter()
                    .filter(move |controller| controller.uuid == uuid)
                    .map(move |controller| (id, composition, controller))
            });
        let (declaring_id, declaring, controller) = candidates
            .next()
            .ok_or_else(|| error("controller is absent"))?;
        if candidates.next().is_some() {
            return Err(error("controller UUID is ambiguous"));
        }
        let composition_id = controller
            .source_comp_id
            .ok_or_else(|| error("source composition ID is absent"))?;
        let layer_id = controller
            .source_layer_id
            .ok_or_else(|| error("source layer ID is absent"))?;
        if controller.controller_type != 6
            || controller.path.len() != 2
            || controller.path[0].match_name != "ADBE Text Properties"
            || controller.path[1].match_name != "ADBE Text Document"
            || controller
                .path
                .iter()
                .any(|component| component.child_index.is_some())
        {
            return Err(error("controller is not a direct Source Text property"));
        }
        // Cross-composition declarations must resolve the explicit source ID too.
        let composition = if composition_id == declaring_id {
            declaring
        } else {
            match self.project.item(composition_id).map(|item| &item.kind) {
                Some(ItemKind::Composition(composition)) => composition,
                _ => return Err(error("source composition is absent")),
            }
        };
        let source = composition
            .layers
            .iter()
            .find(|layer| layer.record.id() == layer_id)
            .ok_or_else(|| error("source layer is absent"))?;
        let (document, postscript_font, diagnostics) =
            crate::structure_document::text::saved_graphic_document(source)
                .map_err(|source| SavedGraphicTextError { source })?;
        Ok(SavedGraphicText {
            controller_uuid: uuid.to_owned(),
            composition_id,
            layer_id,
            document,
            postscript_font,
            diagnostics,
        })
    }
}

impl SavedGraphicText {
    /// Apply a current uniform saved instance value, not only enabled UI controls.
    /// Saved values remain effective when the font editing controls are disabled.
    ///
    /// # Errors
    /// Rejects empty font identity and nonpositive/nonfinite size before mutation.
    pub fn set_value(
        &mut self,
        text: &str,
        postscript_font: &str,
        size: f64,
        all_caps: bool,
    ) -> Result<(), GraphicTemplateError> {
        let size = PositiveProperty::new(size)
            .ok_or_else(|| GraphicTemplateError::Controller("invalid saved font size".into()))?;
        if postscript_font.is_empty() {
            return Err(GraphicTemplateError::Controller(
                "empty saved font identity".into(),
            ));
        }
        let (family, style) = crate::structure_document::text::split_font_identity(postscript_font);
        self.document.text = text.replace("\r\n", "\n").replace('\r', "\n");
        self.document.font_family = family.into();
        self.document.font_style = style.into();
        self.document.font_size = size;
        self.document.all_caps = all_caps;
        self.postscript_font = postscript_font.to_owned();
        Ok(())
    }
}
