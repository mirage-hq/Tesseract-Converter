//! Canonical persisted AI Edit compatibility-layer schema.
use super::Layer;
use crate::{Duration, LayerId, Time, TimeRangeProperty};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[path = "ai_edit_declaration.rs"]
mod declaration;
crate::define_ai_edit_layer_schema!();

/// Sticker content owned by one semantic AI Edit segment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub struct AiEditSticker {
    pub content: AiEditStickerContent,
    /// Display duration in milliseconds.
    pub duration: Duration,
    /// Canvas-relative center used when no explicit origin is present.
    pub location: AiEditStickerLocation,
    /// Optional point in the legacy canvas coordinate space.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub origin: Option<AiEditStickerPoint>,
    /// Project-timeline display start in milliseconds.
    pub start_time: Time,
    /// Caption word associated with this sticker.
    pub word_id: String,
    #[serde(default)]
    pub is_hidden: bool,
    /// Clockwise rotation in degrees.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub rotation: Option<f64>,
}

impl AiEditSticker {
    #[must_use]
    pub fn end_time(&self) -> Time {
        self.start_time.saturating_add(self.duration)
    }

    #[must_use]
    pub fn pag_asset_id(&self) -> Option<&str> {
        self.content.pag.as_deref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub struct AiEditStickerContent {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub pag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub text: Option<String>,
}

/// Canvas-relative point expressed as fractions from `0.0` to `1.0`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub struct AiEditStickerLocation {
    pub horizontal_pct: f64,
    pub vertical_pct: f64,
}

/// Point expressed in the legacy canvas coordinate space.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub struct AiEditStickerPoint {
    pub x: f64,
    pub y: f64,
}

/// Base rendered below background PAG content and the cut-out source subject.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub enum AiEditBackground {
    /// Paint the unmasked source media below the background PAG.
    Source,
    /// Paint an opaque black canvas below the background PAG.
    Black,
}
