//! Explicit conversion-time font availability; never a host-font probe.

use std::collections::BTreeSet;

use serde_json::Value;

use crate::{ImportDiagnostic, Limitation};

pub(super) fn apply(
    document: &mut Value,
    available: Option<&BTreeSet<String>>,
) -> Vec<ImportDiagnostic> {
    let mut diagnostics = Vec::new();
    if let Some(available) = available {
        visit(document, available, &mut diagnostics);
    }
    diagnostics
}

fn visit(value: &mut Value, available: &BTreeSet<String>, diagnostics: &mut Vec<ImportDiagnostic>) {
    match value {
        Value::Object(object) => {
            // Both static sourceText and animated TextDocument values have this
            // shape. Work on the serialized editable model, not source AEP bytes.
            if object.get("text").and_then(Value::as_str).is_some()
                && let Some(family) = object.get("fontFamily").and_then(Value::as_str)
            {
                let style = object
                    .get("fontStyle")
                    .and_then(Value::as_str)
                    .unwrap_or("Regular");
                let original = if style.is_empty() {
                    family.to_owned()
                } else {
                    format!("{family}-{style}")
                };
                if !available.contains(&original) {
                    let suffix = if style.is_empty() {
                        original
                            .rsplit_once('-')
                            .map_or("Regular", |(_, suffix)| suffix)
                    } else {
                        style
                    };
                    let preferred = format!("Inter-{suffix}");
                    let replacement = available
                        .iter()
                        .find(|name| name.eq_ignore_ascii_case(&preferred))
                        .map(String::as_str)
                        .unwrap_or("Inter-Regular");
                    let availability_note = if available.contains(replacement) {
                        "Font bytes must still be staged or packaged before rendering."
                    } else {
                        "Inter-Regular is also absent from the inventory; stage or import its bytes before rendering."
                    };
                    diagnostics.push(ImportDiagnostic {
                        limitation: Limitation::Properties,
                        composition_id: None,
                        layer_id: None,
                        message: format!(
                            "Source Text font {original:?} is absent from the explicit available-font inventory; substituted {replacement:?}. Original authored identity is retained in this diagnostic; glyph metrics and layout may change. {availability_note}"
                        ),
                    });
                    object.insert("fontFamily".into(), Value::String(replacement.into()));
                    // Preserve the authoritative PostScript namespace (#4805).
                    object.insert("fontStyle".into(), Value::String(String::new()));
                }
            }
            for child in object.values_mut() {
                visit(child, available, diagnostics);
            }
        }
        Value::Array(values) => {
            for child in values {
                visit(child, available, diagnostics);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AfterEffects, AfterEffectsImportOptions};
    use fx_conv::{ConversionMode, ImportToTesseract};
    use tesseract_file::TesseractFile;

    #[test]
    fn available_fonts_no_inventory_preserves_authored_identity() {
        let mut document = serde_json::json!({"sourceText": {
            "text": "hello", "fontFamily": "NeverInstalledFace", "fontStyle": "Bold"
        }});
        let original = document.clone();
        assert!(apply(&mut document, None).is_empty());
        assert_eq!(document, original);
    }

    #[test]
    fn available_fonts_found_faces_and_source_text_holds_keep_their_structure() {
        let mut document = serde_json::json!({
            "sourceText": {"text": "first", "fontFamily": "Found", "fontStyle": "Regular"},
            "keys": [{"value": {"type": "textDocument", "value": {
                "text": "second", "fontFamily": "Other", "fontStyle": "BoldItalic", "fontSize": 37
            }}}]
        });
        let available = BTreeSet::from([
            "Found-Regular".into(),
            "Inter-Regular".into(),
            "Inter-BoldItalic".into(),
        ]);
        let warnings = apply(&mut document, Some(&available));
        assert_eq!(document["sourceText"]["fontFamily"], "Found");
        assert_eq!(document["sourceText"]["fontStyle"], "Regular");
        let hold = &document["keys"][0]["value"]["value"];
        assert_eq!(hold["fontFamily"], "Inter-BoldItalic");
        assert_eq!(hold["fontStyle"], "");
        assert_eq!(hold["text"], "second");
        assert_eq!(hold["fontSize"], 37);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("Other-BoldItalic"));
        assert!(warnings[0].message.contains("Inter-BoldItalic"));
    }

    #[test]
    fn available_fonts_empty_inventory_falls_back_and_reports_missing_inter() {
        let mut document = serde_json::json!({"sourceText": {
            "text": "hello", "fontFamily": "Missing-Heavy", "fontStyle": ""
        }});
        let warnings = apply(&mut document, Some(&BTreeSet::new()));
        assert_eq!(document["sourceText"]["fontFamily"], "Inter-Regular");
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("Missing-Heavy"));
        assert!(warnings[0].message.contains("also absent"));
    }

    fn font_names(value: &Value, names: &mut BTreeSet<String>) {
        match value {
            Value::Object(object) => {
                if let Some(name) = object.get("fontFamily").and_then(Value::as_str) {
                    names.insert(name.to_owned());
                }
                for child in object.values() {
                    font_names(child, names);
                }
            }
            Value::Array(values) => {
                for child in values {
                    font_names(child, names);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn available_fonts_native_import_replaces_unavailable_face_and_warns() {
        let source =
            include_bytes!("../../tests/fixtures/pr4442_native/sources/text_document_point.aep");
        let root = tempfile::tempdir().unwrap();
        let input = root.path().join("source.aep");
        std::fs::write(&input, source).unwrap();
        let project = crate::structure::read_project(source).unwrap();
        let id = project
            .items
            .iter()
            .find(|item| matches!(item.kind, crate::structure::ItemKind::Composition(_)))
            .unwrap()
            .id;
        let mut options = AfterEffectsImportOptions {
            composition: Some(id),
            ..Default::default()
        };
        let original_path = root.path().join("original");
        AfterEffects
            .import_to_tesseract(&input, &original_path, &options, ConversionMode::Write)
            .unwrap();
        let original = TesseractFile::open(original_path.join("project.tsrct")).unwrap();
        let mut names = BTreeSet::new();
        font_names(&original.project_json().unwrap(), &mut names);
        assert!(
            !names.is_empty(),
            "native fixture must contain editable text"
        );
        assert!(!names.contains("Inter-Regular"));

        options.available_fonts = Some(BTreeSet::from(["Inter-Regular".to_owned()]));
        let output = root.path().join("fallback");
        let report = AfterEffects
            .import_to_tesseract(&input, &output, &options, ConversionMode::Write)
            .unwrap();
        let archive = TesseractFile::open(output.join("project.tsrct")).unwrap();
        let mut replacement = BTreeSet::new();
        font_names(&archive.project_json().unwrap(), &mut replacement);
        assert_eq!(replacement, BTreeSet::from(["Inter-Regular".to_owned()]));
        for name in names {
            assert!(
                report
                    .diagnostics
                    .iter()
                    .any(|warning| warning.message.contains(&name)
                        && warning.message.contains("Inter-Regular")
                        && warning.message.contains("available-font"))
            );
        }
    }
}
