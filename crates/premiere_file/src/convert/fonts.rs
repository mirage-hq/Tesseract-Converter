//! Text font identity from the document, never from a bundled catalog.
//!
//! Converted Premiere text stores the PostScript name as its family with an
//! empty style. The editable schema fills an absent style with `Regular`, so an
//! empty style is written only on purpose: it marks a stored PostScript name.
//! `tsrct` renders only fonts packaged in the document, and export follows the
//! same rule.

use fx_schema::{custom_font_selection_name, FontAssetProperties};
use std::collections::BTreeMap;

/// Diagnostic for a font that the document does not package. `tsrct` refuses to
/// render the text until the font is imported.
pub(super) fn not_packaged(font: &str, before: &str) -> String {
    format!(
        "font \"{font}\" is not packaged in this document; import it with tsrct project import-font before {before}."
    )
}

/// Returns the PostScript name that Premiere uses to find a text layer's font,
/// or the reason to omit the layer.
///
/// The first rule that applies wins:
/// 1. A packaged face (`metadata.json` `fonts`) matches the family and style
///    by its family/style names, its typographic names, or a selection name.
/// 2. An empty style marks a stored PostScript name.
/// 3. Otherwise the document does not package the font.
pub(super) fn postscript_name(
    family: &str,
    style: &str,
    fonts: &BTreeMap<String, FontAssetProperties>,
) -> Result<String, String> {
    let request = custom_font_selection_name(family, style);
    for face in fonts.values().flat_map(|font| &font.faces) {
        let typographic = (
            face.typographic_family_name
                .as_deref()
                .unwrap_or(&face.family_name),
            face.typographic_style_name
                .as_deref()
                .unwrap_or(&face.style_name),
        );
        if (face.family_name.as_str(), face.style_name.as_str()) == (family, style)
            || typographic == (family, style)
        {
            return Ok(face.postscript_name.clone());
        }
        let Some(request) = request
            .as_deref()
            .filter(|request| face.selection_names.iter().any(|name| name == request))
        else {
            continue;
        };
        // A selection name other than the face's own selects a named instance.
        return match face
            .variation_instances
            .iter()
            .find(|instance| instance.selection_name == request)
        {
            None => Ok(face.postscript_name.clone()),
            Some(instance) => instance.postscript_name.clone().ok_or_else(|| {
                format!(
                    "font \"{family} {style}\" selects the named instance {:?} of the packaged face {:?}, which has no PostScript name of its own, so Premiere cannot select it.",
                    instance.name, face.postscript_name
                )
            }),
        };
    }
    if style.is_empty() {
        return Ok(family.to_owned());
    }
    Err(not_packaged(&format!("{family} {style}"), "export"))
}
