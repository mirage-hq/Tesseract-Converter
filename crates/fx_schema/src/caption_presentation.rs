//! Sparse occurrence-local presentation for captions projected by FX donor
//! layers.
//!
//! Projected effective-caption ids are runtime view identities and never
//! belong in this persisted model. A caption-capable video or audio layer is
//! the occurrence key; each sparse entry addresses canonical project
//! transcription coordinates.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// How an FX donor phrase position interacts with shared caption placement.
/// Missing values retain the historical keyframe-first behavior.
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Serialize, Deserialize, TS)]
#[ts(export_to = "project_types.d.ts")]
pub enum DonorCaptionPositionMode {
    #[default]
    #[serde(rename = "CAPTION_POSITION_MODE_UNSPECIFIED")]
    Unspecified,
    #[serde(rename = "CAPTION_POSITION_MODE_INHERIT_GLOBAL")]
    InheritGlobal,
    #[serde(rename = "CAPTION_POSITION_MODE_CUSTOM_FOR_PHRASE")]
    CustomForPhrase,
}

impl DonorCaptionPositionMode {
    fn is_unspecified(&self) -> bool {
        *self == Self::Unspecified
    }
}

/// Sparse caption presentation attached to one caption-capable donor layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub struct DonorCaptionPresentation {
    /// Canonically addressed phrase presentation overrides for this layer.
    /// These stay separate from the shared project transcription content.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub phrase_overrides: Vec<DonorCaptionPhrasePresentationOverride>,
    /// Canonically addressed word presentation overrides for this layer.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub word_overrides: Vec<DonorCaptionWordPresentationOverride>,
}

/// Occurrence-local presentation for one canonical transcription phrase.
///
/// Caption templates and positions belong to the host project model rather
/// than this portable schema, so their wire-compatible JSON payloads stay
/// opaque here and are decoded by the host read model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub struct DonorCaptionPhrasePresentationOverride {
    /// Canonical project transcription id selected by the donor asset.
    pub transcription_id: String,
    /// Canonical source-relative phrase id inside the transcription.
    pub phrase_id: String,
    /// Occurrence-local caption-template override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "unknown")]
    pub template: Option<serde_json::Value>,
    /// Occurrence-local caption-position override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "unknown")]
    pub position: Option<serde_json::Value>,
    /// Versioned precedence for this donor-owned phrase position.
    #[serde(
        default,
        skip_serializing_if = "DonorCaptionPositionMode::is_unspecified"
    )]
    pub position_mode: DonorCaptionPositionMode,
    /// Stable caller-supplied override identity when one was provided.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub override_id: Option<String>,
}

impl DonorCaptionPhrasePresentationOverride {
    /// Whether this entry carries no presentation override.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        // Keep this destructure exhaustive so adding a persisted field forces
        // its pruning semantics to be considered here at compile time.
        let Self {
            transcription_id: _,
            phrase_id: _,
            template,
            position,
            position_mode,
            override_id: _,
        } = self;
        template.is_none()
            && position.is_none()
            && *position_mode == DonorCaptionPositionMode::Unspecified
    }
}

#[cfg(test)]
mod phrase_override_tests {
    use super::*;

    fn entry(position_mode: DonorCaptionPositionMode) -> DonorCaptionPhrasePresentationOverride {
        DonorCaptionPhrasePresentationOverride {
            transcription_id: "transcription".into(),
            phrase_id: "phrase".into(),
            template: None,
            position: None,
            position_mode,
            override_id: None,
        }
    }

    #[test]
    fn inherit_global_mode_only_entry_survives_pruning() {
        let mut entries = vec![entry(DonorCaptionPositionMode::InheritGlobal)];
        entries.retain(|entry| !entry.is_empty());
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].position_mode,
            DonorCaptionPositionMode::InheritGlobal
        );
    }

    #[test]
    fn unspecified_mode_only_entry_is_empty() {
        assert!(entry(DonorCaptionPositionMode::Unspecified).is_empty());
    }
}

/// Occurrence-local presentation for one canonical transcription word.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub struct DonorCaptionWordPresentationOverride {
    /// Canonical project transcription id selected by the donor asset.
    pub transcription_id: String,
    /// Canonical source-relative phrase id inside the transcription.
    pub phrase_id: String,
    /// Canonical source-relative word id inside the phrase.
    pub word_id: String,
    /// Occurrence-local emphasized presentation state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub emphasized: Option<bool>,
    /// Occurrence-local supersized presentation state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub supersized: Option<bool>,
    /// Occurrence-local underline presentation state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub underlined: Option<bool>,
    /// Occurrence-local visibility state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub hidden: Option<bool>,
    /// Occurrence-local static emoji write, including an explicit clear.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub emoji: Option<DonorCaptionEmojiOverride>,
    /// Occurrence-local word color write, including an explicit clear.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub color_override: Option<DonorCaptionColorOverride>,
    /// Occurrence-local animated emoji write, including an explicit clear.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub animated_emoji: Option<DonorCaptionJsonOverride>,
    /// Occurrence-local word background write, including an explicit clear.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub word_background_effect: Option<DonorCaptionJsonOverride>,
    /// Occurrence-local font override write, including an explicit clear.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub font_override_name: Option<DonorCaptionJsonOverride>,
    /// Occurrence-local animation override write, including an explicit clear.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub animation_override: Option<DonorCaptionJsonOverride>,
    /// Occurrence-local break-mode write, including an explicit clear.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub word_break_mode: Option<DonorCaptionJsonOverride>,
    /// Occurrence-local negative-mode write, including an explicit clear.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub negative_mode: Option<DonorCaptionJsonOverride>,
    /// Occurrence-local emphasis-animation write, including an explicit clear.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub use_emphasis_animation: Option<DonorCaptionJsonOverride>,
}

impl DonorCaptionWordPresentationOverride {
    /// Whether this entry carries no presentation override.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        // Keep this destructure exhaustive so adding a persisted field forces
        // its pruning semantics to be considered here at compile time.
        let Self {
            transcription_id: _,
            phrase_id: _,
            word_id: _,
            emphasized,
            supersized,
            underlined,
            hidden,
            emoji,
            color_override,
            animated_emoji,
            word_background_effect,
            font_override_name,
            animation_override,
            word_break_mode,
            negative_mode,
            use_emphasis_animation,
        } = self;
        emphasized.is_none()
            && supersized.is_none()
            && underlined.is_none()
            && hidden.is_none()
            && emoji.is_none()
            && color_override.is_none()
            && animated_emoji.is_none()
            && word_background_effect.is_none()
            && font_override_name.is_none()
            && animation_override.is_none()
            && word_break_mode.is_none()
            && negative_mode.is_none()
            && use_emphasis_animation.is_none()
    }
}

/// Explicit opaque JSON write.
///
/// The wrapper preserves a JSON `null` across serde round-trips, allowing
/// host-model option fields to distinguish a persisted clear from omission.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub struct DonorCaptionJsonOverride {
    /// Host-model value, including JSON `null` for an explicit clear.
    #[ts(type = "unknown")]
    pub value: serde_json::Value,
}

/// Explicit static-emoji write.
///
/// The wrapper distinguishes no override from a persisted clear (`value:
/// null`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub struct DonorCaptionEmojiOverride {
    /// Static emoji text, or `None` to clear the canonical donor value for
    /// this occurrence.
    pub value: Option<String>,
}

/// Explicit word-color write.
///
/// The wrapper distinguishes no override from a persisted clear (`value:
/// null`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub struct DonorCaptionColorOverride {
    /// Normalized word color, or `None` to clear the canonical donor value for
    /// this occurrence.
    pub value: Option<DonorCaptionColor>,
}

/// Normalized RGBA word color persisted by donor caption presentation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub struct DonorCaptionColor {
    pub red: f64,
    pub green: f64,
    pub blue: f64,
    pub alpha: f64,
}
