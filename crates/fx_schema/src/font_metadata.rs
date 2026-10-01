//! Portable metadata and validation for embedded and project-scoped fonts.

mod extensions;

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::id::AssetId;

pub use extensions::AssetDataExtensions;

pub const MAX_CUSTOM_FONT_FACES: usize = 64;
pub const MAX_CUSTOM_FONT_AXES_PER_FACE: usize = 16;
pub const MAX_CUSTOM_FONT_INSTANCES_PER_FACE: usize = 256;
pub const MAX_CUSTOM_FONT_SELECTION_NAMES_PER_FACE: usize = 257;
pub const MAX_CUSTOM_FONT_SELECTION_NAME_BYTES: usize = 1024;

/// One OpenType variation axis advertised by an uploaded font face.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub struct FontVariationAxisMetadata {
    pub tag: String,
    #[serde(default)]
    pub name: String,
    pub min: f32,
    pub default: f32,
    pub max: f32,
    #[serde(default)]
    pub hidden: bool,
}

/// One coordinate in an OpenType variable-font instance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub struct FontVariationCoordinateMetadata {
    pub tag: String,
    pub value: f32,
}

/// One font-authored named instance from an OpenType `fvar` table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub struct FontVariationInstanceMetadata {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub postscript_name: Option<String>,
    pub coordinates: Vec<FontVariationCoordinateMetadata>,
    pub selection_name: String,
}

/// One face discovered in an uploaded TTF, OTF, or TTC source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub struct FontFaceMetadata {
    #[serde(default)]
    pub face_index: u32,
    pub postscript_name: String,
    pub full_name: String,
    pub family_name: String,
    pub style_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub typographic_family_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub typographic_style_name: Option<String>,
    pub weight: u16,
    pub width: u8,
    #[serde(default)]
    pub italic: bool,
    #[serde(default)]
    pub is_serif: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub covered_scripts: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variation_axes: Vec<FontVariationAxisMetadata>,
    /// Font-authored named instances. No synthetic weights are added. Keep an
    /// explicit empty array on the wire so authoring clients can distinguish
    /// this reader-first schema from the legacy synthetic-weight inspector.
    #[serde(default)]
    pub variation_instances: Vec<FontVariationInstanceMetadata>,
    /// Ordinary semantic family/style requests offered by authoring clients:
    /// the physical face plus its font-defined named instances. Arbitrary axis
    /// coordinates are persisted separately in the text `fontVariations`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub selection_names: Vec<String>,
}

/// Intrinsic metadata for one project-scoped uploaded font source.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub struct FontAssetProperties {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub faces: Vec<FontFaceMetadata>,
    #[serde(default, flatten)]
    #[ts(flatten)]
    pub extensions: AssetDataExtensions,
}

/// Failure to validate one entry in a portable font registry.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FontRegistryValidationError {
    #[error("invalid font metadata for asset {asset_id}: {reason}")]
    InvalidFontProperties { asset_id: AssetId, reason: String },
    #[error(
        "custom font selection name {selection_name:?} appears in assets {first_asset_id} and {duplicate_asset_id}"
    )]
    DuplicateFontSelectionName {
        selection_name: String,
        first_asset_id: AssetId,
        duplicate_asset_id: AssetId,
    },
}

/// Build the ordinary renderer request for a semantic family/style pair.
#[must_use]
pub fn custom_font_selection_name(family_name: &str, style_name: &str) -> Option<String> {
    let family_name = family_name.trim();
    let style_name = style_name.trim();
    if family_name.is_empty()
        || style_name.is_empty()
        || family_name.contains('/')
        || style_name.contains('/')
        || family_name.chars().any(char::is_control)
        || style_name.chars().any(char::is_control)
    {
        return None;
    }
    Some(format!("{family_name}/{style_name}"))
}

/// Validate an embedded semantic font registry.
///
/// Source keys identify byte sources but do not participate in semantic font
/// selection. Collisions with a host catalog are intentionally not checked.
pub fn validate_embedded_font_registry<'a>(
    entries: impl IntoIterator<Item = (&'a str, &'a FontAssetProperties)>,
) -> Result<(), String> {
    let mut selection_names = BTreeMap::new();
    for (source_key, properties) in entries {
        validate_font_asset_properties(
            &AssetId::from_trusted(source_key),
            properties,
            &mut selection_names,
            |_| false,
        )
        .map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// Validate one font source and add its semantic names to a registry.
///
/// `is_reserved_selection_name` lets a host reject collisions with its own
/// bundled catalog without making the portable schema depend on that catalog.
pub fn validate_font_asset_properties(
    asset_id: &AssetId,
    properties: &FontAssetProperties,
    selection_names: &mut BTreeMap<String, AssetId>,
    is_reserved_selection_name: impl Fn(&str) -> bool,
) -> Result<(), FontRegistryValidationError> {
    let invalid = |reason: String| FontRegistryValidationError::InvalidFontProperties {
        asset_id: asset_id.clone(),
        reason,
    };
    if properties.faces.is_empty() || properties.faces.len() > MAX_CUSTOM_FONT_FACES {
        return Err(invalid(format!(
            "face count must be between 1 and {MAX_CUSTOM_FONT_FACES}"
        )));
    }
    let mut face_indices = BTreeSet::new();
    for face in &properties.faces {
        if !face_indices.insert(face.face_index) {
            return Err(invalid(format!("duplicate face index {}", face.face_index)));
        }
        if face.postscript_name.trim().is_empty()
            || face.full_name.trim().is_empty()
            || face.family_name.trim().is_empty()
            || face.style_name.trim().is_empty()
        {
            return Err(invalid("font names must be non-empty".to_owned()));
        }
        if face.variation_axes.len() > MAX_CUSTOM_FONT_AXES_PER_FACE {
            return Err(invalid(format!(
                "face {} has too many variation axes",
                face.face_index
            )));
        }
        let mut axis_tags = BTreeSet::new();
        for axis in &face.variation_axes {
            if !axis_tags.insert(axis.tag.as_str())
                || axis.tag.len() != 4
                || !axis.tag.is_ascii()
                || !axis.min.is_finite()
                || !axis.default.is_finite()
                || !axis.max.is_finite()
                || axis.min > axis.default
                || axis.default > axis.max
            {
                return Err(invalid(format!(
                    "face {} has an invalid variation axis",
                    face.face_index
                )));
            }
        }
        if face.variation_instances.len() > MAX_CUSTOM_FONT_INSTANCES_PER_FACE {
            return Err(invalid(format!(
                "face {} has too many named instances",
                face.face_index
            )));
        }
        for instance in &face.variation_instances {
            let expected_name = custom_font_selection_name(&face.family_name, &instance.name);
            if instance.name.trim().is_empty()
                || !custom_font_coordinates_are_valid(face, &instance.coordinates)
                || expected_name.as_deref() != Some(instance.selection_name.as_str())
            {
                return Err(invalid(format!(
                    "face {} has an invalid named instance",
                    face.face_index
                )));
            }
        }
        if face.selection_names.is_empty()
            || face.selection_names.len() > MAX_CUSTOM_FONT_SELECTION_NAMES_PER_FACE
        {
            return Err(invalid(format!(
                "face {} must expose between 1 and {MAX_CUSTOM_FONT_SELECTION_NAMES_PER_FACE} selection names",
                face.face_index
            )));
        }
        let Some(base_selection_name) =
            custom_font_selection_name(&face.family_name, &face.style_name)
        else {
            return Err(invalid(format!(
                "face {} has an invalid family or style name",
                face.face_index
            )));
        };
        let expected_selection_names: BTreeSet<_> = std::iter::once(base_selection_name)
            .chain(
                face.variation_instances
                    .iter()
                    .map(|instance| instance.selection_name.clone()),
            )
            .collect();
        if face.selection_names.iter().collect::<BTreeSet<_>>().len() != face.selection_names.len()
            || face.selection_names.iter().any(|name| {
                !expected_selection_names.contains(name)
                    || name.len() > MAX_CUSTOM_FONT_SELECTION_NAME_BYTES
            })
            || face.selection_names.len() != expected_selection_names.len()
        {
            return Err(invalid(format!(
                "face {} selection names do not match its semantic family/styles",
                face.face_index
            )));
        }
        for name in &face.selection_names {
            if is_reserved_selection_name(name) {
                return Err(invalid(format!(
                    "selection name '{name}' conflicts with the registered font catalog"
                )));
            }
            let lookup_name = name.to_lowercase();
            if let Some(first_asset_id) = selection_names.insert(lookup_name, asset_id.clone()) {
                return Err(FontRegistryValidationError::DuplicateFontSelectionName {
                    selection_name: name.clone(),
                    first_asset_id,
                    duplicate_asset_id: asset_id.clone(),
                });
            }
        }
    }
    Ok(())
}

fn custom_font_coordinates_are_valid(
    face: &FontFaceMetadata,
    coordinates: &[FontVariationCoordinateMetadata],
) -> bool {
    !coordinates.is_empty()
        && coordinates.len() <= MAX_CUSTOM_FONT_AXES_PER_FACE
        && coordinates.iter().all(|coordinate| {
            coordinate.value.is_finite()
                && face.variation_axes.iter().any(|axis| {
                    axis.tag == coordinate.tag
                        && coordinate.value >= axis.min
                        && coordinate.value <= axis.max
                })
        })
        && coordinates
            .iter()
            .map(|coordinate| coordinate.tag.as_str())
            .collect::<BTreeSet<_>>()
            .len()
            == coordinates.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn properties(selection_name: &str) -> FontAssetProperties {
        FontAssetProperties {
            faces: vec![FontFaceMetadata {
                face_index: 0,
                postscript_name: "Brand-Regular".to_owned(),
                full_name: "Brand Regular".to_owned(),
                family_name: "Brand".to_owned(),
                style_name: "Regular".to_owned(),
                typographic_family_name: None,
                typographic_style_name: None,
                weight: 400,
                width: 5,
                italic: false,
                is_serif: false,
                covered_scripts: Vec::new(),
                variation_axes: Vec::new(),
                variation_instances: Vec::new(),
                selection_names: vec![selection_name.to_owned()],
            }],
            ..FontAssetProperties::default()
        }
    }

    #[test]
    fn unknown_fields_round_trip_without_loss() {
        let value = serde_json::json!({
            "futureRegistryField": { "nested": [1, true, null] }
        });
        let properties: FontAssetProperties = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(properties).unwrap(), value);
    }

    #[test]
    fn validation_rejects_reserved_and_duplicate_names() {
        let first_id = AssetId::from_trusted("first");
        let second_id = AssetId::from_trusted("second");
        let properties = properties("Brand/Regular");
        let mut names = BTreeMap::new();
        validate_font_asset_properties(&first_id, &properties, &mut names, |_| false).unwrap();

        let duplicate =
            validate_font_asset_properties(&second_id, &properties, &mut names, |_| false)
                .unwrap_err();
        assert!(matches!(
            duplicate,
            FontRegistryValidationError::DuplicateFontSelectionName { .. }
        ));

        let reserved =
            validate_font_asset_properties(&first_id, &properties, &mut BTreeMap::new(), |name| {
                name == "Brand/Regular"
            })
            .unwrap_err();
        assert!(reserved
            .to_string()
            .contains("conflicts with the registered font catalog"));
    }

    #[test]
    fn invalid_variable_axis_range_is_rejected() {
        let mut properties = properties("Brand/Regular");
        properties.faces[0].variation_axes = vec![FontVariationAxisMetadata {
            tag: "wght".to_owned(),
            name: "Weight".to_owned(),
            min: 900.0,
            default: 400.0,
            max: 100.0,
            hidden: false,
        }];
        let error = validate_embedded_font_registry([("font", &properties)]).unwrap_err();
        assert!(error.contains("invalid variation axis"));
    }
}
