//! Ephemeral font identities from hash-verified archive bytes, never host fonts.

mod bounds;

use std::sync::Arc;

use fx_schema::FontFaceMetadata;
use tesseract_file::{AssetKind, TesseractFile, TesseractFileError};
use ttf_parser::{RawFace, Tag, name, name_id};

use crate::writer::text::FontFormat;

#[derive(Debug, Default)]
pub(crate) struct ArchiveFonts {
    faces: Vec<ArchiveFace>,
}

#[derive(Debug)]
struct ArchiveFace {
    metadata: FontFaceMetadata,
    identity: Result<(String, FontFormat), &'static str>,
    bytes: Arc<[u8]>,
}

impl ArchiveFonts {
    pub(crate) fn prepare(archive: &TesseractFile) -> Result<Self, TesseractFileError> {
        let mut faces = Vec::new();
        for (asset_id, properties) in &archive.metadata().fonts {
            let asset = archive.asset(asset_id)?;
            let bytes: Arc<[u8]> = asset
                .read_verified_bytes(asset.descriptor().byte_length)?
                .into();
            for metadata in &properties.faces {
                faces.push(ArchiveFace {
                    metadata: metadata.clone(),
                    identity: inspect_face(&bytes, metadata.face_index, &metadata.postscript_name),
                    bytes: Arc::clone(&bytes),
                });
            }
        }
        // Older archives embed verified font assets without a semantic registry.
        // Their font-authored names, never filenames or aliases, supply identity.
        if archive.metadata().fonts.is_empty() {
            for (asset_id, descriptor) in &archive.metadata().assets {
                if descriptor.kind != AssetKind::Font {
                    continue;
                }
                let asset = archive.asset(asset_id)?;
                let bytes = asset.read_verified_bytes(descriptor.byte_length)?;
                faces.extend(inferred_faces(&bytes));
            }
        }
        Ok(Self { faces })
    }

    pub(crate) fn used_faces<'a>(
        &'a self,
        names: &'a std::collections::BTreeSet<String>,
    ) -> impl Iterator<Item = (&'a FontFaceMetadata, &'a [u8], FontFormat)> {
        self.faces.iter().filter_map(move |face| {
            let (name, format) = face.identity.as_ref().ok()?;
            names
                .contains(name)
                .then_some((&face.metadata, face.bytes.as_ref(), *format))
        })
    }

    pub(super) fn resolve(
        &self,
        family: &str,
        style: &str,
    ) -> Result<(&str, FontFormat), &'static str> {
        let face = self.resolve_face(family, style)?;
        let (name, format) = face.identity.as_ref().map_err(|reason| *reason)?;
        Ok((name.as_str(), *format))
    }

    fn resolve_face(&self, family: &str, style: &str) -> Result<&ArchiveFace, &'static str> {
        let mut matches = self.faces.iter().filter(|face| {
            let metadata = &face.metadata;
            if style.trim().is_empty() {
                return metadata.postscript_name.eq_ignore_ascii_case(family.trim());
            }
            // Physical selection only: named variable instances require a native
            // design-vector grammar, not the physical face's PostScript identity.
            let pair = |f: &str, s: &str| {
                f.eq_ignore_ascii_case(family.trim()) && s.eq_ignore_ascii_case(style.trim())
            };
            pair(&metadata.family_name, &metadata.style_name)
                || pair(
                    metadata
                        .typographic_family_name
                        .as_deref()
                        .unwrap_or(&metadata.family_name),
                    metadata
                        .typographic_style_name
                        .as_deref()
                        .unwrap_or(&metadata.style_name),
                )
        });
        let face = matches
            .next()
            .ok_or("No matching physical face in the archive font registry")?;
        if matches.next().is_some() {
            return Err("Ambiguous archive font family/style or PostScript identity");
        }
        face.identity.as_ref().map_err(|reason| *reason)?;
        Ok(face)
    }
}

pub(crate) fn emitted_font_names(
    layers: &[crate::writer::LayerSpec],
) -> std::collections::BTreeSet<String> {
    fn collect(layer: &crate::writer::LayerSpec, names: &mut std::collections::BTreeSet<String>) {
        use crate::writer::LayerSpec;
        match layer {
            LayerSpec::Options(inner, _) | LayerSpec::Timed(inner, _) => collect(inner, names),
            LayerSpec::Precomposition(spec) => {
                for layer in &spec.layers {
                    collect(layer, names);
                }
            }
            LayerSpec::Text(spec) => {
                names.extend(
                    spec.documents
                        .keys
                        .iter()
                        .map(|key| key.document.font_postscript.clone()),
                );
            }
            _ => {}
        }
    }
    let mut names = std::collections::BTreeSet::new();
    for layer in layers {
        collect(layer, &mut names);
    }
    names
}

fn decoded_font_name(value: name::Name<'_>) -> Option<String> {
    value.to_string().or_else(|| {
        // Roman Macintosh names can contain non-ASCII bytes. Only the shared
        // ASCII subset is established here; do not guess other encodings.
        (value.platform_id == ttf_parser::PlatformId::Macintosh
            && value.encoding_id == 0
            && value.name.is_ascii())
        .then(|| std::str::from_utf8(value.name).ok().map(str::to_owned))
        .flatten()
    })
}

fn inferred_faces(bytes: &[u8]) -> Vec<ArchiveFace> {
    let bytes: Arc<[u8]> = bytes.into();
    let mut faces = Vec::new();
    for index in 0..ttf_parser::fonts_in_collection(&bytes).unwrap_or(1) {
        let Ok(face) = ttf_parser::Face::parse(&bytes, index) else {
            continue;
        };
        let name = |id| {
            let collect = |preferred: bool| {
                face.names()
                    .into_iter()
                    .filter(|name| {
                        name.name_id == id
                            && (!preferred || name.language_id == 0x0409 || name.language_id == 0)
                    })
                    .filter_map(decoded_font_name)
                    .collect::<std::collections::BTreeSet<_>>()
            };
            let values = collect(true);
            let values = if values.is_empty() {
                collect(false)
            } else {
                values
            };
            (values.len() == 1)
                .then(|| values.into_iter().next())
                .flatten()
        };
        let (Some(postscript_name), Some(family_name), Some(style_name), Some(full_name)) = (
            name(name_id::POST_SCRIPT_NAME),
            name(name_id::FAMILY),
            name(name_id::SUBFAMILY),
            name(name_id::FULL_NAME),
        ) else {
            continue;
        };
        let Ok(width) = u8::try_from(face.width().to_number()) else {
            continue;
        };
        let metadata = FontFaceMetadata {
            face_index: index,
            postscript_name,
            full_name,
            family_name,
            style_name,
            typographic_family_name: name(name_id::TYPOGRAPHIC_FAMILY),
            typographic_style_name: name(name_id::TYPOGRAPHIC_SUBFAMILY),
            weight: face.weight().to_number(),
            width,
            italic: face.is_italic(),
            is_serif: false,
            covered_scripts: Vec::new(),
            variation_axes: Vec::new(),
            variation_instances: Vec::new(),
            selection_names: Vec::new(),
        };
        faces.push(ArchiveFace {
            identity: inspect_face(&bytes, index, &metadata.postscript_name),
            metadata,
            bytes: Arc::clone(&bytes),
        });
    }
    faces
}

fn inspect_face(
    bytes: &[u8],
    index: u32,
    expected_name: &str,
) -> Result<(String, FontFormat), &'static str> {
    let face = RawFace::parse(bytes, index).map_err(|_| "Invalid embedded font or face index")?;
    let names = face
        .table(Tag::from_bytes(b"name"))
        .and_then(name::Table::parse)
        .ok_or("Embedded face has no readable name table")?;
    let names = names
        .names
        .into_iter()
        .filter(|name| name.name_id == name_id::POST_SCRIPT_NAME)
        .filter_map(decoded_font_name)
        .collect::<std::collections::BTreeSet<_>>();
    if names.len() != 1 || !names.contains(expected_name) {
        return Err("Archive PostScript metadata disagrees with the indexed font bytes");
    }
    let glyf = face.table(Tag::from_bytes(b"glyf")).is_some();
    let cff = face.table(Tag::from_bytes(b"CFF ")).is_some();
    let cff2 = face.table(Tag::from_bytes(b"CFF2")).is_some();
    if face.table(Tag::from_bytes(b"fvar")).is_some() {
        return Err(
            "Variable face identity/format needs unestablished native design-vector metadata",
        );
    }
    let format = match (glyf, cff, cff2) {
        (true, false, false) => FontFormat::TrueType,
        (false, true, false) => FontFormat::Cff,
        _ => {
            return Err("Unknown or conflicting embedded outline format; native font type omitted");
        }
    };
    Ok((expected_name.to_owned(), format))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Directory/name-table fixtures exercise the maintained parser, not a
    // production bespoke font parser. Independent native proof is separate.
    fn bytes(outline: &[u8; 4], postscript: &str) -> Vec<u8> {
        let name = postscript
            .encode_utf16()
            .flat_map(u16::to_be_bytes)
            .collect::<Vec<_>>();
        let mut names = vec![0, 0, 0, 1, 0, 18, 0, 3, 0, 1, 4, 9, 0, 6];
        names.extend(u16::try_from(name.len()).unwrap().to_be_bytes());
        names.extend([0, 0]);
        names.extend(name);
        let mut bytes = vec![0, 1, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0];
        let mut tables = vec![(*outline, vec![0]), (*b"name", names)];
        tables.sort_by_key(|(tag, _)| *tag);
        let mut offset = 44u32;
        for (tag, data) in &tables {
            bytes.extend(tag);
            bytes.extend(0u32.to_be_bytes());
            bytes.extend(offset.to_be_bytes());
            bytes.extend(u32::try_from(data.len()).unwrap().to_be_bytes());
            offset += u32::try_from(data.len()).unwrap();
        }
        for (_, data) in tables {
            bytes.extend(data);
        }
        bytes
    }

    fn macintosh_bytes(font: &[u8], encoding: u16) -> Vec<u8> {
        let mut data = bytes(b"glyf", "placeholder");
        let offset = u32::from_be_bytes(data[36..40].try_into().unwrap()) as usize;
        let mut names = vec![0, 0, 0, 1, 0, 18, 0, 1];
        names.extend(encoding.to_be_bytes());
        names.extend([0, 0, 0, 6]);
        names.extend(u16::try_from(font.len()).unwrap().to_be_bytes());
        names.extend([0, 0]);
        names.extend(font);
        data[40..44].copy_from_slice(&u32::try_from(names.len()).unwrap().to_be_bytes());
        data.truncate(offset);
        data.extend(names);
        data
    }

    #[test]
    fn macintosh_roman_ascii_font_identity_is_byte_backed() {
        assert_eq!(
            inspect_face(&macintosh_bytes(b"Menlo-Regular", 0), 0, "Menlo-Regular"),
            Ok(("Menlo-Regular".into(), FontFormat::TrueType))
        );
        assert!(inspect_face(&macintosh_bytes(b"Menlo-Regular", 1), 0, "Menlo-Regular").is_err());
        assert!(inspect_face(&macintosh_bytes(b"Menlo-\x80", 0), 0, "Menlo-Regular").is_err());
        assert!(inspect_face(&macintosh_bytes(b"Menlo-Regular", 0), 0, "Other-Regular").is_err());
    }

    #[test]
    fn macintosh_roman_ascii_registry_inference_preserves_physical_face() {
        let mut data =
            include_bytes!("../../../../tests/references/premiere/fonts/Arial-BoldMT.ttf").to_vec();
        let entries: [(u16, &[u8]); 4] = [
            (name_id::FAMILY, b"Mac Family"),
            (name_id::SUBFAMILY, b"Regular"),
            (name_id::FULL_NAME, b"Mac Family Regular"),
            (name_id::POST_SCRIPT_NAME, b"MacFamily-Regular"),
        ];
        let mut names = vec![0, 0, 0, 4, 0, 54];
        let mut strings = Vec::<u8>::new();
        for (id, value) in entries {
            names.extend([0, 1, 0, 0, 0, 0]);
            names.extend(id.to_be_bytes());
            names.extend(u16::try_from(value.len()).unwrap().to_be_bytes());
            names.extend(u16::try_from(strings.len()).unwrap().to_be_bytes());
            strings.extend(value);
        }
        names.extend(strings);
        let count = u16::from_be_bytes(data[4..6].try_into().unwrap()) as usize;
        let header = (0..count)
            .map(|i| 12 + i * 16)
            .find(|&i| &data[i..i + 4] == b"name")
            .unwrap();
        let offset = u32::try_from(data.len()).unwrap();
        data[header + 8..header + 12].copy_from_slice(&offset.to_be_bytes());
        data[header + 12..header + 16]
            .copy_from_slice(&u32::try_from(names.len()).unwrap().to_be_bytes());
        data.extend(names);
        let fonts = ArchiveFonts {
            faces: inferred_faces(&data),
        };
        assert_eq!(
            fonts.resolve("Mac Family", "Regular"),
            Ok(("MacFamily-Regular", FontFormat::TrueType))
        );
        assert!(fonts.resolve("Arial", "Bold").is_err());
    }

    #[test]
    fn archive_font_format_comes_from_actual_tables_not_name_or_extension() {
        assert_eq!(
            inspect_face(&bytes(b"glyf", "Face"), 0, "Face"),
            Ok(("Face".into(), FontFormat::TrueType))
        );
        assert_eq!(
            inspect_face(&bytes(b"CFF ", "Face"), 0, "Face"),
            Ok(("Face".into(), FontFormat::Cff))
        );
        assert!(inspect_face(&bytes(b"CFF2", "Face"), 0, "Face").is_err());
        assert!(inspect_face(&bytes(b"glyf", "Face"), 0, "Wrong").is_err());
        assert!(inspect_face(&bytes(b"glyf", "Face"), 1, "Face").is_err());
    }

    fn face(family: &str, style: &str, ps: &str) -> ArchiveFace {
        let metadata = serde_json::from_value(serde_json::json!({
            "familyName": family, "styleName": style, "postscriptName": ps,
            "fullName": format!("{family} {style}"), "weight": 400, "width": 5
        }))
        .unwrap();
        ArchiveFace {
            metadata,
            identity: inspect_face(&bytes(b"glyf", ps), 0, ps),
            bytes: bytes(b"glyf", ps).into(),
        }
    }

    #[test]
    fn archive_metadata_resolves_dashless_identity_and_rejects_ambiguity() {
        let mut fonts = ArchiveFonts {
            faces: vec![face("Family", "Regular", "PhysicalPS")],
        };
        assert_eq!(
            fonts.resolve("Family", "Regular"),
            Ok(("PhysicalPS", FontFormat::TrueType))
        );
        assert_eq!(
            fonts.resolve("PhysicalPS", ""),
            Ok(("PhysicalPS", FontFormat::TrueType))
        );
        assert!(fonts.resolve("Other", "Regular").is_err());
        fonts.faces.push(face("Family", "Regular", "OtherPS"));
        assert!(
            fonts
                .resolve("Family", "Regular")
                .unwrap_err()
                .contains("Ambiguous")
        );
        fonts.faces.truncate(1);
        fonts.faces[0].identity = Err("bad face");
        assert_eq!(fonts.resolve("Family", "Regular"), Err("bad face"));
    }

    #[test]
    fn real_font_fixture_binds_metadata_to_verified_face_name() {
        let bytes = include_bytes!("../../../../tests/references/premiere/fonts/Arial-BoldMT.ttf");
        assert_eq!(
            inspect_face(bytes, 0, "Arial-BoldMT"),
            Ok(("Arial-BoldMT".into(), FontFormat::TrueType))
        );
        assert!(inspect_face(bytes, 0, "Arial-Regular").is_err());
    }

    #[test]
    fn absent_registry_uses_only_actual_embedded_face_names() {
        let bytes = include_bytes!("../../../../tests/references/premiere/fonts/Arial-BoldMT.ttf");
        let fonts = ArchiveFonts {
            faces: inferred_faces(bytes),
        };
        assert_eq!(
            fonts.resolve("Arial", "Bold"),
            Ok(("Arial-BoldMT", FontFormat::TrueType))
        );
        assert_eq!(
            fonts.resolve("Arial-BoldMT", ""),
            Ok(("Arial-BoldMT", FontFormat::TrueType))
        );
        assert!(fonts.resolve("NotArial", "Bold").is_err());
        assert!(inferred_faces(b"not a font").is_empty());
    }

    #[test]
    fn embedded_font_bounds_retain_a_mixed_motion_blurred_storyboard_card() {
        let fonts = ArchiveFonts {
            faces: inferred_faces(include_bytes!(
                "../../../../tests/references/premiere/fonts/Arial-BoldMT.ttf"
            )),
        };
        let fixture = serde_json::json!({
            "$schema": "https://jerboa.dev/schemas/fx-composition/editable/v1/document.schema.json",
            "formatVersion": 1,
            "dimensions": {"width": 1920, "height": 1080},
            "duration": 7.0,
            "composition": {"id": "main", "name": "Embedded font card", "dynamics": {"entries": [{
                "target": {"kind": "layer", "layerId": 11200, "propertyType": "opacity"},
                "animator": {"type": "keyframes", "enabled": true, "keyframes": [
                    {"id": "label-opacity-start", "layerTime": 0,
                     "value": {"type": "float", "value": 0}, "easing": {"type": "linear"}},
                    {"id": "label-opacity-end", "layerTime": 2000,
                     "value": {"type": "float", "value": 100}, "easing": {"type": "linear"}}
                ]}
            }]}, "layers": [{
                "type": "Group", "id": 11000, "name": "Storyboard card",
                "playback": {"type": "windowed",
                    "inputRange": {"start": 0, "duration": 7000},
                    "mapping": {"type": "linear",
                        "input": {"start": 0, "duration": 7000},
                        "output": {"start": 0, "duration": 7000}}, "inputOffsetMs": 0},
                "motionBlur": true,
                "transform": {"position": [960, 550], "anchorPoint": [960, 540],
                              "scale": [100, 100], "rotation": 0, "opacity": 50},
                "layers": [{
                    "type": "Text", "id": 11200, "name": "Editable view label",
                    "activeRange": {"start": 0, "duration": 7000},
                    "transform": {"position": [1810, 116], "anchorPoint": [0, 0],
                                  "scale": [100, 100], "rotation": 0, "opacity": 100},
                    "sourceText": {"text": "02 / Three-quarter", "fontFamily": "Arial",
                                   "fontStyle": "Bold", "fontSize": 42,
                                   "fillColor": [1, 1, 1, 1],
                                   "tracking": 20, "justification": "right"}
                }, {
                    "type": "Rect", "id": 11100, "name": "Card frame",
                    "activeRange": {"start": 0, "duration": 7000},
                    "transform": {"position": [0, 0], "anchorPoint": [0, 0],
                                  "scale": [100, 100], "rotation": 0, "opacity": 100},
                    "rect": {"size": [1920, 1080], "fillColor": [1, 1, 1, 1]}
                }]
            }]}
        });
        let document =
            fx_schema::EditableFxCompositionDocument::from_json_value(fixture.clone()).unwrap();
        let fx_schema::LayerData::Group(group) = document.composition().layers()[0].data() else {
            panic!("Group fixture");
        };
        fonts
            .bounds_geometry(group, &crate::export_document::AnimationIndex::new(&[]))
            .unwrap();
        let output = crate::export_document::to_aep_with_document_views_and_media_and_fps(
            crate::export_document::ExportDocumentViews::unchanged(&document).with_fonts(&fonts),
            &Default::default(),
            30.0,
        )
        .unwrap();
        assert!(
            output.omitted_layer_ids.is_empty(),
            "{:?}",
            output.diagnostics
        );
        let native = crate::structure::read_project(&output.bytes).unwrap();
        let imported =
            crate::structure_document::to_structural_fx_document(&native, Some(1)).unwrap();
        fn contains_text(layers: &[fx_schema::Layer]) -> bool {
            layers.iter().any(|layer| match layer.data() {
                fx_schema::LayerData::Text(text) => text.source_text.text == "02 / Three-quarter",
                fx_schema::LayerData::Group(group) => contains_text(&group.layers),
                _ => false,
            })
        }
        assert!(contains_text(imported.document.composition().layers()));

        // P037's Group-owned native Drop Shadow must not disable the same
        // physical-font enclosure. Its native Size is twice FX blurRadius.
        let mut shadow_fixture = fixture.clone();
        shadow_fixture["composition"]["layers"][0]["effects"] = serde_json::json!([{
            "id": 11810, "enabled": true, "effect": {
                "type": "dropShadow", "enabled": true,
                "color": [0.0, 0.0, 0.0, 0.6], "offset": [0.0, 28.0],
                "blurRadius": 65.0, "spreadRadius": 0.0
            }
        }]);
        shadow_fixture["composition"]["layers"][0]["transform"]["rotationX"] = 20.0.into();
        let shadow_document =
            fx_schema::EditableFxCompositionDocument::from_json_value(shadow_fixture.clone())
                .unwrap();
        let shadow_output = crate::export_document::to_aep_with_document_views_and_media_and_fps(
            crate::export_document::ExportDocumentViews::unchanged(&shadow_document)
                .with_fonts(&fonts),
            &Default::default(),
            30.0,
        )
        .unwrap();
        assert!(
            shadow_output.omitted_layer_ids.is_empty(),
            "{:?}",
            shadow_output.diagnostics
        );
        let shadow_native = crate::structure::read_project(&shadow_output.bytes).unwrap();
        let shadow_imported =
            crate::structure_document::to_structural_fx_document(&shadow_native, Some(1)).unwrap();
        assert!(contains_text(
            shadow_imported.document.composition().layers()
        ));

        let mut animated_shadow = shadow_fixture.clone();
        let mut effect_entry = animated_shadow["composition"]["dynamics"]["entries"][0].clone();
        effect_entry["animator"]["keyframes"][0]["id"] = "shadow-blur-start".into();
        effect_entry["animator"]["keyframes"][1]["id"] = "shadow-blur-end".into();
        effect_entry["target"] = serde_json::to_value(fx_schema::PropertyTarget::effect_param(
            fx_schema::EffectId::new(11810),
            "blurRadius",
        ))
        .unwrap();
        animated_shadow["composition"]["dynamics"]["entries"]
            .as_array_mut()
            .unwrap()
            .push(effect_entry);
        let animated_shadow =
            fx_schema::EditableFxCompositionDocument::from_json_value(animated_shadow).unwrap();
        let output = crate::export_document::to_aep_with_document_views_and_media_and_fps(
            crate::export_document::ExportDocumentViews::unchanged(&animated_shadow)
                .with_fonts(&fonts),
            &Default::default(),
            30.0,
        )
        .unwrap();
        assert!(
            output
                .omitted_layer_ids
                .contains(&fx_schema::LayerId::new(11000))
        );

        // The exception is not permission to admit unknown spatial effects.
        shadow_fixture["composition"]["layers"][0]["effects"][0]["effect"] =
            serde_json::json!({"type": "gaussianBlur", "blurriness": 65.0});
        let unsupported =
            fx_schema::EditableFxCompositionDocument::from_json_value(shadow_fixture).unwrap();
        let output = crate::export_document::to_aep_with_document_views_and_media_and_fps(
            crate::export_document::ExportDocumentViews::unchanged(&unsupported).with_fonts(&fonts),
            &Default::default(),
            30.0,
        )
        .unwrap();
        assert!(
            output
                .omitted_layer_ids
                .contains(&fx_schema::LayerId::new(11000))
        );

        // A nested, short occurrence must use its checked source-local geometry,
        // not a full-root or viewport-only exemption.
        let mut nested = fixture.clone();
        let card = &mut nested["composition"]["layers"][0];
        card["playback"]["inputRange"]["duration"] = 2250.into();
        card["playback"]["mapping"]["input"]["duration"] = 2250.into();
        card["playback"]["mapping"]["output"]["duration"] = 2250.into();
        let card = card.clone();
        nested["composition"]["layers"] = serde_json::json!([{
            "type": "Group", "id": 100, "name": "Outer mixed source",
            "playback": {"type": "windowed",
                "inputRange": {"start": 0, "duration": 7000},
                "mapping": {"type": "linear",
                    "input": {"start": 0, "duration": 7000},
                    "output": {"start": 0, "duration": 7000}}, "inputOffsetMs": 0},
            "motionBlur": true,
            "transform": {"position": [0, 0], "anchorPoint": [0, 0],
                          "scale": [100, 100], "rotation": 0, "opacity": 50},
            "layers": [card]
        }]);
        let nested = fx_schema::EditableFxCompositionDocument::from_json_value(nested).unwrap();
        let output = crate::export_document::to_aep_with_document_views_and_media_and_fps(
            crate::export_document::ExportDocumentViews::unchanged(&nested).with_fonts(&fonts),
            &Default::default(),
            30.0,
        )
        .unwrap();
        assert!(
            output.omitted_layer_ids.is_empty(),
            "{:?}",
            output.diagnostics
        );

        // Without the verified face, the original omission is still required.
        let output = crate::export_document::to_aep(&document).unwrap();
        assert!(
            output
                .omitted_layer_ids
                .contains(&fx_schema::LayerId::new(11000))
        );
        for unsupported in ["fontSize", "leading"] {
            let mut unsupported_fixture = fixture.clone();
            unsupported_fixture["composition"]["dynamics"]["entries"][0]["target"]["propertyType"] =
                unsupported.into();
            let unsupported_document =
                fx_schema::EditableFxCompositionDocument::from_json_value(unsupported_fixture)
                    .unwrap();
            let output = crate::export_document::to_aep_with_document_views_and_media_and_fps(
                crate::export_document::ExportDocumentViews::unchanged(&unsupported_document)
                    .with_fonts(&fonts),
                &Default::default(),
                30.0,
            )
            .unwrap();
            assert!(
                output
                    .omitted_layer_ids
                    .contains(&fx_schema::LayerId::new(11000))
            );
        }
    }

    #[test]
    fn embedded_font_bounds_keep_whitespace_and_emitted_document_tracks() {
        let fonts = ArchiveFonts {
            faces: inferred_faces(include_bytes!(
                "../../../../tests/references/premiere/fonts/Arial-BoldMT.ttf"
            )),
        };
        let mut fixture = serde_json::json!({
            "$schema": "https://jerboa.dev/schemas/fx-composition/editable/v1/document.schema.json",
            "formatVersion": 1,
            "dimensions": {"width": 1920, "height": 1080},
            "duration": 2.0,
            "composition": {"id": "main", "name": "Whitespace and emitted Text", "dynamics": {"entries": []},
                "layers": [{"type": "Group", "id": 100, "name": "Text-only moving source",
                    "playback": {"type": "windowed", "inputRange": {"start": 0, "duration": 2000},
                        "mapping": {"type": "linear", "input": {"start": 0, "duration": 2000},
                            "output": {"start": 0, "duration": 2000}}, "inputOffsetMs": 0},
                    "motionBlur": true,
                    "transform": {"position": [300, 300], "anchorPoint": [0, 0],
                        "scale": [100, 100], "rotation": 0, "opacity": 50},
                    "layers": [{"type": "Text", "id": 4900, "name": "Mirage",
                        "activeRange": {"start": 0, "duration": 2000},
                        "transform": {"position": [0, 0], "anchorPoint": [0, 0],
                            "scale": [100, 100], "rotation": 0, "opacity": 100},
                        "sourceText": {"text": "Mirage", "fontFamily": "Arial", "fontStyle": "Bold",
                            "fontSize": 64, "fillColor": [1, 1, 1, 1], "tracking": 20}},
                        {"type": "Text", "id": 896, "name": "Editable space",
                        "activeRange": {"start": 0, "duration": 2000},
                        "transform": {"position": [10000, 10000], "anchorPoint": [0, 0],
                            "scale": [100, 100], "rotation": 0, "opacity": 100},
                        "sourceText": {"text": " ", "fontFamily": "Arial", "fontStyle": "Bold",
                            "fontSize": 64, "fillColor": [1, 1, 1, 1]}}]}]}
        });
        let whitespace_document =
            fx_schema::EditableFxCompositionDocument::from_json_value(fixture.clone()).unwrap();
        let fx_schema::LayerData::Group(whitespace_group) =
            whitespace_document.composition().layers()[0].data()
        else {
            panic!("Whitespace Group fixture");
        };
        let whitespace_projection = fonts
            .bounds_geometry(
                whitespace_group,
                &crate::export_document::AnimationIndex::new(&[]),
            )
            .unwrap();
        assert!(
            matches!(whitespace_projection.layers[1].data(), fx_schema::LayerData::Rect(rect) if rect.is_hidden)
        );
        let track = |property: &str, values: [f64; 2]| {
            serde_json::json!({
                "target": {"kind": "layer", "layerId": 4900, "propertyType": property},
                "animator": {"type": "keyframes", "enabled": true, "keyframes": [
                    {"id": format!("{property}-start"), "layerTime": 0,
                        "value": {"type": "float", "value": values[0]}, "easing": {"type": "linear"}},
                    {"id": format!("{property}-end"), "layerTime": 2000,
                        "value": {"type": "float", "value": values[1]}, "easing": {"type": "linear"}}
                ]}
            })
        };
        fixture["composition"]["dynamics"]["entries"] = serde_json::json!([
            track("tracking", [20.0, 200.0]),
            track("positionY", [0.0, 60.0]),
            track("rotation", [0.0, 10.0])
        ]);
        let document =
            fx_schema::EditableFxCompositionDocument::from_json_value(fixture.clone()).unwrap();
        let entries = document.composition().dynamics().entries();
        let dynamics = crate::export_document::AnimationIndex::new(entries);
        let fx_schema::LayerData::Group(group) = document.composition().layers()[0].data() else {
            panic!("Group fixture");
        };
        let projected = fonts.bounds_geometry(group, &dynamics).unwrap();
        assert!(
            matches!(projected.layers[1].data(), fx_schema::LayerData::Rect(rect) if rect.is_hidden)
        );
        let timeline = crate::export_document::text::bounds_documents(
            match group.layers[0].data() {
                fx_schema::LayerData::Text(text) => text,
                _ => panic!("Text"),
            },
            &dynamics,
            &fonts,
        )
        .unwrap();
        assert!(!timeline.keyed);
        assert_eq!(timeline.keys[0].document.tracking, 20.0);
        let output = crate::export_document::to_aep_with_document_views_and_media_and_fps(
            crate::export_document::ExportDocumentViews::unchanged(&document).with_fonts(&fonts),
            &Default::default(),
            30.0,
        )
        .unwrap();
        assert!(
            output.omitted_layer_ids.is_empty(),
            "{:?}",
            output.diagnostics
        );
        assert!(output.diagnostics.iter().any(|message| {
            message
                .message
                .contains("Tracking uses continuous interpolation")
        }));
        let native = crate::structure::read_project(&output.bytes).unwrap();
        let imported =
            crate::structure_document::to_structural_fx_document(&native, Some(1)).unwrap();
        fn texts(layers: &[fx_schema::Layer], output: &mut Vec<String>) {
            for layer in layers {
                match layer.data() {
                    fx_schema::LayerData::Text(text) => output.push(text.source_text.text.clone()),
                    fx_schema::LayerData::Group(group) => texts(&group.layers, output),
                    _ => {}
                }
            }
        }
        let mut retained = Vec::new();
        texts(imported.document.composition().layers(), &mut retained);
        assert!(retained.contains(&"Mirage".to_owned()));
        assert!(retained.contains(&" ".to_owned()));

        // A CustomShader effect is dropped; its Text owner remains. A mapped
        // raster effect must not use this plain-point certificate as an
        // effect-expansion enclosure.
        let mut shader_fixture = fixture.clone();
        shader_fixture["composition"]["layers"][0]["layers"][0]["effects"] = serde_json::json!([
            {"id": 7, "enabled": true, "effect": {"type": "customShader", "name": "Never native",
                "wgsl": "not interpreted", "params": []}}
        ]);
        let shader =
            fx_schema::EditableFxCompositionDocument::from_json_value(shader_fixture.clone())
                .unwrap();
        let output = crate::export_document::to_aep_with_document_views_and_media_and_fps(
            crate::export_document::ExportDocumentViews::unchanged(&shader).with_fonts(&fonts),
            &Default::default(),
            30.0,
        )
        .unwrap();
        assert!(
            output.omitted_layer_ids.is_empty(),
            "{:?}",
            output.diagnostics
        );
        assert!(output.diagnostics.iter().any(|diagnostic| {
            diagnostic.layer_id == Some(fx_schema::LayerId::new(4900))
                && diagnostic
                    .message
                    .contains("CustomShader \"Never native\" dropped")
        }));
        shader_fixture["composition"]["layers"][0]["layers"][0]["effects"] = serde_json::json!([
            {"type": "gaussianBlur", "blurriness": 12.0}
        ]);
        let raster =
            fx_schema::EditableFxCompositionDocument::from_json_value(shader_fixture).unwrap();
        let fx_schema::LayerData::Group(raster_group) = raster.composition().layers()[0].data()
        else {
            panic!("Group")
        };
        assert!(
            fonts
                .bounds_geometry(
                    raster_group,
                    &crate::export_document::AnimationIndex::new(
                        raster.composition().dynamics().entries()
                    )
                )
                .is_err()
        );

        // Hold Tracking is already supported natively: its outline union must
        // include the actual second document, not merely the typed static base.
        fixture["composition"]["dynamics"]["entries"][0]["animator"]["keyframes"][1]["easing"]["type"] =
            "hold".into();
        let held = fx_schema::EditableFxCompositionDocument::from_json_value(fixture).unwrap();
        let fx_schema::LayerData::Group(held_group) = held.composition().layers()[0].data() else {
            panic!("Group")
        };
        let held_projection = fonts
            .bounds_geometry(
                held_group,
                &crate::export_document::AnimationIndex::new(
                    held.composition().dynamics().entries(),
                ),
            )
            .unwrap();
        let fx_schema::LayerData::Rect(base) = projected.layers[0].data() else {
            panic!("Bounds")
        };
        let fx_schema::LayerData::Rect(held) = held_projection.layers[0].data() else {
            panic!("Bounds")
        };
        assert!(held.rect.size[0] > base.rect.size[0]);
    }

    #[test]
    fn collection_face_index_is_not_the_first_face() {
        let mut collection = b"ttcf\0\x01\0\0\0\0\0\x02\0\0\0\x14\0\0\0\x41".to_vec();
        let first = bytes(b"glyf", "A");
        let second_offset = 20 + first.len();
        collection[16..20].copy_from_slice(&u32::try_from(second_offset).unwrap().to_be_bytes());
        for (mut face, offset) in [(first, 20), (bytes(b"CFF ", "B"), second_offset)] {
            for at in [20, 36] {
                let original = u32::from_be_bytes(face[at..at + 4].try_into().unwrap());
                face[at..at + 4]
                    .copy_from_slice(&(original + u32::try_from(offset).unwrap()).to_be_bytes());
            }
            collection.extend(face);
        }
        assert_eq!(
            inspect_face(&collection, 1, "B"),
            Ok(("B".into(), FontFormat::Cff))
        );
        assert!(inspect_face(&collection, 0, "B").is_err());
    }
}
