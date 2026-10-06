//! Bounded static lowering of the legacy Basic Text generator into editable Text.
//!
//! Only the packed profile observed in an unchanged native source is admitted.
//! Unknown layouts/alignment state are not treated as defaults.

use std::sync::Arc;

use fx_schema::{GroupLayer, LayerData, TextDocument, TextLayer};

use crate::{effects::native::DecodedEffect, properties, structure::Layer};

pub(super) const MATCH_NAME: &str = "ADBE Basic Text2";

pub(super) struct Generator {
    text: String,
    font: String,
    position: [f64; 2],
    size: f64,
    color: [f64; 4],
    plane: [u16; 2],
}

pub(super) fn recognize(
    layer: &Layer,
    effects: &[DecodedEffect],
    plane: [u16; 2],
) -> Result<Option<Generator>, String> {
    let candidates: Vec<_> = effects
        .iter()
        .filter(|effect| effect.match_name == MATCH_NAME)
        .collect();
    let [effect] = candidates.as_slice() else {
        return if candidates.is_empty() {
            Ok(None)
        } else {
            Err("multiple Basic Text generators".into())
        };
    };
    if !effect.enabled || !layer.record.flags().effects_active {
        return Ok(None);
    }
    if effect.index != 1
        || plane.contains(&0)
        || layer.record.flags().adjustment_layer
        || layer.record.layer_type() != 4
    {
        return Err(
            "requires the first effect on an ordinary composition-sized Shape source plane".into(),
        );
    }
    let descriptor = descriptor(layer)?;
    let controls =
        properties::unique_list(descriptor, *b"tdgp").map_err(|error| error.to_string())?;
    let explicit = properties::runs(controls).map_err(|error| error.to_string())?;
    let mut names = std::collections::HashSet::new();
    if explicit.iter().any(|(name, _)| !names.insert(*name)) {
        return Err("duplicate explicit Basic Text controls".into());
    }
    // Plugin Composite On Original does not describe AE's per-effect opacity
    // or mask references. Only the empty built-in compositing profile is known.
    for (_, run) in explicit
        .iter()
        .filter(|(name, _)| *name == "ADBE Effect Built In Params")
    {
        let options = properties::runs(
            properties::unique_list(run, *b"tdgp").map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        if options.iter().any(|(name, _)| *name != "ADBE Group End") {
            return Err("nonempty effect compositing options".into());
        }
    }
    let declarations =
        properties::unique_list(descriptor, *b"parT").map_err(|error| error.to_string())?;
    let declarations = properties::runs(declarations).map_err(|error| error.to_string())?;
    let value = |suffix: &str| -> Result<Vec<f64>, String> {
        let name = format!("{MATCH_NAME}-{suffix}");
        if explicit.iter().any(|(id, _)| *id == name) || !matches!(suffix, "0002" | "0007") {
            return static_values(effect, suffix);
        }
        let declared: Vec<_> = declarations.iter().filter(|(id, _)| *id == name).collect();
        let [(_, declaration)] = declared.as_slice() else {
            return Err(format!("{name}: unique declaration required"));
        };
        let bytes = properties::data(declaration, *b"pard").map_err(|error| error.to_string())?;
        declared_default(bytes, suffix, plane)
    };
    let scalar = |suffix: &str| -> Result<f64, String> {
        let values = value(suffix)?;
        match values.as_slice() {
            [value] => Ok(*value),
            _ => Err(format!("{suffix}: scalar required")),
        }
    };
    if scalar("0004")? != 1.0 || scalar("0007")? != 1.0 {
        return Err("only source-declared Composite On Original and fill-only Display Options are supported".into());
    }
    if scalar("0005")? != 0.0 {
        return Err("nonzero Basic Text tracking has unverified units".into());
    }
    let size = scalar("0001")?;
    if size <= 0.0 {
        return Err("positive Size required".into());
    }
    let position = value("0002")?;
    let [x, y] = position.as_slice() else {
        return Err("two-dimensional Position required".into());
    };
    let color = static_values(effect, "0003")?;
    let [red, green, blue, alpha] = color.as_slice() else {
        return Err("decoded RGBA Fill Color required".into());
    };
    if color
        .iter()
        .any(|component| !(0.0..=1.0).contains(component))
    {
        return Err("normalized Fill Color required".into());
    }
    let payload = properties::data(descriptor, *b"sdat").map_err(|error| error.to_string())?;
    let (text, font) = packed_text(payload)?;
    Ok(Some(Generator {
        text,
        font,
        position: [*x, *y],
        size,
        color: [*red, *green, *blue, *alpha],
        plane,
    }))
}

fn descriptor(layer: &Layer) -> Result<&[crate::rifx::Chunk], String> {
    let root = properties::root_runs(&layer.content).map_err(|error| error.to_string())?;
    for (_, masks) in root.iter().filter(|(name, _)| *name == "ADBE Mask Parade") {
        let masks = properties::unique_list(masks, *b"tdgp").map_err(|error| error.to_string())?;
        if !properties::runs(masks)
            .map_err(|error| error.to_string())?
            .is_empty()
        {
            return Err("authored masks before Basic Text are not supported".into());
        }
    }
    let parade: Vec<_> = root
        .iter()
        .filter(|(name, _)| *name == "ADBE Effect Parade")
        .collect();
    let [(_, parade)] = parade.as_slice() else {
        return Err("unique Effect Parade required".into());
    };
    let groups = properties::unique_list(parade, *b"tdgp").map_err(|error| error.to_string())?;
    let runs = properties::runs(groups).map_err(|error| error.to_string())?;
    let instances: Vec<_> = runs
        .iter()
        .filter(|(name, _)| *name == MATCH_NAME)
        .collect();
    let [(_, instance)] = instances.as_slice() else {
        return Err("unique Basic Text instance required".into());
    };
    properties::unique_list(instance, *b"sspc").map_err(|error| error.to_string())
}

fn declared_default(bytes: &[u8], suffix: &str, plane: [u16; 2]) -> Result<Vec<f64>, String> {
    if bytes.len() != 148 {
        return Err("invalid native parameter declaration length".into());
    }
    let kind = u32::from_be_bytes(
        bytes[12..16]
            .try_into()
            .expect("checked declaration length"),
    );
    // PF_PointDef stores its two 16:16 percentage defaults after current x/y
    // and restrict_bounds. PF_PopupDef has current value, num_choices, dephault.
    // These are declarations, not an opaque guessed generator default.
    match (suffix, kind) {
        ("0002", 6) => Ok([68, 72]
            .into_iter()
            .zip(plane)
            .map(|(offset, extent)| {
                let raw = i32::from_be_bytes(
                    bytes[offset..offset + 4]
                        .try_into()
                        .expect("checked declaration length"),
                );
                f64::from(raw) / 65_536.0 / 100.0 * f64::from(extent)
            })
            .collect()),
        ("0007", 7) if u16::from_be_bytes([bytes[60], bytes[61]]) == 4 => {
            Ok(vec![f64::from(u16::from_be_bytes([bytes[62], bytes[63]]))])
        }
        _ => Err("unrecognized Basic Text point/popup declaration".into()),
    }
}

fn static_values(effect: &DecodedEffect, suffix: &str) -> Result<Vec<f64>, String> {
    let name = format!("{MATCH_NAME}-{suffix}");
    let parameters: Vec<_> = effect
        .parameters
        .iter()
        .filter(|parameter| parameter.match_name == name)
        .collect();
    let [parameter] = parameters.as_slice() else {
        return Err(format!("{name}: unique source control required"));
    };
    let numeric = parameter
        .numeric
        .as_ref()
        .map_err(|error| format!("{name}: {error}"))?;
    if numeric.animated
        || numeric.expression_enabled
        || numeric.dimensions_separated
        || numeric.values.is_empty()
        || numeric.values.iter().any(|value| !value.is_finite())
    {
        return Err(format!("{name}: static finite source value required"));
    }
    Ok(numeric.values.clone())
}

fn packed_text(payload: &[u8]) -> Result<(String, String), String> {
    // Native source d2451f4eadd9, comp186/layer224: fixed C-string slots and
    // observed centered single-line profile. No alternate flags are inferred.
    if payload.len() != 1804
        || payload[..8] != [0, 255, 43, 237, 4, 0, 0, 1]
        || payload[1800..] != [0, 0, 1, 0]
    {
        return Err("unrecognized packed text/alignment profile".into());
    }
    let string = |slot: &[u8]| -> Result<String, String> {
        let end = slot
            .iter()
            .position(|byte| *byte == 0)
            .ok_or("unterminated packed text string")?;
        let value = std::str::from_utf8(&slot[..end]).map_err(|_| "non-UTF8 packed text string")?;
        if value.is_empty() || value.chars().any(char::is_control) {
            return Err("only nonempty single-line packed text strings are supported".into());
        }
        Ok(value.to_owned())
    };
    let text = string(&payload[8..1033])?;
    let _family = string(&payload[1033..1289])?;
    let _display_style = string(&payload[1289..1545])?;
    let font = string(&payload[1545..1800])?;
    Ok((text, font))
}

impl Generator {
    pub(super) fn into_layer(self, id: fx_schema::LayerId, owner: &GroupLayer) -> LayerData {
        let mut transform = owner.transform;
        // Shape/Text effect Position is measured from the composition-sized
        // effect plane's top-left, while their editable source origin is centered.
        transform.position = fx_schema::Position::TwoD([
            self.position[0] - f64::from(self.plane[0]) / 2.0,
            self.position[1] - f64::from(self.plane[1]) / 2.0 + self.size / 2.0,
        ]);
        LayerData::Text(TextLayer {
            id, name: "Basic Text (editable generator)".into(),
            description: "Legacy Basic Text replaced with editable single-line centered Text; vertical placement uses an em-box baseline approximation, not native glyph metrics".into(),
            is_hidden: false, parent: Some(owner.id), blend_mode: Default::default(),
            track_matte: None, masks: Vec::new(), active_range: fx_schema::TimeRangeProperty::new(fx_schema::Time::ZERO, fx_schema::Duration::from_secs(super::MAX_TIME_SECS)),
            effects: Vec::new(), motion_blur: false, transform,
            source_text: TextDocument {
                // The packed face is authoritative PostScript identity, not
                // family + display style. Empty style preserves that namespace.
                text: self.text, font_family: Arc::from(self.font), font_style: Arc::from(""),
                font_size: fx_schema::PositiveProperty::new(self.size).expect("recognized positive Size"),
                font_variations: None, apply_fill: true,
                fill_color: self.color,
                apply_stroke: false, stroke_color: None, stroke_width: Default::default(), stroke_over_fill: false,
                justification: fx_schema::Justification::Center, tracking: Default::default(), leading: None,
                baseline_shift: Default::default(), box_text: false, scale_box_text_with_transform: false,
                box_size: None, box_position: None, box_first_baseline: None,
                all_caps: false, underline: false, strikethrough: false, vertical_align: None,
            },
            animators: Vec::new(), path_options: None, anchor_options: Default::default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload() -> Vec<u8> {
        // Sanitized source-layout reconstruction, not proprietary source bytes.
        let mut bytes = vec![0; 1804];
        bytes[..8].copy_from_slice(&[0, 255, 43, 237, 4, 0, 0, 1]);
        bytes[1800..].copy_from_slice(&[0, 0, 1, 0]);
        for (offset, value) in [
            (8, "Editable"),
            (1033, "Arial"),
            (1289, "Bold"),
            (1545, "Arial-BoldMT"),
        ] {
            bytes[offset..offset + value.len()].copy_from_slice(value.as_bytes());
        }
        bytes
    }

    #[test]
    fn source_observed_basic_text_profile_retains_text_and_explicit_font() {
        assert_eq!(
            packed_text(&payload()).unwrap(),
            ("Editable".into(), "Arial-BoldMT".into())
        );
    }

    #[test]
    fn basic_text_packed_postscript_identity_is_independent_of_display_labels() {
        for (family, style, face) in [
            ("Example", "Bold", "Example-BoldMT"),
            ("Neighbor Family", "Semi Bold", "NeighborPS"),
            ("Other", "Regular", "Other-Regular"),
        ] {
            let mut bytes = payload();
            for (start, end, value) in [
                (1033, 1289, family),
                (1289, 1545, style),
                (1545, 1800, face),
            ] {
                bytes[start..end].fill(0);
                bytes[start..start + value.len()].copy_from_slice(value.as_bytes());
            }
            assert_eq!(
                packed_text(&bytes).unwrap(),
                ("Editable".into(), face.into())
            );
        }
    }

    #[test]
    fn basic_text_declaration_defaults_use_sdk_point_percentages_and_popup_default() {
        let mut declaration = vec![0; 148];
        declaration[12..16].copy_from_slice(&6u32.to_be_bytes());
        declaration[68..72].copy_from_slice(&(50i32 * 65_536).to_be_bytes());
        declaration[72..76].copy_from_slice(&(25i32 * 65_536).to_be_bytes());
        assert_eq!(
            declared_default(&declaration, "0002", [3840, 2160]).unwrap(),
            [1920.0, 540.0]
        );
        declaration[12..16].copy_from_slice(&7u32.to_be_bytes());
        declaration[60..62].copy_from_slice(&4u16.to_be_bytes());
        declaration[62..64].copy_from_slice(&1u16.to_be_bytes());
        assert_eq!(
            declared_default(&declaration, "0007", [3840, 2160]).unwrap(),
            [1.0]
        );
        declaration[60..62].copy_from_slice(&3u16.to_be_bytes());
        assert!(declared_default(&declaration, "0007", [3840, 2160]).is_err());
        assert!(declared_default(&declaration[..147], "0007", [3840, 2160]).is_err());
    }

    #[test]
    fn basic_text_generator_is_editable_and_uses_source_plane_not_occurrence_transform() {
        let owner = super::super::group(
            1.into(),
            "Source".into(),
            None,
            fx_schema::TimeRangeProperty::new(
                fx_schema::Time::ZERO,
                fx_schema::Duration::from_secs(2.0),
            ),
        );
        let generator = Generator {
            text: "Editable".into(),
            font: "Arial-BoldMT".into(),
            position: [1920.0, 1080.0],
            size: 254.0,
            color: [220.0 / 255.0, 220.0 / 255.0, 220.0 / 255.0, 1.0],
            plane: [3840, 2160],
        };
        let LayerData::Text(text) = generator.into_layer(2.into(), &owner) else {
            panic!("editable Text required")
        };
        assert_eq!(text.source_text.text, "Editable");
        assert_eq!(&*text.source_text.font_family, "Arial-BoldMT");
        assert_eq!(&*text.source_text.font_style, "");
        assert_eq!(
            text.source_text.fill_color,
            [220.0 / 255.0, 220.0 / 255.0, 220.0 / 255.0, 1.0]
        );
        assert_eq!(
            text.source_text.justification,
            fx_schema::Justification::Center
        );
        assert_eq!(
            text.transform.position,
            fx_schema::Position::TwoD([0.0, 127.0])
        );
        assert_eq!(text.parent, Some(1.into()));
        assert!(text.effects.is_empty());
        assert!(text.animators.is_empty());
    }

    fn qualifying_project(
        cutout: Option<bool>,
    ) -> (crate::structure::StructuralProject, serde_json::Value) {
        use crate::{
            rifx::Chunk,
            structure::{ItemKind, read_project},
            structure_document::to_structural_fx_document,
        };

        fn name(value: &str) -> Chunk {
            let mut bytes = vec![0; 40];
            bytes[..value.len()].copy_from_slice(value.as_bytes());
            Chunk::data(*b"tdmn", bytes).unwrap()
        }
        fn control(suffix: &str, values: &[f64], kind: u8) -> Vec<Chunk> {
            let mut meta = vec![0; 124];
            meta[..2].copy_from_slice(&[0xdb, 0x99]);
            meta[2..4].copy_from_slice(&u16::try_from(values.len()).unwrap().to_be_bytes());
            meta[59] = kind;
            vec![
                name(&format!("{MATCH_NAME}-{suffix}")),
                Chunk::list(
                    *b"tdbs",
                    vec![
                        Chunk::data(*b"tdsb", vec![0, 0, 0, 1]).unwrap(),
                        Chunk::data(*b"tdb4", meta).unwrap(),
                        Chunk::data(
                            *b"cdat",
                            values
                                .iter()
                                .flat_map(|value| value.to_be_bytes())
                                .collect::<Vec<_>>(),
                        )
                        .unwrap(),
                    ],
                ),
            ]
        }
        let mut project = read_project(include_bytes!(
            "../../tests/fixtures/effects/static_point_controls.aep"
        ))
        .unwrap();
        let original = to_structural_fx_document(&project, Some(1))
            .unwrap()
            .document
            .to_json_value()
            .unwrap();
        let item = project.items.iter_mut().find(|item| item.id == 1).unwrap();
        let ItemKind::Composition(composition) = &mut item.kind else {
            panic!("native Shape fixture composition")
        };
        let owner = &mut composition.layers[0];
        assert_eq!(owner.record.layer_type(), 4);
        let root = owner
            .content
            .iter_mut()
            .find(|chunk| chunk.list_kind() == Some(*b"tdgp"))
            .unwrap()
            .children_mut()
            .unwrap();
        let start = root
            .iter()
            .position(|chunk| {
                chunk
                    .data_payload()
                    .is_some_and(|bytes| bytes.starts_with(b"ADBE Effect Parade"))
            })
            .unwrap();
        let parade = root[start + 1..]
            .iter_mut()
            .find(|chunk| chunk.list_kind() == Some(*b"tdgp"))
            .unwrap()
            .children_mut()
            .unwrap();
        let mut point = vec![0; 148];
        point[12..16].copy_from_slice(&6u32.to_be_bytes());
        point[68..72].copy_from_slice(&(50i32 * 65_536).to_be_bytes());
        point[72..76].copy_from_slice(&(50i32 * 65_536).to_be_bytes());
        let mut popup = vec![0; 148];
        popup[12..16].copy_from_slice(&7u32.to_be_bytes());
        popup[60..62].copy_from_slice(&4u16.to_be_bytes());
        popup[62..64].copy_from_slice(&1u16.to_be_bytes());
        let controls = [
            ("0001", vec![254.0], 0),
            ("0003", vec![255.0, 220.0, 220.0, 220.0], 1),
            ("0004", vec![1.0], 4),
            ("0005", vec![0.0], 0),
        ]
        .into_iter()
        .flat_map(|(suffix, values, kind)| control(suffix, &values, kind))
        .collect();
        // Synthetic observed Basic Text layout prepended to an independent native
        // Shape/Twirl fixture. This is not new Adobe-authored Basic Text proof.
        parade.splice(
            0..0,
            [
                name(MATCH_NAME),
                Chunk::list(
                    *b"sspc",
                    vec![
                        Chunk::list(*b"tdgp", controls),
                        Chunk::list(
                            *b"parT",
                            vec![
                                name("ADBE Basic Text2-0002"),
                                Chunk::data(*b"pard", point).unwrap(),
                                name("ADBE Basic Text2-0007"),
                                Chunk::data(*b"pard", popup).unwrap(),
                            ],
                        ),
                        Chunk::data(*b"sdat", payload()).unwrap(),
                    ],
                ),
            ],
        );
        if let Some(enabled) = cutout {
            parade.splice(
                2..2,
                [
                    name("ADBE Samurai"),
                    Chunk::list(
                        *b"sspc",
                        vec![Chunk::list(
                            *b"tdgp",
                            vec![Chunk::data(*b"tdsb", vec![0, 0, 0, u8::from(enabled)]).unwrap()],
                        )],
                    ),
                ],
            );
        }
        (project, original)
    }

    #[test]
    fn basic_text_full_import_places_editable_text_above_shape_and_keeps_twirl() {
        let (project, original) = qualifying_project(None);
        let imported =
            crate::structure_document::to_structural_fx_document(&project, Some(1)).unwrap();
        let document = imported.document.to_json_value().unwrap();
        fn nodes<'a>(value: &'a serde_json::Value, found: &mut Vec<&'a serde_json::Value>) {
            match value {
                serde_json::Value::Object(object) => {
                    if object.contains_key("type") && object.contains_key("id") {
                        found.push(value);
                    }
                    for child in object.values() {
                        nodes(child, found);
                    }
                }
                serde_json::Value::Array(array) => {
                    for child in array {
                        nodes(child, found);
                    }
                }
                _ => {}
            }
        }
        let mut layers = Vec::new();
        nodes(&document, &mut layers);
        let text = layers
            .iter()
            .find(|layer| layer["name"] == "Basic Text (editable generator)")
            .expect("full importer must emit editable generator");
        assert_eq!(text["type"], "Text");
        assert_eq!(text["sourceText"]["text"], "Editable");
        assert_eq!(text["sourceText"]["fontFamily"], "Arial-BoldMT");
        assert_eq!(text["sourceText"]["fontStyle"], "");
        let source = layers
            .iter()
            .find(|layer| layer["id"] == text["parent"])
            .unwrap();
        let children = source["layers"].as_array().unwrap();
        assert_eq!(children[0]["id"], text["id"]);
        let mut original_layers = Vec::new();
        nodes(&original, &mut original_layers);
        let original_source = original_layers
            .iter()
            .find(|layer| layer["id"] == source["id"])
            .unwrap();
        assert_eq!(
            &children[1..],
            original_source["layers"].as_array().unwrap(),
            "original editable Shape content must remain unchanged below Text"
        );
        assert!(
            layers
                .iter()
                .any(|layer| layer["type"] == "Rect" || layer["type"] == "Shape")
        );
        let occurrence = layers
            .iter()
            .find(|layer| layer["id"] == source["parent"])
            .unwrap();
        let original_occurrence = original_layers
            .iter()
            .find(|layer| layer["id"] == occurrence["id"])
            .unwrap();
        let effects = occurrence["effects"].as_array().unwrap();
        let original_effects = original_occurrence["effects"].as_array().unwrap();
        assert_eq!(effects.len(), original_effects.len());
        for (effect, original_effect) in effects.iter().zip(original_effects) {
            assert_eq!(
                effect["effect"], original_effect["effect"],
                "all subsequent effects must retain their editable payload and ordering"
            );
            assert_eq!(effect["enabled"], original_effect["enabled"]);
        }
        // Generated transport IDs are allocated after content: inserting Text
        // legitimately changes them, but must not alias a layer or effect ID.
        for document in [&document, &original] {
            let mut nodes_in_document = Vec::new();
            nodes(document, &mut nodes_in_document);
            let mut ids = std::collections::BTreeSet::new();
            let mut effect_ids = std::collections::BTreeSet::new();
            for layer in nodes_in_document {
                assert!(ids.insert(layer["id"].as_u64().unwrap()));
                if let Some(effects) = layer["effects"].as_array() {
                    for effect in effects {
                        let id = effect["id"].as_u64().unwrap();
                        assert!(ids.insert(id));
                        assert!(effect_ids.insert(id));
                    }
                }
            }
            for entry in document["composition"]["dynamics"]["entries"]
                .as_array()
                .unwrap()
            {
                if let Some(id) = entry["target"]["effectId"].as_u64() {
                    assert!(
                        effect_ids.contains(&id),
                        "effect animator must target this document"
                    );
                }
            }
        }
        assert!(
            occurrence["effects"]
                .as_array()
                .unwrap()
                .iter()
                .any(|effect| effect["effect"]["type"] == "twirl"),
            "subsequent native Twirl must remain on the owner"
        );
        assert!(
            imported
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("replaced with editable Text"))
        );
        assert!(!imported.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("Effect ADBE Basic Text2: no current native")
        }));
    }

    fn generated_paint(value: &serde_json::Value, matte: bool, found: &mut Vec<(bool, bool)>) {
        match value {
            serde_json::Value::Object(object) => {
                let matte = matte
                    || object
                        .get("description")
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|description| {
                            description.contains("independent matte sample copy")
                        });
                if object
                    .get("name")
                    .is_some_and(|name| name == "Basic Text (editable generator)")
                {
                    found.push((
                        matte,
                        object
                            .get("isHidden")
                            .and_then(serde_json::Value::as_bool)
                            .unwrap_or(false),
                    ));
                }
                for child in object.values() {
                    generated_paint(child, matte, found);
                }
            }
            serde_json::Value::Array(array) => {
                for child in array {
                    generated_paint(child, matte, found);
                }
            }
            _ => {}
        }
    }

    fn add_independent_sibling(project: &mut crate::structure::StructuralProject, matte: bool) {
        use crate::{
            schema::layer_records::LayerRecord,
            structure::{ItemKind, read_project},
        };
        let original = read_project(include_bytes!(
            "../../tests/fixtures/effects/static_point_controls.aep"
        ))
        .unwrap();
        let original = original.items.iter().find(|item| item.id == 1).unwrap();
        let ItemKind::Composition(original) = &original.kind else {
            panic!("native Shape fixture")
        };
        let mut consumer = original.layers[0].clone();
        let mut bytes = consumer.record.encode();
        bytes[..4].copy_from_slice(&5000u32.to_be_bytes());
        let item = project.items.iter_mut().find(|item| item.id == 1).unwrap();
        let ItemKind::Composition(composition) = &mut item.kind else {
            panic!("native Shape fixture")
        };
        let provider = if matte {
            composition.layers[0].record.id()
        } else {
            0
        };
        consumer.record = LayerRecord::decode(&bytes)
            .unwrap()
            .with_export_options(true, false, 0, 0, provider, u8::from(matte))
            .unwrap();
        consumer.name = if matte {
            "Independent matte consumer"
        } else {
            "Independent lower sibling"
        }
        .into();
        composition.layers.push(consumer);
    }

    fn named_layer<'a>(value: &'a serde_json::Value, name: &str) -> Option<&'a serde_json::Value> {
        match value {
            serde_json::Value::Object(object) => {
                if object.get("name").is_some_and(|value| value == name) {
                    return Some(value);
                }
                object.values().find_map(|child| named_layer(child, name))
            }
            serde_json::Value::Array(array) => {
                array.iter().find_map(|child| named_layer(child, name))
            }
            _ => None,
        }
    }

    fn visible_paint(value: &serde_json::Value, hidden: bool) -> bool {
        match value {
            serde_json::Value::Object(object) => {
                let hidden = hidden
                    || object
                        .get("isHidden")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false);
                if object
                    .get("type")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|kind| matches!(kind, "Text" | "Rect" | "Shape"))
                {
                    return !hidden;
                }
                object.values().any(|child| visible_paint(child, hidden))
            }
            serde_json::Value::Array(array) => {
                array.iter().any(|child| visible_paint(child, hidden))
            }
            _ => false,
        }
    }

    #[test]
    fn basic_text_downstream_cutout_hides_complete_generated_source_only_when_enabled() {
        for enabled in [true, false] {
            let (mut project, _) = qualifying_project(Some(enabled));
            add_independent_sibling(&mut project, false);
            let imported =
                crate::structure_document::to_structural_fx_document(&project, Some(1)).unwrap();
            let document = imported.document.to_json_value().unwrap();
            let mut paint = Vec::new();
            generated_paint(&document, false, &mut paint);
            assert_eq!(
                paint,
                [(false, enabled)],
                "generated Text must join the completed-source Roto Brush hide pass; enabled={enabled}"
            );
            assert_eq!(
                imported
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.message.contains("unsegmented source is hidden")),
                enabled
            );
            let sibling = named_layer(&document, "Independent lower sibling").unwrap();
            assert!(
                visible_paint(sibling, false),
                "lower sibling must remain visible under unsupported cutout fallback"
            );
        }
    }

    #[test]
    fn basic_text_cutout_matte_sample_retains_editable_generated_paint_for_existing_fallback() {
        let (mut project, _) = qualifying_project(Some(true));
        add_independent_sibling(&mut project, true);
        let imported =
            crate::structure_document::to_structural_fx_document(&project, Some(1)).unwrap();
        let document = imported.document.to_json_value().unwrap();
        let mut paint = Vec::new();
        generated_paint(&document, false, &mut paint);
        assert!(
            paint.contains(&(true, false)),
            "matte sampling must not unexpectedly hide provider paint; {paint:?}"
        );
        assert!(
            paint
                .iter()
                .filter(|(matte, _)| !matte)
                .all(|(_, hidden)| *hidden),
            "ordinary source paint must still hide under an enabled unsupported cutout; {paint:?}"
        );
    }

    fn light_sweep_project(enabled: bool) -> crate::structure::StructuralProject {
        use crate::{rifx::Chunk, structure::ItemKind};
        let (mut project, _) = qualifying_project(Some(enabled));
        fn replace(chunks: &mut [Chunk]) -> bool {
            if let Some(index) = chunks.iter().position(|chunk| {
                chunk
                    .data_payload()
                    .is_some_and(|bytes| bytes.starts_with(b"ADBE Samurai"))
            }) {
                let mut bytes = vec![0; 40];
                bytes[..14].copy_from_slice(b"CC Light Sweep");
                chunks[index] = Chunk::data(*b"tdmn", bytes).unwrap();
                let controls = chunks[index + 1]
                    .children_mut()
                    .unwrap()
                    .iter_mut()
                    .find(|chunk| chunk.list_kind() == Some(*b"tdgp"))
                    .unwrap()
                    .children_mut()
                    .unwrap();
                let mut name = vec![0; 40];
                name[..19].copy_from_slice(b"CC Light Sweep-0009");
                let mut meta = vec![0; 124];
                meta[..2].copy_from_slice(&[0xdb, 0x99]);
                meta[2..4].copy_from_slice(&1u16.to_be_bytes());
                controls.extend([
                    Chunk::data(*b"tdmn", name).unwrap(),
                    Chunk::list(
                        *b"tdbs",
                        vec![
                            Chunk::data(*b"tdsb", vec![0, 0, 0, 1]).unwrap(),
                            Chunk::data(*b"tdb4", meta).unwrap(),
                            Chunk::data(*b"cdat", 3.0f64.to_be_bytes().to_vec()).unwrap(),
                        ],
                    ),
                ]);
                return true;
            }
            chunks.iter_mut().any(|chunk| {
                chunk
                    .children_mut()
                    .is_some_and(|children| replace(children))
            })
        }
        let item = project.items.iter_mut().find(|item| item.id == 1).unwrap();
        let ItemKind::Composition(composition) = &mut item.kind else {
            panic!("native Shape fixture")
        };
        assert!(replace(&mut composition.layers[0].content));
        project
    }

    #[test]
    fn basic_text_light_sweep_cutout_hides_all_ordinary_paint_only_when_enabled() {
        for enabled in [true, false] {
            let mut project = light_sweep_project(enabled);
            add_independent_sibling(&mut project, false);
            let imported =
                crate::structure_document::to_structural_fx_document(&project, Some(1)).unwrap();
            let document = imported.document.to_json_value().unwrap();
            let mut paint = Vec::new();
            generated_paint(&document, false, &mut paint);
            assert_eq!(paint, [(false, enabled)]);
            let text = named_layer(&document, "Basic Text (editable generator)").unwrap();
            fn source_children<'a>(
                value: &'a serde_json::Value,
                id: &serde_json::Value,
            ) -> Option<&'a Vec<serde_json::Value>> {
                match value {
                    serde_json::Value::Object(object) => {
                        if object.get("id") == Some(id) {
                            return object.get("layers").and_then(serde_json::Value::as_array);
                        }
                        object.values().find_map(|child| source_children(child, id))
                    }
                    serde_json::Value::Array(array) => {
                        array.iter().find_map(|child| source_children(child, id))
                    }
                    _ => None,
                }
            }
            let children = source_children(&document, &text["parent"]).unwrap();
            assert!(
                children.len() > 1,
                "original source must survive beside generated Text"
            );
            for child in children {
                assert_eq!(
                    visible_paint(child, false),
                    !enabled,
                    "all generated and original source paint must share fallback hiding"
                );
            }
            assert_eq!(
                imported
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.message.contains("raw source paint is hidden")),
                enabled
            );
            assert!(visible_paint(
                named_layer(&document, "Independent lower sibling").unwrap(),
                false
            ));
        }
    }

    #[test]
    fn basic_text_light_sweep_cutout_retains_matte_provider_for_diagnosed_fallback() {
        let mut project = light_sweep_project(true);
        add_independent_sibling(&mut project, true);
        let imported =
            crate::structure_document::to_structural_fx_document(&project, Some(1)).unwrap();
        let document = imported.document.to_json_value().unwrap();
        let mut paint = Vec::new();
        generated_paint(&document, false, &mut paint);
        assert!(paint.contains(&(true, false)), "{paint:?}");
        assert!(
            paint
                .iter()
                .filter(|(matte, _)| !matte)
                .all(|(_, hidden)| *hidden),
            "{paint:?}"
        );
        assert!(
            imported
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("CC Light Sweep"))
        );
        assert!(visible_paint(
            named_layer(&document, "Independent matte consumer").unwrap(),
            false
        ));
    }

    #[test]
    fn basic_text_builtin_compositing_options_are_conservatively_guarded() {
        use crate::{rifx::Chunk, structure::ItemKind};

        fn first_descriptor(chunks: &mut [Chunk]) -> Option<&mut Chunk> {
            for chunk in chunks {
                if chunk.list_kind() == Some(*b"sspc") {
                    return Some(chunk);
                }
                if let Some(children) = chunk.children_mut()
                    && let Some(found) = first_descriptor(children)
                {
                    return Some(found);
                }
            }
            None
        }
        fn name(value: &str) -> Chunk {
            let mut bytes = vec![0; 40];
            bytes[..value.len()].copy_from_slice(value.as_bytes());
            Chunk::data(*b"tdmn", bytes).unwrap()
        }
        for option in [
            None,
            Some("ADBE Effect Opacity"),
            Some("ADBE Effect Mask"),
            Some("Unknown Compositing Control"),
        ] {
            let (mut project, _) = qualifying_project(None);
            let item = project.items.iter_mut().find(|item| item.id == 1).unwrap();
            let ItemKind::Composition(composition) = &mut item.kind else {
                panic!("native Shape fixture composition")
            };
            let descriptor = first_descriptor(&mut composition.layers[0].content).unwrap();
            let controls = descriptor
                .children_mut()
                .unwrap()
                .iter_mut()
                .find(|chunk| chunk.list_kind() == Some(*b"tdgp"))
                .unwrap()
                .children_mut()
                .unwrap();
            let mut options = Vec::new();
            if let Some(option) = option {
                options.push(name(option));
                options.push(Chunk::list(*b"tdgp", Vec::new()));
            }
            options.push(name("ADBE Group End"));
            controls.push(name("ADBE Effect Built In Params"));
            controls.push(Chunk::list(*b"tdgp", options));
            let imported =
                crate::structure_document::to_structural_fx_document(&project, Some(1)).unwrap();
            let document = imported.document.to_json_value().unwrap();
            let generated = named_layer(&document, "Basic Text (editable generator)");
            assert_eq!(generated.is_some(), option.is_none(), "option {option:?}");
            if option.is_some() {
                assert!(imported.diagnostics.iter().any(|diagnostic| {
                    diagnostic
                        .message
                        .contains("nonempty effect compositing options")
                }));
            }
        }
    }

    #[test]
    fn basic_text_does_not_guess_unknown_packed_defaults() {
        let mut bytes = payload();
        bytes[1802] = 2;
        assert!(packed_text(&bytes).is_err());
        bytes = payload();
        bytes[1033..1289].fill(0);
        assert!(packed_text(&bytes).is_err());
        bytes = payload();
        bytes[8..1033].fill(b'a');
        assert!(packed_text(&bytes).is_err());
        assert!(packed_text(&bytes[..1803]).is_err());
    }
}
