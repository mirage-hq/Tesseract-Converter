//! Graphic key mapping. Import inputs are Premiere model values and export
//! inputs are FX JSON documents, so neither direction is checked only against
//! the other's output.

#[path = "graphic_outline.rs"]
mod graphic_outline;

use super::*;
use crate::{
    convert::{premiere_to_tesseract, tesseract_to_premiere},
    format::{FrameRate, PremiereProjectXml},
    schema::{
        text_shadow::PrTextShadow, PrKeyframeEasing, PrPointKeyframe, PrProjectFile, PrVideoItem,
        PrVideoTrack, TICKS,
    },
    test_support::editable_document,
    tests::support::{shape_graphic, text_graphic, video_media, video_sequence},
    OmissionKind,
};
use fx_schema::EditableFxCompositionDocument;
use serde_json::{json, Value};

/// The generator time at which export places every graphic (30 fps).
const EXPORT_IN: i64 = FrameRate::Fps30.generator_in_ticks();

fn scalar(source_ticks: i64, value: f64, easing: PrKeyframeEasing) -> PrScalarKeyframe {
    PrScalarKeyframe {
        source_ticks,
        value,
        easing,
    }
}

/// The FX document of the video sequence with `graphic` on a second track,
/// and every omission.
fn imported(graphic: PrGraphic) -> (Value, Vec<Omission>) {
    imported_graphics(vec![graphic])
}

/// The FX document of the video sequence with `graphics` on a second track,
/// and every omission.
fn imported_graphics(graphics: Vec<PrGraphic>) -> (Value, Vec<Omission>) {
    let mut sequence = video_sequence();
    sequence.video_tracks.push(PrVideoTrack {
        items: graphics.into_iter().map(PrVideoItem::Graphic).collect(),
        transitions: Vec::new(),
        nests: Vec::new(),
    });
    let media = video_media();
    let mut omissions = Vec::new();
    let document = premiere_to_tesseract(
        &sequence,
        &media,
        &crate::tesseract_output::asset_ids_in_order(&sequence, &media),
        &mut omissions,
    )
    .unwrap()
    .to_json_value()
    .unwrap();
    (document, omissions)
}

#[test]
fn a_blended_graphic_groups_its_object_and_writes_the_blend_back() {
    use crate::schema::PrBlendMode;
    for object in [text_graphic(), shape_graphic()] {
        let graphic = PrGraphic {
            blend_mode: PrBlendMode::LighterColor,
            ..object
        };
        let (mut document, omissions) = imported(graphic);
        // One object at the clip's defaults otherwise stays ungrouped; the
        // group blends the whole graphic.
        let group = &document["composition"]["layers"][0];
        assert_eq!(
            (&group["type"], &group["blendMode"]),
            (&json!("Group"), &json!("lighterColor"))
        );
        assert_eq!(group["layers"][0]["blendMode"], "normal");
        let reports = |omissions: &[Omission]| -> Vec<OmissionKind> {
            omissions
                .iter()
                .filter(|omission| omission.reason.contains("Blend Mode (12, 13)"))
                .map(|omission| omission.kind)
                .collect()
        };
        assert_eq!(reports(&omissions), [OmissionKind::Approximated]);
        // A graphic-only export: drop the video layer and keep the black canvas.
        document["composition"]["layers"]
            .as_array_mut()
            .unwrap()
            .remove(1);
        let document = EditableFxCompositionDocument::from_json_value(document).unwrap();
        let mut omissions = Vec::new();
        let project = tesseract_to_premiere(
            &document,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            FrameRate::Fps30,
            &mut omissions,
        )
        .unwrap();
        assert_eq!(reports(&omissions), [OmissionKind::Approximated]);
        // The written project reads the pair back on the graphic clip.
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("blended.prproj");
        PremiereProjectXml::new(&project)
            .unwrap()
            .write_new(&path)
            .unwrap();
        let (reopened, _) = PrProjectFile::load(&path).unwrap();
        for project in [&project, &reopened] {
            let exported = project.sequences[0]
                .video_items()
                .find_map(PrVideoItem::graphic)
                .unwrap();
            assert_eq!(exported.blend_mode, PrBlendMode::LighterColor);
        }
    }
}

#[test]
fn a_lone_objects_blend_is_its_graphics_and_an_objects_among_several_is_lost() {
    use crate::schema::PrBlendMode;
    let blended = |mut layer: Value, mode: &str| {
        layer["blendMode"] = json!(mode);
        layer
    };
    // A rectangle that is not a Color Matte (Opacity 50) exports as a Shape.
    let mut rect = editable_document()["composition"]["layers"][1].clone();
    rect["id"] = json!(9);
    rect["name"] = json!("Half solid");
    rect["activeRange"] = json!({"start": 0, "duration": 1000});
    rect["transform"]["opacity"] = json!(50);
    // Group 8 holds text 9 and shape 11, which blends.
    let mut child = blended(imported_object(shape_graphic()), "screen");
    child["id"] = json!(11);
    child["parent"] = json!(8);
    let mut group = graphic_group(json!({}));
    group["layers"].as_array_mut().unwrap().push(child);
    let lighter = "Blend Mode (12, 13) Lighter Color picks each pixel's layer by BT.709 luma in Premiere and by channel sum in FX, so pixels whose two orders differ show the other layer (9.9 levels mean error on the measured colour chart)";
    for (layer, record, objects, mode, reports) in [
        (
            blended(imported_object(shape_graphic()), "multiply"),
            "layer 9 (\"Box\")",
            1,
            PrBlendMode::Multiply,
            vec![],
        ),
        (
            blended(root_title(), "lighterColor"),
            "layer 9 (\"Title\")",
            1,
            PrBlendMode::LighterColor,
            vec![(OmissionKind::Approximated, lighter.to_owned())],
        ),
        (
            blended(rect, "screen"),
            "layer 9 (\"Half solid\")",
            1,
            PrBlendMode::Screen,
            vec![],
        ),
        (
            group,
            "layer 11 (\"Box\")",
            2,
            PrBlendMode::Normal,
            vec![(
                OmissionKind::Omitted,
                "unsupported blend mode was not exported (using normal)".to_owned(),
            )],
        ),
    ] {
        let (project, omissions) = export_over_canvas(vec![layer], Vec::new());
        let project = project.unwrap_or_else(|error| panic!("{error}: {omissions:?}"));
        let [graphic] = project
            .single_sequence()
            .unwrap()
            .video_items()
            .filter_map(PrVideoItem::graphic)
            .collect::<Vec<_>>()[..]
        else {
            panic!("{record}: one graphic: {omissions:?}");
        };
        assert_eq!(
            (graphic.objects.len(), graphic.blend_mode),
            (objects, mode),
            "{record}"
        );
        let reported: Vec<_> = omissions
            .iter()
            .filter(|omission| omission.record == record)
            .map(|omission| (omission.kind, omission.reason.clone()))
            .collect();
        assert_eq!(reported, reports, "{record}: {omissions:?}");
    }
}

/// The keyframes of `layer`'s `property` track as (time, value, easing type).
fn track(document: &Value, layer: u64, property: &str) -> Vec<(i64, f64, String)> {
    let entries = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    let entry = entries
        .iter()
        .find(|entry| {
            entry["target"]["layerId"] == layer && entry["target"]["propertyType"] == property
        })
        .unwrap_or_else(|| panic!("no {property} track on layer {layer}"));
    entry["animator"]["keyframes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|key| {
            (
                key["layerTime"].as_i64().unwrap(),
                key["value"]["value"].as_f64().unwrap(),
                key["easing"]["type"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

fn keys(values: &[(i64, f64, &str)]) -> Vec<(i64, f64, String)> {
    values
        .iter()
        .map(|&(time, value, easing)| (time, value, easing.to_owned()))
        .collect()
}

/// `text_graphic` with keys on every text object property, some before and
/// after its two-second placement.
fn keyed_graphic() -> PrGraphic {
    use PrKeyframeEasing::{Hold, Linear};
    let mut graphic = text_graphic();
    let start = graphic.in_ticks;
    graphic.text_mut().animations = vec![
        PrPropertyAnimation::Position(vec![
            PrPointKeyframe {
                source_ticks: start - TICKS / 2,
                value: [0.25, 0.5],
                easing: Linear,
                spatial_in_tangent: None,
                spatial_out_tangent: Some([0.05, 0.0]),
            },
            PrPointKeyframe {
                source_ticks: start + TICKS,
                value: [0.3, 0.55],
                easing: Linear,
                spatial_in_tangent: Some([-0.05, 0.0]),
                spatial_out_tangent: None,
            },
            PrPointKeyframe {
                source_ticks: start + 5 * TICKS / 2,
                value: [0.3, 0.6],
                easing: Hold,
                spatial_in_tangent: None,
                spatial_out_tangent: None,
            },
        ]),
        PrPropertyAnimation::UniformScale(vec![
            scalar(start, 80.0, Linear),
            scalar(start + TICKS, 96.0, Linear),
        ]),
        PrPropertyAnimation::Rotation(vec![
            scalar(start, -15.0, Linear),
            scalar(start + TICKS, 20.0, Hold),
        ]),
        PrPropertyAnimation::Opacity(vec![
            scalar(start + TICKS / 2, 75.0, Linear),
            scalar(start + 3 * TICKS / 2, 40.0, Linear),
        ]),
    ];
    graphic
}

#[test]
fn text_object_keys_become_text_layer_tracks_timed_from_the_in_point() {
    let (document, omissions) = imported(keyed_graphic());
    // Only the unpackaged font is reported.
    assert_eq!(omissions.len(), 1, "{omissions:?}");
    let layers = document["composition"]["layers"].as_array().unwrap();
    assert_eq!(layers[0]["type"], "Text");
    assert_eq!(layers[0]["id"], 2);
    // Layer times are key times minus the in point, so keys outside the
    // placement stay; positions become pixels with their spatial tangents.
    assert_eq!(
        track(&document, 2, "positionX"),
        keys(&[
            (-500, 480.0, "linear"),
            (1000, 576.0, "linear"),
            (2500, 576.0, "hold")
        ])
    );
    assert_eq!(
        track(&document, 2, "positionY"),
        keys(&[
            (-500, 540.0, "linear"),
            (1000, 594.0, "linear"),
            (2500, 648.0, "hold")
        ])
    );
    let entries = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    let position_x = entries
        .iter()
        .find(|entry| entry["target"]["propertyType"] == "positionX")
        .unwrap();
    let first = &position_x["animator"]["keyframes"][0];
    assert_eq!(first["id"], "premiere-position-x-2-0");
    assert_eq!(first["spatialOutTangent"], 96.0);
    assert_eq!(
        position_x["animator"]["keyframes"][1]["spatialInTangent"],
        -96.0
    );
    for axis in ["scaleX", "scaleY"] {
        assert_eq!(
            track(&document, 2, axis),
            keys(&[(0, 80.0, "linear"), (1000, 96.0, "linear")])
        );
    }
    assert_eq!(
        track(&document, 2, "rotation"),
        keys(&[(0, -15.0, "linear"), (1000, 20.0, "hold")])
    );
    assert_eq!(
        track(&document, 2, "opacity"),
        keys(&[(500, 75.0, "linear"), (1500, 40.0, "linear")])
    );
}

#[test]
fn text_keys_that_cannot_import_are_reported_and_the_others_convert() {
    let mut graphic = keyed_graphic();
    let start = graphic.in_ticks;
    // Two Scale keys 0.2 ms apart collide on the FX millisecond clock.
    graphic.text_mut().animations[1] = PrPropertyAnimation::UniformScale(vec![
        scalar(start, 80.0, PrKeyframeEasing::Linear),
        scalar(start + TICKS / 5000, 96.0, PrKeyframeEasing::Linear),
    ]);
    let (document, omissions) = imported(graphic);
    let reasons: Vec<_> = omissions
        .iter()
        .filter(|omission| omission.record == "20")
        .map(|omission| (omission.scope, omission.reason.as_str()))
        .collect();
    assert_eq!(reasons.len(), 1, "{omissions:?}");
    assert_eq!(reasons[0].0, OmissionScope::Feature);
    assert!(
        reasons[0]
            .1
            .starts_with("Scale animation was not imported:"),
        "{reasons:?}"
    );
    let properties: Vec<_> = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["target"]["propertyType"].as_str().unwrap())
        .collect();
    assert_eq!(
        properties,
        ["positionX", "positionY", "rotation", "opacity"]
    );
}

/// A keyframe track entry for text layer 9 in FX JSON.
fn entry(property: &str, keys: &[(i64, f64, &str)]) -> Value {
    json!({
        "target": {"kind": "layer", "layerId": 9, "propertyType": property},
        "animator": {"type": "keyframes", "enabled": true, "keyframes": keys.iter().enumerate().map(|(index, (time, value, easing))| json!({
            "id": format!("{property}-{index}"),
            "layerTime": time,
            "value": {"type": "float", "value": value},
            "easing": {"type": easing},
        })).collect::<Vec<_>>()}
    })
}

/// Export a text-only document whose text layer 9 (1 s long, at 960, 540)
/// has `entries` and `effects`, with the graphic and every omission.
fn exported(entries: Vec<Value>, effects: Option<Value>) -> (PrGraphic, Vec<Omission>) {
    exported_at(entries, effects, FrameRate::Fps30)
}

/// [`exported`] into a sequence of `frame_rate`.
fn exported_at(
    entries: Vec<Value>,
    effects: Option<Value>,
    frame_rate: FrameRate,
) -> (PrGraphic, Vec<Omission>) {
    let mut wire = editable_document();
    let mut text = json!({
        "type": "Text",
        "id": 9,
        "name": "Title",
        "activeRange": {"start": 0, "duration": 1000},
        "transform": {"anchorPoint": [0, 0], "position": [960, 540], "scale": [100, 100], "rotation": 0, "opacity": 100},
        "sourceText": {"text": "Keys", "fontFamily": "Inter-Bold", "fontStyle": "", "fontSize": 80, "fillColor": [1, 1, 1, 1]},
    });
    if let Some(effects) = effects {
        text["effects"] = effects;
    }
    wire["composition"]["layers"][0] = text;
    wire["composition"]["dynamics"] = json!({ "entries": entries });
    let document = EditableFxCompositionDocument::from_json_value(wire).unwrap();
    let mut omissions = Vec::new();
    let project = tesseract_to_premiere(
        &document,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        frame_rate,
        &mut omissions,
    )
    .unwrap();
    let graphic = project
        .single_sequence()
        .unwrap()
        .video_items()
        .find_map(PrVideoItem::graphic)
        .expect("the text exports")
        .clone();
    (graphic, omissions)
}

#[test]
fn graphic_keys_use_the_generator_clock_of_the_export_rate() {
    // Key times stay exact in milliseconds; the in point is the export rate's
    // one hour, rounded down to a frame.
    for frame_rate in [
        FrameRate::Fps24000Over1001,
        FrameRate::Fps25,
        FrameRate::Fps60000Over1001,
    ] {
        let (graphic, _) = exported_at(
            vec![entry(
                "opacity",
                &[(0, 100.0, "linear"), (700, 20.0, "linear")],
            )],
            None,
            frame_rate,
        );
        let start = frame_rate.generator_in_ticks();
        assert_eq!(graphic.in_ticks, start, "{frame_rate}");
        assert_eq!(
            graphic.text().animations,
            [PrPropertyAnimation::Opacity(vec![
                scalar(start, 100.0, PrKeyframeEasing::Linear),
                scalar(start + 7 * TICKS / 10, 20.0, PrKeyframeEasing::Linear),
            ])],
            "{frame_rate}"
        );
    }
}

#[test]
fn text_layer_tracks_export_as_text_object_keys_on_the_generator_clock() {
    use PrKeyframeEasing::{Hold, Linear};
    let (graphic, omissions) = exported(
        vec![
            entry("positionX", &[(0, 960.0, "linear"), (1500, 480.0, "hold")]),
            entry("positionY", &[(0, 540.0, "linear"), (1500, 270.0, "hold")]),
            entry("scaleX", &[(-500, 100.0, "linear"), (500, 150.0, "linear")]),
            entry("scaleY", &[(-500, 100.0, "linear"), (500, 150.0, "linear")]),
            entry("rotation", &[(0, 0.0, "linear"), (500, 45.0, "hold")]),
            entry("opacity", &[(0, 100.0, "linear"), (800, 40.0, "linear")]),
        ],
        None,
    );
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(graphic.in_ticks, EXPORT_IN);
    let at = |millis: i64| EXPORT_IN + millis * TICKS / 1000;
    assert_eq!(
        graphic.text().animations,
        [
            PrPropertyAnimation::Opacity(vec![
                scalar(at(0), 100.0, Linear),
                scalar(at(800), 40.0, Linear),
            ]),
            PrPropertyAnimation::Rotation(vec![
                scalar(at(0), 0.0, Linear),
                scalar(at(500), 45.0, Hold),
            ]),
            PrPropertyAnimation::Position(vec![
                PrPointKeyframe {
                    source_ticks: at(0),
                    value: [0.5, 0.5],
                    easing: Linear,
                    spatial_in_tangent: None,
                    spatial_out_tangent: None,
                },
                PrPointKeyframe {
                    source_ticks: at(1500),
                    value: [0.25, 0.25],
                    easing: Hold,
                    spatial_in_tangent: None,
                    spatial_out_tangent: None,
                },
            ]),
            PrPropertyAnimation::UniformScale(vec![
                scalar(at(-500), 100.0, Linear),
                scalar(at(500), 150.0, Linear),
            ]),
        ]
    );
}

#[test]
fn text_tracks_premiere_cannot_hold_are_reported_and_the_text_exports() {
    let title = "layer 9 (\"Title\")";
    for (entries, record, reason) in [
        (
            vec![entry("scaleY", &[(0, 100.0, "linear"), (500, 5000.0, "linear")])],
            title,
            "Vertical Scale animation was not exported: unsupported conversion: Scale keys must stay within Premiere's range 0..4000",
        ),
        (
            vec![
                entry("scaleX", &[(0, 100.0, "linear"), (500, 150.0, "linear")]),
                entry("scaleY", &[(0, 100.0, "linear"), (500, 120.0, "linear")]),
            ],
            title,
            "nonuniform or unpaired Scale keyframes were not exported",
        ),
        (
            vec![entry("positionX", &[(0, 960.0, "linear"), (500, 480.0, "linear")])],
            title,
            "unpaired Position keyframes were not exported",
        ),
        (
            vec![entry("rotation", &[(0, 0.0, "linear"), (500, 40000.0, "linear")])],
            title,
            "Rotation animation was not exported: unsupported conversion: Rotation keys must stay within Premiere's range -32768..32767",
        ),
        (
            vec![
                entry("scaleX", &[(0, 100.0, "linear"), (500, 5000.0, "linear")]),
                entry("scaleY", &[(0, 100.0, "linear"), (500, 5000.0, "linear")]),
            ],
            title,
            "Scale animation was not exported: unsupported conversion: Scale keys must stay within Premiere's range 0..4000",
        ),
        // Anchor Point keys export only as a clip's Motion keys; a text
        // still reports them, as before.
        (
            vec![
                entry("anchorPointX", &[(0, 0.0, "linear"), (500, 40.0, "linear")]),
                entry("anchorPointY", &[(0, 0.0, "linear"), (500, 20.0, "linear")]),
            ],
            "layer 9",
            "only independent Opacity, paired Position, Rotation, uniform Scale, audio volume and text Source Text keyframes can be exported; animation was omitted",
        ),
        // Style keys of a text whose content is not keyed do not become
        // Source Text keys: the text stays static, as before Source Text
        // keys exported.
        (
            vec![
                entry("fontSize", &[(0, 80.0, "hold"), (500, 120.0, "hold")]),
                valued_entry(
                    "strokeEnabled",
                    &[
                        (0, json!({"type": "bool", "value": false}), "hold"),
                        (500, json!({"type": "bool", "value": true}), "hold"),
                    ],
                ),
            ],
            "layer 9",
            "only independent Opacity, paired Position, Rotation, uniform Scale, audio volume and text Source Text keyframes can be exported; animation was omitted",
        ),
    ] {
        let (graphic, omissions) = exported(entries, None);
        assert!(graphic.text().animations.is_empty(), "{reason}");
        assert!(graphic.text().source_text_keys.is_empty(), "{reason}");
        let reasons: Vec<_> = omissions
            .iter()
            .map(|omission| (omission.scope, omission.record.as_str(), omission.reason.as_str()))
            .collect();
        assert_eq!(reasons, [(OmissionScope::Feature, record, reason)]);
    }
}

#[test]
fn text_tracks_beyond_the_former_key_limit_export() {
    let limit = 4096;
    let values = |count: usize, base: f64| -> Vec<(i64, f64, &str)> {
        (0..count)
            .map(|index| (index as i64, base + (index % 2) as f64, "linear"))
            .collect()
    };
    let opacity = |count| vec![entry("opacity", &values(count, 50.0))];
    let position = |count| {
        vec![
            entry("positionX", &values(count, 970.0)),
            entry("positionY", &values(count, 540.0)),
        ]
    };
    let (kept, moved) = ([1152.0, 594.0], [1162.0, 594.0]);
    for (entries, property, name, count, position) in [
        (
            opacity(limit),
            PrAnimatedProperty::Opacity,
            "Opacity",
            limit,
            kept,
        ),
        (
            opacity(limit + 1),
            PrAnimatedProperty::Opacity,
            "Opacity",
            limit + 1,
            kept,
        ),
        (
            position(limit),
            PrAnimatedProperty::Position,
            "Position",
            limit,
            moved,
        ),
        (
            position(limit + 1),
            PrAnimatedProperty::Position,
            "Position",
            limit + 1,
            moved,
        ),
    ] {
        let (_, graphic, omissions) = written_graphic(entries);
        let written = graphic
            .text()
            .animations
            .iter()
            .find(|animation| animation.property() == property)
            .map_or(0, |animation| match animation {
                PrPropertyAnimation::Position(keys) => keys.len(),
                scalar => scalar.keys().len(),
            });
        assert_eq!(written, count, "{name}: {omissions:?}");
        assert!(omissions.is_empty(), "{name}: {omissions:?}");
        let transform = graphic.text().transform;
        assert_eq!(transform.opacity, 100.0, "{name}");
        assert!(
            (0..2).all(|axis| (transform.position[axis] - position[axis]).abs() < 1e-9),
            "{name}: {:?}",
            transform.position
        );
    }
}

#[test]
fn imported_text_keys_edited_in_the_document_export_and_read_back() {
    let (mut document, _) = imported(keyed_graphic());
    // Move the second Opacity key 250 ms later and change its value.
    let entries = document["composition"]["dynamics"]["entries"]
        .as_array_mut()
        .unwrap();
    let opacity = entries
        .iter_mut()
        .find(|entry| entry["target"]["propertyType"] == "opacity")
        .unwrap();
    opacity["animator"]["keyframes"][1]["layerTime"] = json!(1750);
    opacity["animator"]["keyframes"][1]["value"]["value"] = json!(10.0);
    // A text-only export: drop the video layer and keep the black canvas.
    document["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .remove(1);
    let document = EditableFxCompositionDocument::from_json_value(document).unwrap();
    let mut omissions = Vec::new();
    let project = tesseract_to_premiere(
        &document,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        FrameRate::Fps30,
        &mut omissions,
    )
    .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("edited.prproj");
    PremiereProjectXml::new(&project)
        .unwrap()
        .write_new(&path)
        .unwrap();
    let (reopened, _) = PrProjectFile::load(&path).unwrap();
    let graphic = reopened.sequences[0]
        .video_items()
        .find_map(PrVideoItem::graphic)
        .unwrap();
    let opacity = graphic
        .text()
        .animations
        .iter()
        .find(|animation| animation.property() == PrAnimatedProperty::Opacity)
        .unwrap();
    assert_eq!(
        opacity.keys(),
        [
            scalar(EXPORT_IN + TICKS / 2, 75.0, PrKeyframeEasing::Linear),
            scalar(EXPORT_IN + 7 * TICKS / 4, 10.0, PrKeyframeEasing::Linear),
        ]
    );
    // The other keys come back unchanged, including those outside the trim.
    let rotation = graphic
        .text()
        .animations
        .iter()
        .find(|animation| animation.property() == PrAnimatedProperty::Rotation)
        .unwrap();
    assert_eq!(
        rotation.keys(),
        [
            scalar(EXPORT_IN, -15.0, PrKeyframeEasing::Linear),
            scalar(EXPORT_IN + TICKS, 20.0, PrKeyframeEasing::Hold),
        ]
    );
    let position = graphic
        .text()
        .animations
        .iter()
        .find_map(PrPropertyAnimation::point_keys)
        .unwrap();
    assert_eq!(position[0].source_ticks, EXPORT_IN - TICKS / 2);
    assert_eq!(position[2].source_ticks, EXPORT_IN + 5 * TICKS / 2);
}

#[test]
fn a_text_shadow_on_text_with_scale_or_rotation_keys_is_omitted_in_both_directions() {
    // Import: the static text is unscaled and unrotated; its keys are not.
    let mut graphic = keyed_graphic();
    graphic.text_mut().transform.scale = 100.0;
    graphic.text_mut().transform.rotation = 0.0;
    graphic.text_mut().document.shadow = Some(PrTextShadow {
        color: crate::schema::text::PrRgb([0, 0, 0]),
        opacity: 100.0,
        angle: 135.0,
        distance: 3.0,
        size: 6.0,
        blur: 12.0,
    });
    let (document, omissions) = imported(graphic);
    assert!(document["composition"]["layers"][0]
        .get("effects")
        .is_none());
    assert!(
        omissions
            .iter()
            .any(|omission| omission.scope == OmissionScope::Feature
                && omission.record == "20"
                && omission.reason
                    == "text shadow not converted: its text has Scale or Rotation keys"),
        "{omissions:?}"
    );
    // Export: the same rule for a rotation track.
    let shadow = json!([{"id": 5, "effect": {"type": "dropShadow", "offset": [3, 3], "color": [0, 0, 0, 1]}}]);
    let (graphic, omissions) = exported(
        vec![entry(
            "rotation",
            &[(0, 0.0, "linear"), (500, 45.0, "linear")],
        )],
        Some(shadow.clone()),
    );
    assert_eq!(graphic.text().document.shadow, None);
    assert_eq!(
        omissions
            .iter()
            .map(|omission| omission.reason.as_str())
            .collect::<Vec<_>>(),
        ["drop shadow 5 was not exported: unsupported conversion: its text has Scale or Rotation keys"]
    );
    // Opacity and Position keys keep the shadow.
    let (graphic, omissions) = exported(
        vec![entry(
            "opacity",
            &[(0, 100.0, "linear"), (500, 50.0, "linear")],
        )],
        Some(shadow),
    );
    assert!(graphic.text().document.shadow.is_some(), "{omissions:?}");
}

/// Keyed Vector Motion like case A's on top of `keyed_graphic`: an
/// off-centre anchor, Position keys with a Hold, Scale 100 -> 70 Linear and
/// Rotation 0 -> 20 Hold.
fn vector_motion_graphic() -> PrGraphic {
    use PrKeyframeEasing::{Hold, Linear};
    let mut graphic = keyed_graphic();
    let start = graphic.in_ticks;
    let point = |source_ticks, value, easing| PrPointKeyframe {
        source_ticks,
        value,
        easing,
        spatial_in_tangent: None,
        spatial_out_tangent: None,
    };
    graphic.vector_motion = Some(PrVectorMotion {
        position: [960.0, 540.0],
        anchor: [768.0, 486.0],
        scale: 100.0,
        rotation: 0.0,
        animations: vec![
            PrPropertyAnimation::Position(vec![
                point(start, [0.5, 0.5], Linear),
                point(start + TICKS / 2, [0.55, 0.45], Linear),
                point(start + TICKS, [0.6, 0.4], Hold),
            ]),
            PrPropertyAnimation::UniformScale(vec![
                scalar(start, 100.0, Linear),
                scalar(start + TICKS, 70.0, Linear),
            ]),
            PrPropertyAnimation::Rotation(vec![
                scalar(start, 0.0, Linear),
                scalar(start + TICKS, 20.0, Hold),
            ]),
        ],
    });
    graphic
}

#[test]
fn keyed_vector_motion_becomes_a_graphic_group_around_the_text() {
    let (document, omissions) = imported(vector_motion_graphic());
    assert_eq!(omissions.len(), 1, "{omissions:?}");
    let layers = document["composition"]["layers"].as_array().unwrap();
    let types: Vec<_> = layers.iter().map(|layer| &layer["type"]).collect();
    assert_eq!(types, ["Group", "Video", "Rect"]);
    // The group takes the placement's range and the Vector Motion transform;
    // the text keeps its own layer id, transform and keys inside it.
    let group = &layers[0];
    assert_eq!(group["id"], 3);
    assert_eq!(group["name"], "Premiere graphic 2");
    assert_eq!(
        (*crate::test_support::layer_range(group)),
        json!({"start": 1000, "duration": 2000})
    );
    assert_eq!(group["transform"]["position"], json!([960.0, 540.0]));
    assert_eq!(group["transform"]["anchorPoint"], json!([768.0, 486.0]));
    assert_eq!(group["transform"]["scale"], json!([100.0, 100.0]));
    let children = group["layers"].as_array().unwrap();
    assert_eq!(children.len(), 1);
    let text = &children[0];
    assert_eq!(
        (&text["type"], &text["id"], &text["parent"]),
        (&json!("Text"), &json!(2), &json!(3))
    );
    assert_eq!(
        (*crate::test_support::layer_range(text)),
        json!({"start": 0, "duration": 2000})
    );
    assert_eq!(text["transform"]["position"], json!([480.0, 540.0]));
    // Both clocks start at the placement, so key times are the same offsets
    // from the in point.
    assert_eq!(
        track(&document, 3, "positionX"),
        keys(&[
            (0, 960.0, "linear"),
            (500, 1056.0, "linear"),
            (1000, 1152.0, "hold")
        ])
    );
    assert_eq!(
        track(&document, 3, "positionY"),
        keys(&[
            (0, 540.0, "linear"),
            (500, 486.0, "linear"),
            (1000, 432.0, "hold")
        ])
    );
    for axis in ["scaleX", "scaleY"] {
        assert_eq!(
            track(&document, 3, axis),
            keys(&[(0, 100.0, "linear"), (1000, 70.0, "linear")])
        );
    }
    assert_eq!(
        track(&document, 3, "rotation"),
        keys(&[(0, 0.0, "linear"), (1000, 20.0, "hold")])
    );
    assert_eq!(
        track(&document, 2, "opacity"),
        keys(&[(500, 75.0, "linear"), (1500, 40.0, "linear")])
    );
    let ids: Vec<_> = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["animator"]["keyframes"][0]["id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&"premiere-scale-x-3-0") && ids.contains(&"premiere-opacity-2-0"));

    // A disabled graphic hides its group, not the text inside it.
    let mut disabled = vector_motion_graphic();
    disabled.enabled = false;
    let (document, _) = imported(disabled);
    let group = &document["composition"]["layers"][0];
    assert_eq!(group["isHidden"], true);
    assert!(group["layers"][0].get("isHidden").is_none());
}

/// A keyframe track's value at `time` ms, as `fx_composition` evaluates
/// keyframes (`animator/keyframes.rs`): the first or last value outside the
/// keys, the earlier value through a Hold interval, linear progress (graphic
/// keys have no Bezier timing), and a cubic on each axis when either key has
/// a spatial tangent.
fn evaluate(keys: &[Value], time: f64) -> f64 {
    let at = |key: &Value| key["layerTime"].as_f64().unwrap();
    let value = |key: &Value| key["value"]["value"].as_f64().unwrap();
    let Some(next) = keys.iter().position(|key| at(key) > time) else {
        return value(&keys[keys.len() - 1]);
    };
    if next == 0 {
        return value(&keys[0]);
    }
    let (left, right) = (&keys[next - 1], &keys[next]);
    let progress = match right["easing"]["type"].as_str().unwrap() {
        "hold" => return value(left),
        "linear" => (time - at(left)) / (at(right) - at(left)),
        easing => panic!("graphic keys have Linear or Hold timing, not {easing}"),
    };
    let (from, to) = (value(left), value(right));
    let outgoing = left["spatialOutTangent"].as_f64();
    let incoming = right["spatialInTangent"].as_f64();
    if outgoing.is_none() && incoming.is_none() {
        return from + (to - from) * progress;
    }
    let first = from + outgoing.unwrap_or((to - from) / 3.0);
    let second = to + incoming.unwrap_or((from - to) / 3.0);
    let inverse = 1.0 - progress;
    inverse.powi(3) * from
        + 3.0 * inverse * inverse * progress * first
        + 3.0 * inverse * progress * progress * second
        + progress.powi(3) * to
}

/// Where `layer` draws its local `point` at layer time `time`, given the
/// document's tracks: position + scale * R(rotation) * (point - anchor).
fn place(document: &Value, layer: &Value, point: [f64; 2], time: f64) -> ([f64; 2], f64) {
    let id = layer["id"].as_u64().unwrap();
    let transform = &layer["transform"];
    let property = |name: &str, fallback: f64| {
        document["composition"]["dynamics"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| {
                entry["target"]["layerId"] == id && entry["target"]["propertyType"] == name
            })
            .map_or(fallback, |entry| {
                evaluate(entry["animator"]["keyframes"].as_array().unwrap(), time)
            })
    };
    let number = |value: &Value| value.as_f64().unwrap();
    let position = [
        property("positionX", number(&transform["position"][0])),
        property("positionY", number(&transform["position"][1])),
    ];
    let scale = property("scaleX", number(&transform["scale"][0])) / 100.0;
    let (sin, cos) = property("rotation", number(&transform["rotation"]))
        .to_radians()
        .sin_cos();
    let offset = [
        point[0] - number(&transform["anchorPoint"][0]),
        point[1] - number(&transform["anchorPoint"][1]),
    ];
    let placed = [
        position[0] + scale * (offset[0] * cos - offset[1] * sin),
        position[1] + scale * (offset[0] * sin + offset[1] * cos),
    ];
    (placed, property("opacity", number(&transform["opacity"])))
}

#[test]
fn a_composed_static_vector_motion_draws_like_the_same_graphic_as_a_group() {
    // A static Vector Motion with an off-centre anchor, 80% and 30°, over text
    // keys that include a curved path, with Linear and Hold timing.
    let motion = PrVectorMotion {
        position: [900.0, 500.0],
        anchor: [768.0, 486.0],
        scale: 80.0,
        rotation: 30.0,
        animations: Vec::new(),
    };
    let graphic = keyed_graphic();
    let mut as_group = graphic.clone();
    as_group.vector_motion = Some(motion.clone());
    let mut composed = graphic;
    composed
        .text_mut()
        .compose_static_vector_motion(&motion, [1920, 1080]);
    let (group_document, _) = imported(as_group);
    let (composed_document, _) = imported(composed);
    let group = &group_document["composition"]["layers"][0];
    let text_in_group = &group["layers"][0];
    let text = &composed_document["composition"]["layers"][0];
    assert_eq!(
        (&group["type"], &text["type"]),
        (&json!("Group"), &json!("Text"))
    );
    // Key times of every track, with the midpoints between them.
    let mut times: Vec<f64> = vec![-500.0, 0.0, 500.0, 1000.0, 1500.0, 2500.0];
    let midpoints: Vec<f64> = times
        .windows(2)
        .map(|pair| (pair[0] + pair[1]) / 2.0)
        .collect();
    times.extend(midpoints);
    for time in times {
        for point in [[0.0, 0.0], [120.0, -40.0], [-35.0, 80.0]] {
            let (local, opacity) = place(&group_document, text_in_group, point, time);
            let (grouped, _) = place(&group_document, group, local, time);
            let (flat, flat_opacity) = place(&composed_document, text, point, time);
            assert!(
                (grouped[0] - flat[0]).abs() < 1e-6 && (grouped[1] - flat[1]).abs() < 1e-6,
                "{time} ms, {point:?}: group {grouped:?} != composed {flat:?}"
            );
            assert_eq!(opacity, flat_opacity, "{time} ms");
        }
    }
}

/// Group 8 (1 s long, from 1 s) holding only text layer 9, with `group`
/// merged into the group's JSON.
fn graphic_group(group: Value) -> Value {
    let mut layer = json!({
        "type": "Group",
        "id": 8,
        "name": "Graphic",
        "playback": crate::test_support::linear_playback(json!({"start": 1000, "duration": 1000}), json!({"start": 0, "duration": 1000})),
        "transform": {"anchorPoint": [768, 486], "position": [960, 540], "scale": [100, 100], "rotation": 0, "opacity": 100},
        "layers": [{
            "type": "Text",
            "id": 9,
            "name": "Title",
            "parent": 8,
            "activeRange": {"start": 0, "duration": 1000},
            "transform": {"anchorPoint": [0, 0], "position": [960, 540], "scale": [100, 100], "rotation": 0, "opacity": 100},
            "sourceText": {"text": "Keys", "fontFamily": "Inter-Bold", "fontStyle": "", "fontSize": 80, "fillColor": [1, 1, 1, 1]}
        }],
    });
    for (key, value) in group.as_object().unwrap() {
        layer[key] = value.clone();
    }
    layer
}

/// Export `layers` over the black canvas (layer 2) of a two-second document,
/// with `entries` as its animation, and every omission.
fn export_over_canvas(
    layers: Vec<Value>,
    entries: Vec<Value>,
) -> (crate::error::Result<PrProjectFile>, Vec<Omission>) {
    let mut wire = editable_document();
    wire["duration"] = json!(2.0);
    let mut canvas = wire["composition"]["layers"][1].clone();
    canvas["activeRange"]["duration"] = json!(2000);
    wire["composition"]["layers"] = layers.into_iter().chain([canvas]).collect();
    wire["composition"]["dynamics"] = json!({ "entries": entries });
    let document = EditableFxCompositionDocument::from_json_value(wire).unwrap();
    let mut omissions = Vec::new();
    let project = tesseract_to_premiere(
        &document,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        FrameRate::Fps30,
        &mut omissions,
    );
    (project, omissions)
}

#[test]
fn multi_text_authored_keys_record_each_written_owner_after_group_success() {
    let mut group = graphic_group(json!({}));
    let mut sibling = sibling_text();
    sibling["parent"] = json!(8);
    group["layers"].as_array_mut().unwrap().push(sibling);
    let entries = vec![
        entry_on(9, "opacity", &[(0, 100.0, "linear"), (500, 40.0, "linear")]),
        entry_on(10, "rotation", &[(0, 0.0, "linear"), (500, 45.0, "linear")]),
    ];
    for retained in [true, false] {
        let mut wire = editable_document();
        wire["duration"] = json!(2.0);
        let mut canvas = wire["composition"]["layers"][1].clone();
        canvas["activeRange"]["duration"] = json!(2000);
        let mut candidate = group.clone();
        if !retained {
            // The first owner's tracks must not count if another child fails.
            candidate["layers"][1]["sourceText"]["fontStyle"] = json!("Unpackaged");
        }
        wire["composition"]["layers"] = json!([candidate, canvas]);
        wire["composition"]["dynamics"] = json!({"entries": entries});
        let document = EditableFxCompositionDocument::from_json_value(wire).unwrap();
        let mut omissions = Vec::new();
        let exported = crate::convert::lower_document(
            &document,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            FrameRate::Fps30,
            &mut omissions,
        )
        .unwrap();
        assert_eq!(exported.project.is_some(), retained);
        for entry in &entries {
            let target = serde_json::from_value(entry["target"].clone()).unwrap();
            assert_eq!(
                exported.written.contains(&target),
                retained,
                "{omissions:?}"
            );
        }
    }
}

/// Export a document whose group 8 (1 s long) holds only text layer 9, with
/// `group` merged into the group's JSON and `entries` as its animation.
fn exported_group(group: Value, entries: Vec<Value>) -> (Option<PrGraphic>, Vec<Omission>) {
    let (project, omissions) = export_over_canvas(vec![graphic_group(group)], entries);
    let graphic = project.ok().and_then(|project| {
        project
            .single_sequence()
            .unwrap()
            .video_items()
            .find_map(PrVideoItem::graphic)
            .cloned()
    });
    (graphic, omissions)
}

/// A root text layer 10 over the first second, which exports as a graphic of
/// its own.
fn sibling_text() -> Value {
    json!({
        "type": "Text",
        "id": 10,
        "name": "Sibling",
        "activeRange": {"start": 0, "duration": 1000},
        "transform": {"anchorPoint": [0, 0], "position": [960, 270], "scale": [100, 100], "rotation": 0, "opacity": 100},
        "sourceText": {"text": "Stays", "fontFamily": "Inter-Bold", "fontStyle": "", "fontSize": 80, "fillColor": [1, 1, 1, 1]}
    })
}

/// Text layer 9 of [`graphic_group`] as a root layer over the second second,
/// which exports as a graphic of its own.
fn root_title() -> Value {
    let mut text = graphic_group(json!({}))["layers"][0].clone();
    text.as_object_mut().unwrap().remove("parent");
    text["activeRange"]["start"] = json!(1000);
    text
}

/// The names of the graphics that exporting `layers` next to
/// [`sibling_text`] produces, and every omission.
fn graphics_beside_sibling(layers: Vec<Value>) -> (Vec<String>, Vec<Omission>) {
    let (project, omissions) = export_over_canvas(
        layers.into_iter().chain([sibling_text()]).collect(),
        Vec::new(),
    );
    let project = project.unwrap_or_else(|error| panic!("{error}: {omissions:?}"));
    let names = project
        .single_sequence()
        .unwrap()
        .video_items()
        .filter_map(PrVideoItem::graphic)
        .map(|graphic| graphic.text().name.clone())
        .collect();
    (names, omissions)
}

/// Assert that exporting `layers` next to [`sibling_text`] omits the graphic
/// occurrence `record` with `reason`, and that the sibling still exports.
fn assert_graphic_omitted(layers: Vec<Value>, record: &str, reason: &str) {
    let (names, omissions) = graphics_beside_sibling(layers);
    assert_eq!(names, ["Sibling"], "{reason}: {omissions:?}");
    assert!(
        omissions
            .iter()
            .any(|omission| omission.scope == OmissionScope::Occurrence
                && omission.record == record
                && omission.reason == reason),
        "{reason}: {omissions:?}"
    );
}

/// [`assert_graphic_omitted`] for graphic group 8 with `group` merged into
/// its JSON, rejected by the graphic-group rule `rule`.
fn assert_group_rejected(group: Value, rule: &str) {
    assert_graphic_omitted(
        vec![graphic_group(group)],
        "layer 8 (\"Graphic\")",
        &format!("graphic group was not exported: {rule}"),
    );
}

#[test]
fn a_graphic_group_exports_beside_the_sibling_that_rejection_tests_keep() {
    // The rejection tests change one property of this pair.
    let (names, omissions) = graphics_beside_sibling(vec![graphic_group(json!({}))]);
    assert_eq!(names, ["Sibling", "Title"], "{omissions:?}");
    assert!(
        omissions
            .iter()
            .all(|omission| omission.scope != OmissionScope::Occurrence),
        "{omissions:?}"
    );
}

#[test]
fn a_graphic_group_whose_text_has_masks_is_omitted() {
    // A mask outline stored inline, as documents from before shape-layer mask
    // references keep it. A referenced outline is a second layer of the
    // group, which then exports as a nested sequence, not as a graphic.
    let mut group = graphic_group(json!({}));
    group["layers"][0]["masks"] = json!([{
        "id": 50,
        "mode": "add",
        "path": {"commands": [
            {"type": "moveTo", "x": 0, "y": 0},
            {"type": "lineTo", "x": 200, "y": 0},
            {"type": "lineTo", "x": 200, "y": 40},
            {"type": "close"}
        ]}
    }]);
    assert_graphic_omitted(
        vec![group],
        "layer 8 (\"Graphic\")",
        "graphic group was not exported: its text layer must have no masks",
    );
}

#[test]
fn a_graphic_group_whose_text_has_a_track_matte_is_omitted() {
    let mut group = graphic_group(json!({}));
    group["layers"][0]["trackMatte"] = json!({"mode": "alpha", "layer": 2});
    assert_graphic_omitted(
        vec![group],
        "layer 8 (\"Graphic\")",
        "graphic group was not exported: its text layer must have no track matte",
    );
}

#[test]
fn a_root_text_with_masks_is_omitted() {
    let mut text = root_title();
    text["masks"] = json!([{
        "id": 50,
        "mode": "add",
        "path": {"commands": [
            {"type": "moveTo", "x": 0, "y": 0},
            {"type": "lineTo", "x": 200, "y": 0},
            {"type": "lineTo", "x": 200, "y": 40},
            {"type": "close"}
        ]}
    }]);
    assert_graphic_omitted(
        vec![text],
        "layer 9 (\"Title\")",
        "graphic was not exported: its text layer must have no masks",
    );
}

#[test]
fn a_root_text_with_a_track_matte_is_omitted() {
    let mut text = root_title();
    text["trackMatte"] = json!({"mode": "alpha", "layer": 2});
    assert_graphic_omitted(
        vec![text],
        "layer 9 (\"Title\")",
        "graphic was not exported: its text layer must have no track matte",
    );
}

#[test]
fn a_graphic_group_with_effects_is_omitted() {
    assert_group_rejected(
        json!({"effects": [{"id": 1, "effect": {"type": "mosaic", "horizontalBlocks": 10.0, "verticalBlocks": 20.0}}]}),
        "a graphic has no group effects",
    );
}

#[test]
fn a_graphic_group_mask_that_is_no_clip_opacity_mask_is_omitted() {
    // Only one mask over a shape guide beside the group, at the identity,
    // is the clip Opacity mask that Premiere applies after the Vector
    // Motion: an inline outline, a guide moved off the identity, and a
    // guide inside the group, which the group's transform would move, omit
    // the graphic rather than export it unmasked.
    let outline = json!({"commands": [
        {"type": "moveTo", "x": 0, "y": 0},
        {"type": "lineTo", "x": 400, "y": 0},
        {"type": "lineTo", "x": 400, "y": 200},
        {"type": "close"}
    ]});
    assert_group_rejected(
        json!({"masks": [{"id": 51, "mode": "add", "path": outline}]}),
        "its mask cannot be exported: the mask has an inline path",
    );
    let guide = |parent: Option<u64>, position: [f64; 2]| {
        let mut guide = json!({
            "type": "Shape",
            "id": 11,
            "name": "Guide",
            "activeRange": {"start": 1000, "duration": 1000},
            "transform": {"anchorPoint": [0, 0], "position": position, "scale": [100, 100], "rotation": 0, "opacity": 100},
            "shape": {"path": outline}
        });
        if let Some(parent) = parent {
            guide["parent"] = json!(parent);
            guide["activeRange"]["start"] = json!(0);
        }
        guide
    };
    let masked = || graphic_group(json!({"masks": [{"id": 51, "mode": "add", "layer": 11}]}));
    assert_graphic_omitted(
        vec![masked(), guide(None, [10.0, 0.0])],
        "layer 8 (\"Graphic\")",
        "graphic group was not exported: its mask cannot be exported: the graphic's mask guide is not at the identity",
    );
    let mut inside = masked();
    inside["layers"]
        .as_array_mut()
        .unwrap()
        .push(guide(Some(8), [0.0, 0.0]));
    assert_graphic_omitted(
        vec![inside],
        "layer 8 (\"Graphic\")",
        "graphic group was not exported: the graphic or one of its layers is another layer's track matte, mask or text path",
    );
    // Keys on the guide's outline, which only a video clip's mask converts.
    let outline_keys = json!({
        "target": {"kind": "layer", "layerId": 11, "propertyType": "shapePath"},
        "animator": {"type": "keyframes", "enabled": true, "keyframes": [
            {"id": "outline-0", "layerTime": 0, "value": {"type": "path", "value": outline}, "easing": {"type": "linear"}},
            {"id": "outline-1", "layerTime": 500, "value": {"type": "path", "value": outline}, "easing": {"type": "linear"}}
        ]}
    });
    let (project, omissions) = export_over_canvas(
        vec![masked(), guide(None, [0.0, 0.0]), sibling_text()],
        vec![outline_keys],
    );
    let names: Vec<_> = project
        .unwrap_or_else(|error| panic!("{error}: {omissions:?}"))
        .single_sequence()
        .unwrap()
        .video_items()
        .filter_map(PrVideoItem::graphic)
        .map(|graphic| graphic.text().name.clone())
        .collect();
    assert_eq!(names, ["Sibling"], "{omissions:?}");
    assert!(
        omissions.iter().any(|omission| omission.scope == OmissionScope::Occurrence
            && omission.record == "layer 8 (\"Graphic\")"
            && omission.reason
                == "graphic group was not exported: its mask cannot be exported: the graphic's mask guide has keys"),
        "{omissions:?}"
    );
}

/// `VideoClipTrackItem:66` of `feature_graphic_masks_d_26_5.prproj`, a
/// Premiere 26.5.1 save whose checker still keeps only its package-relative
/// `RelativePath`, its two absolute media aliases removed (probe d1): a
/// Shape graphic whose static Vector Motion (Position 864, Anchor Point 960)
/// moves it 96 px left and whose clip Opacity holds one static
/// `AE.ADBE AEMask2`, the rectangle 0.4–0.52 × 0.3–0.7 of the frame. AME
/// drew that mask in the sequence frame after the Vector Motion. Probe d0
/// now converts as Mask with Shape; d2 (a mask on Vector Motion) stays omitted.
#[test]
fn adobe_graphic_clip_opacity_mask_stays_in_the_sequence_frame_and_exports_its_edits() {
    use crate::schema::{
        text::{PrPathVertex, PrShapePath},
        PrMask,
    };
    let corner = |x: f32, y: f32| PrPathVertex {
        smooth: false,
        point: [x, y],
        in_tangent: [x, y],
        out_tangent: [x, y],
    };
    let rectangle = |third: [f32; 2]| PrShapePath {
        vertices: vec![
            corner(0.4, 0.3),
            corner(0.52, 0.3),
            corner(third[0], third[1]),
            corner(0.4, 0.7),
        ],
        closed: true,
    };
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/feature_graphic_masks_d_26_5.prproj");
    let (project, omissions) = PrProjectFile::load(&path).unwrap();
    let occurrences: Vec<_> = omissions
        .iter()
        .filter(|omission| omission.scope == OmissionScope::Occurrence)
        .map(|omission| omission.reason.as_str())
        .collect();
    assert_eq!(occurrences.len(), 1, "{omissions:?}");
    let reason = "a mask on a graphic Vector Motion is not converted";
    assert!(
        occurrences
            .iter()
            .any(|occurrence| occurrence.contains(reason)),
        "{reason}: {omissions:?}"
    );
    let sequence = project.single_sequence().unwrap();
    let probe = sequence
        .video_items()
        .filter_map(PrVideoItem::graphic)
        .find(|graphic| graphic.id() == Some("VideoClipTrackItem:66"))
        .unwrap_or_else(|| panic!("probe d1 is read: {omissions:?}"));
    assert_eq!(
        probe.opacity_mask,
        Some(PrMask {
            raster: None,
            feather_keys: Vec::new(),
            expansion: 0.0,
            expansion_keys: Vec::new(),
            opacity_keys: Vec::new(),
            path: rectangle([0.52, 0.7]),
            path_keys: Vec::new(),
            feather: 0.0,
            opacity: 100.0,
            inverted: false,
        })
    );
    // The static Vector Motion folds into the Shape: its Position 900 is 804.
    assert!(probe.vector_motion.is_none());
    let [PrGraphicObject::Shape(shape)] = probe.objects.as_slice() else {
        panic!("{:?}", probe.objects);
    };
    assert_eq!(shape.transform.position, [804.0, 540.0]);

    let mut import_omissions = Vec::new();
    let mut document = premiere_to_tesseract(
        sequence,
        &project.media,
        &crate::tesseract_output::asset_ids_in_order(sequence, &project.media),
        &mut import_omissions,
    )
    .unwrap()
    .to_json_value()
    .unwrap();
    assert!(
        import_omissions
            .iter()
            .all(|omission| !omission.record.contains("VideoClipTrackItem:66")),
        "{import_omissions:?}"
    );
    let layers = document["composition"]["layers"].as_array().unwrap();
    let group = layers
        .iter()
        .find(|layer| {
            layer["masks"]
                .as_array()
                .is_some_and(|masks| !masks.is_empty())
        })
        .unwrap_or_else(|| panic!("the masked graphic group: {layers:?}"));
    assert_eq!(group["type"], "Group");
    assert_eq!(
        group["masks"],
        json!([{"id": group["masks"][0]["id"], "mode": "add", "inverted": false, "layer": group["masks"][0]["layer"], "feather": [0.0, 0.0], "expansion": 0.0, "opacity": 1.0}])
    );
    let shape = &group["layers"][0];
    assert_eq!(
        (&shape["type"], &shape["transform"]["position"]),
        (&json!("Shape"), &json!([804.0, 540.0]))
    );
    // The guide is the group's sibling at the identity, over its range: the
    // mask stays where Premiere draws it whatever moves the Shape.
    let guide = layers
        .iter()
        .find(|layer| layer["id"] == group["masks"][0]["layer"])
        .unwrap_or_else(|| panic!("the guide beside the group: {layers:?}"));
    let canvas = layers.iter().find(|layer| layer["type"] == "Rect").unwrap();
    assert_eq!(guide["type"], "Shape");
    assert_eq!(guide["parent"], group["parent"]);
    assert_eq!(guide["transform"], canvas["transform"], "identity");
    assert_eq!(
        crate::test_support::layer_range(guide),
        crate::test_support::layer_range(group)
    );
    let pixel = |kind: &str, x: f32, y: f32| json!({"type": kind, "x": f64::from(x) * 1920.0, "y": f64::from(y) * 1080.0});
    assert_eq!(
        guide["shape"],
        json!({"path": {"commands": [
            pixel("moveTo", 0.4, 0.3),
            pixel("lineTo", 0.52, 0.3),
            pixel("lineTo", 0.52, 0.7),
            pixel("lineTo", 0.4, 0.7),
            {"type": "close"}
        ]}})
    );

    // Edit the mask in the document: move its third vertex, invert and
    // feather it. Export writes the edited guide and controls.
    let (group_id, guide_id) = (group["id"].clone(), guide["id"].clone());
    let layers = document["composition"]["layers"].as_array_mut().unwrap();
    layers.retain(|layer| {
        layer["id"] == group_id || layer["id"] == guide_id || layer["type"] == "Rect"
    });
    for layer in layers.iter_mut() {
        if layer["id"] == guide_id {
            layer["shape"]["path"]["commands"][2] =
                json!({"type": "lineTo", "x": 1200.0, "y": 810.0});
        } else if layer["id"] == group_id {
            layer["masks"][0]["inverted"] = json!(true);
            layer["masks"][0]["feather"] = json!([6.0, 6.0]);
        }
    }
    let document = EditableFxCompositionDocument::from_json_value(document).unwrap();
    let mut export_omissions = Vec::new();
    let exported = tesseract_to_premiere(
        &document,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        FrameRate::Fps30,
        &mut export_omissions,
    )
    .unwrap_or_else(|error| panic!("{error}: {export_omissions:?}"));
    assert!(
        export_omissions
            .iter()
            .all(|omission| omission.scope != OmissionScope::Occurrence),
        "{export_omissions:?}"
    );
    assert!(
        export_omissions
            .iter()
            .any(|omission| omission.reason == crate::schema::MASK_FEATHER_APPROXIMATION),
        "{export_omissions:?}"
    );
    let edited = PrMask {
        raster: None,
        feather_keys: Vec::new(),
        expansion: 0.0,
        expansion_keys: Vec::new(),
        opacity_keys: Vec::new(),
        path: rectangle([0.625, 0.75]),
        path_keys: Vec::new(),
        feather: 6.0,
        opacity: 100.0,
        inverted: true,
    };
    // The written clip Opacity mask reads back as written.
    let directory = tempfile::tempdir().unwrap();
    let written = directory.path().join("edited.prproj");
    PremiereProjectXml::new(&exported)
        .unwrap()
        .write_new(&written)
        .unwrap();
    let (reopened, _) = PrProjectFile::load(&written).unwrap();
    for project in [&exported, &reopened] {
        let graphics: Vec<_> = project.sequences[0]
            .video_items()
            .filter_map(PrVideoItem::graphic)
            .collect();
        let [graphic] = graphics.as_slice() else {
            panic!("one graphic, its guide consumed: {graphics:?}");
        };
        assert_eq!(graphic.opacity_mask.as_ref(), Some(&edited));
        assert!(matches!(
            graphic.objects.as_slice(),
            [PrGraphicObject::Shape(_)]
        ));
    }
}

#[test]
fn a_graphic_group_with_a_track_matte_is_omitted() {
    assert_group_rejected(
        json!({"trackMatte": {"mode": "alpha", "layer": 2}}),
        "a graphic has no group track matte",
    );
}

#[test]
fn a_text_background_is_a_group_box_in_both_directions() {
    use crate::schema::text::{PrRgb, PrTextBackground};
    let background = PrTextBackground {
        color: PrRgb([255, 0, 0]),
        opacity: 100.0,
        size: 10.0,
        radius: 15.0,
    };
    // Import: the text's group draws the box, padded by the size on every
    // side, filled with the color and rounded by the radius on every corner.
    let mut graphic = text_graphic();
    graphic.text_mut().document.size = 48.0;
    graphic.text_mut().document.background = Some(background);
    let (document, omissions) = imported(graphic);
    // Only the unpackaged font is reported.
    assert_eq!(omissions.len(), 1, "{omissions:?}");
    let group = &document["composition"]["layers"][0];
    assert_eq!(group["type"], "Group");
    for side in ["Top", "Right", "Bottom", "Left"] {
        assert_eq!(group[&format!("padding{side}")], 10.0, "{side}");
    }
    for corner in ["TopLeft", "TopRight", "BottomRight", "BottomLeft"] {
        assert_eq!(group[&format!("cornerRadius{corner}")], 15.0, "{corner}");
    }
    assert_eq!(
        group["fills"],
        json!([{"paint": {"type": "solid", "color": [1.0, 0.0, 0.0, 1.0]}, "fillRule": "nonZeroWinding", "blendMode": "normal", "opacity": 1.0}])
    );
    assert_eq!(group["layers"][0]["type"], "Text");
    assert_eq!(group["layers"][0]["sourceText"]["fontSize"], 48.0);

    // Export: the group of one text keeps the box as the text's background in
    // the calibrated form; any other box is reported and the text exports.
    let solid = |color: [f64; 4]| json!({"paint": {"type": "solid", "color": color}});
    let box_ = |padding: f64, radius: f64, fills: Value| {
        json!({
            "paddingTop": padding, "paddingRight": padding, "paddingBottom": padding, "paddingLeft": padding,
            "cornerRadiusTopLeft": radius, "cornerRadiusTopRight": radius,
            "cornerRadiusBottomRight": radius, "cornerRadiusBottomLeft": radius,
            "fills": fills,
        })
    };
    let red = || json!([solid([1.0, 0.0, 0.0, 1.0])]);
    let exported_text = |group: Value, size: f64, entries: Vec<Value>| {
        let mut group = graphic_group(group);
        group["layers"][0]["sourceText"]["fontSize"] = json!(size);
        let (graphic, omissions) = exported_group(group, entries);
        let graphic = graphic.expect("the text exports");
        (graphic.text().document.clone(), omissions)
    };
    // Accepted: an unscaled, unrotated, opaque text without keys, as a point
    // text at the group's centre and as the fixture's bottom box text (C4),
    // whose anchor and position only translate text and box together.
    let c4_child = json!({"layers": [{
        "type": "Text", "id": 9, "name": "C3 caption 1", "parent": 8,
        "activeRange": {"start": 0, "duration": 1000},
        "transform": {"anchorPoint": [768, 969.6], "position": [960, 1026], "scale": [100, 100], "rotation": 0, "opacity": 100},
        "sourceText": {
            "text": "C4 BACKGROUND", "fontFamily": "Arial-BoldMT", "fontStyle": "", "fontSize": 48,
            "fillColor": [1, 1, 1, 1], "justification": "center", "boxSize": [1536, 998], "verticalAlign": "bottom"
        }
    }]});
    for child in [json!({}), c4_child] {
        let mut group = box_(10.0, 15.0, red());
        for (key, value) in child.as_object().unwrap() {
            group[key] = value.clone();
        }
        let (exported, omissions) = exported_text(group, 48.0, Vec::new());
        assert!(omissions.is_empty(), "{omissions:?}");
        assert_eq!(exported.background, Some(background));
    }
    // The group's own Vector Motion scales box and text together and keeps
    // the background; the text child's own transform and keys must be the
    // calibrated neutral ones.
    let (exported, omissions) = exported_text(
        {
            let mut group = box_(10.0, 15.0, red());
            group["transform"] = json!({"anchorPoint": [960, 540], "position": [960, 540], "scale": [200, 200], "rotation": 45, "opacity": 100});
            group
        },
        48.0,
        Vec::new(),
    );
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(exported.background, Some(background));
    let child_transform = |transform: Value| {
        let mut group = box_(10.0, 15.0, red());
        group["layers"] = json!([{
            "type": "Text", "id": 9, "name": "Title", "parent": 8,
            "activeRange": {"start": 0, "duration": 1000},
            "transform": transform,
            "sourceText": {"text": "Keys", "fontFamily": "Inter-Bold", "fontStyle": "", "fontSize": 80, "fillColor": [1, 1, 1, 1]}
        }]);
        group
    };
    for (group, size, entries, reason) in [
        (
            child_transform(json!({"anchorPoint": [0, 0], "position": [960, 540], "scale": [200, 200], "rotation": 0, "opacity": 100})),
            48.0,
            Vec::new(),
            "its text is scaled or rotated",
        ),
        (
            child_transform(json!({"anchorPoint": [0, 0], "position": [960, 540], "scale": [100, 100], "rotation": 45, "opacity": 100})),
            48.0,
            Vec::new(),
            "its text is scaled or rotated",
        ),
        (
            box_(10.0, 15.0, red()),
            48.0,
            vec![
                entry("scaleX", &[(0, 100.0, "linear"), (500, 150.0, "hold")]),
                entry("scaleY", &[(0, 100.0, "linear"), (500, 150.0, "hold")]),
            ],
            "its text has Scale or Rotation keys",
        ),
        (
            box_(10.0, 15.0, red()),
            48.0,
            vec![
                entry("positionX", &[(0, 960.0, "linear"), (500, 1056.0, "hold")]),
                entry("positionY", &[(0, 540.0, "linear"), (500, 540.0, "hold")]),
            ],
            "its text has keys",
        ),
        (
            child_transform(json!({"anchorPoint": [0, 0], "position": [960, 540], "scale": [100, 100], "rotation": 0, "opacity": 50})),
            48.0,
            Vec::new(),
            "its text is not opaque",
        ),
        (
            json!({"paddingTop": 10.0}),
            48.0,
            Vec::new(),
            "a text background has one size on every side",
        ),
        (
            json!({"cornerRadiusTopLeft": 15.0}),
            48.0,
            Vec::new(),
            "a text background has one radius on every corner",
        ),
        (
            box_(10.0, 15.0, json!([solid([1.0, 0.0, 0.0, 1.0]), solid([0.0, 0.0, 1.0, 1.0])])),
            48.0,
            Vec::new(),
            "a text background has one fill",
        ),
        (
            box_(10.0, 15.0, json!([{"paint": {"type": "solid", "color": [1.0, 0.0, 0.0, 1.0]}, "opacity": 0.5}])),
            48.0,
            Vec::new(),
            "a text background fill blends normally at full opacity",
        ),
        (
            box_(10.0, 15.0, red()),
            80.0,
            Vec::new(),
            "the box is calibrated at 48 px text only, not 80 px",
        ),
        (
            box_(10.0, 40.0, red()),
            48.0,
            Vec::new(),
            "how Premiere clamps a corner radius (40) above half the box height (at least 22) is unknown",
        ),
    ] {
        let (exported, omissions) = exported_text(group, size, entries);
        assert_eq!(exported.background, None, "{reason}");
        assert_eq!(
            omissions,
            [Omission {
                scope: OmissionScope::Feature,
                kind: OmissionKind::Omitted,
                record: "layer 8 (\"Graphic\")".into(),
                reason: format!(
                    "group background was not exported: unsupported conversion: {reason}"
                ),
            }]
        );
    }
    // A box around several objects is not a text background.
    let mut several = graphic_group(json!({"paddingTop": 10.0}));
    let mut second = several["layers"][0].clone();
    second["id"] = json!(11);
    second["name"] = json!("Second");
    several["layers"].as_array_mut().unwrap().push(second);
    assert_graphic_omitted(
        vec![several],
        "layer 8 (\"Graphic\")",
        "graphic group was not exported: a graphic has no group background",
    );
}

#[test]
fn a_graphic_group_with_playback_is_omitted() {
    assert_group_rejected(
        json!({"playback": crate::test_support::remapped_playback(json!({"start": 1000, "duration": 1000}), json!({
            "keyframes": [
                {"id": "a", "time": 0, "value": 0, "easing": {"type": "linear"}},
                {"id": "b", "time": 1000, "value": 500, "easing": {"type": "linear"}}
            ],
            "before": "inactive",
            "after": "inactive"
        }))}),
        "graphic time remapping is unsupported",
    );
}

#[test]
fn group_motion_blur_preserves_graphic_title_and_healthy_sibling() {
    use crate::export_loss::{
        ExportField, ExportLossDomain, ExportLossKind, ExportLossSource, LossCollector,
    };

    let mut wire = editable_document();
    wire["duration"] = json!(2.0);
    let mut canvas = wire["composition"]["layers"][1].clone();
    canvas["activeRange"]["duration"] = json!(2000);
    wire["composition"]["layers"] = json!([
        graphic_group(json!({"motionBlur": true})),
        sibling_text(),
        canvas
    ]);
    let document = EditableFxCompositionDocument::from_json_value(wire).unwrap();
    let mut losses = LossCollector::default();
    let exported = crate::convert::lower_document(
        &document,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        FrameRate::Fps30,
        &mut losses,
    )
    .unwrap();
    let report = losses.finish(exported.project.is_some());
    let project = exported.project.unwrap();
    let graphics: Vec<_> = project
        .single_sequence()
        .unwrap()
        .video_items()
        .filter_map(PrVideoItem::graphic)
        .collect();
    let names: Vec<_> = graphics
        .iter()
        .map(|graphic| graphic.text().name.as_str())
        .collect();
    assert_eq!(names, ["Sibling", "Title"], "{report:?}");
    assert_eq!(graphics[1].text().document.text, "Keys");
    assert_eq!(report.losses.len(), 1, "{report:?}");
    let loss = &report.losses[0];
    assert_eq!(loss.source, ExportLossSource::Layer(LayerId::new(8)));
    assert_eq!(loss.domain, ExportLossDomain::Picture);
    assert_eq!(loss.kind, ExportLossKind::Field(ExportField::MotionBlur));
    assert_eq!(loss.omission.scope, OmissionScope::Feature);
    assert_eq!(loss.omission.kind, OmissionKind::Omitted);
    assert_eq!(loss.omission.record, "layer 8 (\"Graphic\")");
    assert!(
        report
            .diagnostics
            .iter()
            .all(|omission| omission.scope != OmissionScope::Occurrence),
        "{report:?}"
    );
}

#[test]
fn a_graphic_group_with_skew_or_3d_rotation_is_omitted() {
    let base = graphic_group(json!({}))["transform"].clone();
    for (field, value) in [
        ("skew", json!(10.0)),
        ("skewAxis", json!(30.0)),
        ("rotationX", json!(15.0)),
        ("rotationY", json!(15.0)),
        ("orientation", json!([0.0, 0.0, 10.0])),
        ("position", json!([960.0, 540.0, 100.0])),
    ] {
        let mut transform = base.clone();
        transform[field] = value;
        assert_group_rejected(
            json!({"transform": transform}),
            "Vector Motion has no skew or 3D rotation",
        );
    }
}

#[test]
fn a_graphic_group_inside_another_group_exports_into_its_nest() {
    // Export accepts the outer group as a nest and writes the graphic inside
    // it into the nest's sequence: a graphic group, or a text of the nest's
    // own beside a solid, as import makes it, or the Shape of a rectangle
    // that is no Color Matte. No native nested graphic is measured: this form
    // is structurally tested only.
    let graphic = graphic_group(
        json!({"parent": 7, "playback": crate::test_support::linear_playback(json!({"start": 0, "duration": 1000}), json!({"start": 0, "duration": 1000}))}),
    );
    let mut text = graphic["layers"][0].clone();
    text["parent"] = json!(7);
    let mut solid = editable_document()["composition"]["layers"][1].clone();
    solid["id"] = json!(10);
    solid["parent"] = json!(7);
    solid["activeRange"] = json!({"start": 0, "duration": 1000});
    let mut bar = solid.clone();
    bar["rect"]["size"] = json!([400, 100]);
    for children in [vec![graphic], vec![text, solid], vec![bar]] {
        let outer = json!({
            "type": "Group",
            "id": 7,
            "name": "Outer",
            "playback": crate::test_support::linear_playback(json!({"start": 1000, "duration": 1000}), json!({"start": 0, "duration": 1000})),
            "transform": {"anchorPoint": [0, 0], "position": [0, 0], "scale": [100, 100], "rotation": 0, "opacity": 100},
            "layers": children,
        });
        let (project, omissions) = export_over_canvas(vec![outer], Vec::new());
        let project = project.unwrap();
        let nest = project
            .single_sequence()
            .unwrap()
            .nest_occurrences()
            .next()
            .expect("the outer group exports as a nest");
        let graphics: Vec<_> = nest
            .sequence
            .video_items()
            .filter_map(PrVideoItem::graphic)
            .collect();
        assert_eq!(graphics.len(), 1, "{omissions:?}");
        assert_eq!((graphics[0].start_ticks, graphics[0].end_ticks), (0, TICKS));
        assert!(omissions.is_empty(), "{omissions:?}");
    }
}

#[test]
fn a_graphic_group_whose_text_ends_before_it_is_omitted() {
    let mut group = graphic_group(json!({}));
    group["layers"][0]["activeRange"] = json!({"start": 0, "duration": 500});
    assert_graphic_omitted(
        vec![group],
        "layer 8 (\"Graphic\")",
        "graphic group was not exported: its text layer must span the group",
    );
}

/// A track entry on layer `layer` in FX JSON, with key ids unique to it.
fn entry_on(layer: u64, property: &str, keys: &[(i64, f64, &str)]) -> Value {
    let mut entry = entry(property, keys);
    entry["target"]["layerId"] = json!(layer);
    for (index, key) in entry["animator"]["keyframes"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        key["id"] = json!(format!("{property}-{layer}-{index}"));
    }
    entry
}

#[test]
fn a_keyed_graphic_group_exports_as_keyed_vector_motion() {
    use PrKeyframeEasing::{Hold, Linear};
    let (graphic, omissions) = exported_group(
        json!({}),
        vec![
            entry_on(
                8,
                "positionX",
                &[(0, 960.0, "linear"), (500, 1056.0, "hold")],
            ),
            entry_on(
                8,
                "positionY",
                &[(0, 540.0, "linear"), (500, 486.0, "hold")],
            ),
            entry_on(8, "scaleX", &[(0, 100.0, "linear"), (1000, 70.0, "linear")]),
            entry_on(8, "scaleY", &[(0, 100.0, "linear"), (1000, 70.0, "linear")]),
            entry_on(8, "rotation", &[(0, 0.0, "linear"), (1000, 20.0, "hold")]),
            entry_on(
                9,
                "opacity",
                &[(0, 100.0, "linear"), (1000, 40.0, "linear")],
            ),
        ],
    );
    assert!(omissions.is_empty(), "{omissions:?}");
    let graphic = graphic.unwrap();
    // The graphic takes the group's range; both key clocks start there.
    assert_eq!(graphic.timeline_ticks(), TICKS..2 * TICKS);
    let at = |millis: i64| EXPORT_IN + millis * TICKS / 1000;
    let motion = graphic.vector_motion.clone().unwrap();
    assert_eq!(
        (motion.position, motion.anchor),
        ([960.0, 540.0], [768.0, 486.0])
    );
    assert_eq!(
        motion.animations,
        [
            PrPropertyAnimation::Rotation(vec![
                scalar(at(0), 0.0, Linear),
                scalar(at(1000), 20.0, Hold),
            ]),
            PrPropertyAnimation::Position(vec![
                PrPointKeyframe {
                    source_ticks: at(0),
                    value: [0.5, 0.5],
                    easing: Linear,
                    spatial_in_tangent: None,
                    spatial_out_tangent: None,
                },
                PrPointKeyframe {
                    source_ticks: at(500),
                    value: [0.55, 0.45],
                    easing: Hold,
                    spatial_in_tangent: None,
                    spatial_out_tangent: None,
                },
            ]),
            PrPropertyAnimation::UniformScale(vec![
                scalar(at(0), 100.0, Linear),
                scalar(at(1000), 70.0, Linear),
            ]),
        ]
    );
    assert_eq!(
        graphic.text().animations,
        [PrPropertyAnimation::Opacity(vec![
            scalar(at(0), 100.0, Linear),
            scalar(at(1000), 40.0, Linear),
        ])]
    );
    assert_eq!(graphic.text().transform.position, [960.0, 540.0]);
}

#[test]
fn a_static_graphic_group_is_composed_into_the_text() {
    let (graphic, omissions) = exported_group(
        json!({"transform": {"anchorPoint": [960, 540], "position": [480, 270], "scale": [50, 50], "rotation": 90, "opacity": 100}}),
        vec![entry_on(
            9,
            "rotation",
            &[(0, 0.0, "linear"), (500, 10.0, "linear")],
        )],
    );
    assert!(omissions.is_empty(), "{omissions:?}");
    let graphic = graphic.unwrap();
    assert!(graphic.vector_motion.is_none());
    let transform = graphic.text().transform;
    assert!(
        (transform.position[0] - 480.0).abs() < 1e-9
            && (transform.position[1] - 270.0).abs() < 1e-9
    );
    assert_eq!((transform.scale, transform.rotation), (50.0, 90.0));
    let rotation = graphic.text().animations[0].keys();
    assert_eq!(
        rotation.iter().map(|key| key.value).collect::<Vec<_>>(),
        [90.0, 100.0]
    );
}

#[test]
fn a_static_graphic_group_stays_vector_motion_when_folding_leaves_the_text_range() {
    // Folding 200% into a 3000% text would give 6000%, above Text Scale's
    // 4000%: the group exports as a static Vector Motion and reads back so.
    let mut group = graphic_group(
        json!({"transform": {"anchorPoint": [960, 540], "position": [960, 540], "scale": [200, 200], "rotation": 0, "opacity": 100}}),
    );
    group["layers"][0]["transform"]["scale"] = json!([3000, 3000]);
    let (project, omissions) = export_over_canvas(vec![group], Vec::new());
    let project = project.unwrap_or_else(|error| panic!("{error}: {omissions:?}"));
    assert!(omissions.is_empty(), "{omissions:?}");
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("written.prproj");
    PremiereProjectXml::new(&project)
        .unwrap()
        .write_new(&path)
        .unwrap();
    let (loaded, _) = PrProjectFile::load(&path).unwrap();
    let graphic = loaded.sequences[0]
        .video_items()
        .find_map(PrVideoItem::graphic)
        .expect("the written graphic loads")
        .clone();
    let motion = graphic
        .vector_motion
        .as_ref()
        .expect("a static Vector Motion");
    assert_eq!((motion.scale, motion.animations.len()), (200.0, 0));
    assert_eq!(graphic.text().transform.scale, 3000.0);
    let (document, omissions) = imported(graphic);
    assert!(
        omissions
            .iter()
            .all(|omission| omission.scope != OmissionScope::Occurrence),
        "{omissions:?}"
    );
    let group = &document["composition"]["layers"][0];
    assert_eq!(group["transform"]["scale"], json!([200.0, 200.0]));
    assert_eq!(
        group["layers"][0]["transform"]["scale"],
        json!([3000.0, 3000.0])
    );
}

#[test]
fn group_properties_a_graphic_cannot_carry_omit_the_graphic() {
    for (group, entries, reason) in [
        (
            json!({"transform": {"anchorPoint": [0, 0], "position": [0, 0], "scale": [100, 50], "rotation": 0, "opacity": 100}}),
            Vec::new(),
            "Vector Motion scale must be uniform",
        ),
        (
            json!({"layers": [{
                "type": "Text", "id": 9, "name": "Title", "parent": 8,
                "activeRange": {"start": 500, "duration": 500},
                "transform": {"anchorPoint": [0, 0], "position": [960, 540], "scale": [100, 100], "rotation": 0, "opacity": 100},
                "sourceText": {"text": "Keys", "fontFamily": "Inter-Bold", "fontStyle": "", "fontSize": 80, "fillColor": [1, 1, 1, 1]}
            }]}),
            Vec::new(),
            "its text layer must span the group",
        ),
    ] {
        let (graphic, omissions) = exported_group(group, entries);
        assert!(graphic.is_none(), "{reason}");
        assert!(
            omissions
                .iter()
                .any(|omission| omission.scope == OmissionScope::Occurrence
                    && omission.record == "layer 8 (\"Graphic\")"
                    && omission.reason == format!("graphic group was not exported: {reason}")),
            "{reason}: {omissions:?}"
        );
    }
    // A hidden group exports a disabled graphic.
    let (graphic, _) = exported_group(json!({"isHidden": true}), Vec::new());
    assert!(!graphic.unwrap().enabled);
}

/// `text_graphic` with the clip Opacity keys of case B (Linear, Hold, then a
/// last key), half a second apart from half a second into the placement.
fn clip_opacity_graphic() -> PrGraphic {
    use PrKeyframeEasing::{Hold, Linear};
    let mut graphic = keyed_graphic();
    let start = graphic.in_ticks;
    graphic.animations = vec![PrPropertyAnimation::Opacity(vec![
        scalar(start + TICKS / 2, 100.0, Linear),
        scalar(start + TICKS, 0.0, Linear),
        scalar(start + 3 * TICKS / 2, 70.0, Hold),
    ])];
    graphic
}

#[test]
fn a_nondefault_clip_opacity_becomes_the_opacity_of_a_graphic_group() {
    let (document, omissions) = imported(clip_opacity_graphic());
    assert_eq!(omissions.len(), 1, "{omissions:?}");
    let layers = document["composition"]["layers"].as_array().unwrap();
    let types: Vec<_> = layers.iter().map(|layer| &layer["type"]).collect();
    assert_eq!(types, ["Group", "Video", "Rect"]);
    // The clip Opacity fades the whole graphic, so the group carries it; the
    // group is otherwise an identity transform, and the text keeps its own
    // transform and keys, its Opacity included.
    let group = &layers[0];
    assert_eq!(group["id"], 3);
    assert_eq!(
        (*crate::test_support::layer_range(group)),
        json!({"start": 1000, "duration": 2000})
    );
    assert_eq!(group["transform"]["position"], json!([0.0, 0.0]));
    assert_eq!(group["transform"]["anchorPoint"], json!([0.0, 0.0]));
    assert_eq!(group["transform"]["scale"], json!([100.0, 100.0]));
    assert_eq!(group["transform"]["opacity"], json!(100.0));
    assert_eq!(
        track(&document, 3, "opacity"),
        keys(&[
            (500, 100.0, "linear"),
            (1000, 0.0, "linear"),
            (1500, 70.0, "hold")
        ])
    );
    let text = &group["layers"][0];
    assert_eq!((&text["id"], &text["parent"]), (&json!(2), &json!(3)));
    assert_eq!(text["transform"]["position"], json!([480.0, 540.0]));
    assert_eq!(
        track(&document, 2, "opacity"),
        keys(&[(500, 75.0, "linear"), (1500, 40.0, "linear")])
    );

    // A static clip Opacity makes the group too, and default values do not.
    let mut graphic = text_graphic();
    graphic.opacity = 50.0;
    let (document, _) = imported(graphic);
    let group = &document["composition"]["layers"][0];
    assert_eq!(
        (&group["type"], &group["transform"]["opacity"]),
        (&json!("Group"), &json!(50.0))
    );
    let (document, _) = imported(text_graphic());
    assert_eq!(document["composition"]["layers"][0]["type"], "Text");
}

#[test]
fn a_graphic_group_opacity_exports_as_the_clip_opacity() {
    use PrKeyframeEasing::{Hold, Linear};
    let at = |millis: i64| EXPORT_IN + millis * TICKS / 1000;
    // A static group is composed into the text, and its opacity is the clip's.
    let (graphic, omissions) = exported_group(
        json!({"transform": {"anchorPoint": [0, 0], "position": [0, 0], "scale": [100, 100], "rotation": 0, "opacity": 50}}),
        Vec::new(),
    );
    assert!(omissions.is_empty(), "{omissions:?}");
    let graphic = graphic.unwrap();
    assert!(graphic.vector_motion.is_none() && graphic.animations.is_empty());
    assert_eq!(
        (graphic.opacity, graphic.text().transform.opacity),
        (50.0, 100.0)
    );
    // Group opacity keys become clip Opacity keys on the generator clock.
    let (graphic, omissions) = exported_group(
        json!({}),
        vec![entry_on(
            8,
            "opacity",
            &[
                (0, 100.0, "linear"),
                (500, 0.0, "hold"),
                (1000, 70.0, "linear"),
            ],
        )],
    );
    assert!(omissions.is_empty(), "{omissions:?}");
    let graphic = graphic.unwrap();
    assert_eq!(graphic.opacity, 100.0);
    assert_eq!(
        graphic.animations,
        [PrPropertyAnimation::Opacity(vec![
            scalar(at(0), 100.0, Linear),
            scalar(at(500), 0.0, Hold),
            scalar(at(1000), 70.0, Linear),
        ])]
    );
    // Keys that Premiere cannot hold are reported, and the static value stays.
    for (entry, reason) in [
        (
            with_bezier(
                entry_on(8, "opacity", &[(0, 60.0, "linear"), (500, 60.0, "linear")]),
                1,
                [0.5, 0.0, 0.5, 1.0],
            ),
            "Opacity cubic easing between equal values cannot preserve Premiere velocity",
        ),
        (
            entry_on(
                8,
                "opacity",
                &[(0, 100.0, "linear"), (500, 120.0, "linear")],
            ),
            "Opacity keys must stay within Premiere's range 0..100",
        ),
    ] {
        let (graphic, omissions) = exported_group(json!({}), vec![entry]);
        let graphic = graphic.unwrap_or_else(|| panic!("{reason}: {omissions:?}"));
        assert!(graphic.animations.is_empty(), "{reason}");
        assert_eq!(
            omissions
                .iter()
                .map(|omission| (
                    omission.scope,
                    omission.record.as_str(),
                    omission.reason.clone()
                ))
                .collect::<Vec<_>>(),
            [(
                OmissionScope::Feature,
                "layer 8 (\"Graphic\")",
                format!("Opacity animation was not exported: unsupported conversion: {reason}")
            )]
        );
    }
}

#[test]
fn a_bezier_curve_into_a_key_that_starts_a_hold_is_reported_and_the_graphic_keeps_its_static_values(
) {
    use PrKeyframeEasing::{CubicBezier, Hold, Linear};
    let at = |millis: i64| EXPORT_IN + millis * TICKS / 1000;
    // The key at 500 ms starts a Hold. Premiere ignores its in-handle, so the
    // text's keys and the group's clip Opacity keys that ease into it any
    // other way than with a zero-length handle are reported, and the static
    // values export.
    let into_hold = |layer, arrival| {
        with_bezier(
            entry_on(
                layer,
                "opacity",
                &[
                    (0, 100.0, "linear"),
                    (500, 20.0, "linear"),
                    (1000, 60.0, "hold"),
                ],
            ),
            1,
            arrival,
        )
    };
    let reason = "Opacity animation was not exported: unsupported conversion: Opacity cubic easing into a key that starts a Hold cannot keep its in-handle, which Premiere ignores";
    let (graphic, omissions) = exported(vec![into_hold(9, [0.4, 0.1, 0.75, 0.95])], None);
    assert!(graphic.text().animations.is_empty());
    assert_eq!(graphic.text().transform.opacity, 100.0);
    assert_eq!(
        omissions
            .iter()
            .map(|omission| (
                omission.scope,
                omission.record.as_str(),
                omission.reason.as_str()
            ))
            .collect::<Vec<_>>(),
        [(OmissionScope::Feature, "layer 9 (\"Title\")", reason)]
    );
    let (graphic, omissions) =
        exported_group(json!({}), vec![into_hold(8, [0.4, 0.1, 0.75, 0.95])]);
    let graphic = graphic.unwrap_or_else(|| panic!("{omissions:?}"));
    assert!(graphic.animations.is_empty());
    assert_eq!(graphic.opacity, 100.0);
    assert_eq!(
        omissions
            .iter()
            .map(|omission| (
                omission.scope,
                omission.record.as_str(),
                omission.reason.as_str()
            ))
            .collect::<Vec<_>>(),
        [(OmissionScope::Feature, "layer 8 (\"Graphic\")", reason)]
    );
    // A curve that already arrives with a zero-length handle, or a straight
    // one, is drawn the same and exports as before.
    for arrival in [[0.4, 0.1, 1.0, 1.0], [0.3, 0.3, 0.7, 0.7]] {
        let (graphic, omissions) = exported_group(json!({}), vec![into_hold(8, arrival)]);
        assert!(omissions.is_empty(), "{arrival:?}: {omissions:?}");
        let [x1, y1, x2, y2] = arrival;
        assert_eq!(
            graphic.unwrap().animations,
            [PrPropertyAnimation::Opacity(vec![
                scalar(at(0), 100.0, Linear),
                scalar(at(500), 20.0, CubicBezier { x1, y1, x2, y2 }),
                scalar(at(1000), 60.0, Hold),
            ])]
        );
    }
}

#[test]
fn graphic_clip_opacity_keys_are_written_with_the_probed_bezier_speeds_and_read_back() {
    // The Bezier probe's clip Opacity segment (100 to 20 over 1.2 s, the
    // Text Opacity handles) on graphic group 8, after a Linear segment that
    // writes the key between them with no incoming handle.
    let (key_lists, graphic, omissions) = written_graphic(vec![with_bezier(
        entry_on(
            8,
            "opacity",
            &[
                (0, 100.0, "linear"),
                (500, 100.0, "linear"),
                (1700, 20.0, "linear"),
            ],
        ),
        2,
        [0.5, 0.07875, 0.75, 0.94],
    )]);
    assert!(
        omissions
            .iter()
            .all(|omission| !omission.reason.contains("animation")),
        "{omissions:?}"
    );
    let written = key_lists
        .iter()
        .find(|keys| keys.contains(&format!("{},100,5,0,0,0,", EXPORT_IN + TICKS / 2)))
        .unwrap_or_else(|| panic!("the clip Opacity keys: {key_lists:?}"));
    let fields: Vec<Vec<f64>> = written
        .split_terminator(';')
        .map(|key| key.split(',').map(|field| field.parse().unwrap()).collect())
        .collect();
    // The speeds and influences that Premiere saved for the probe's segment.
    for (actual, expected) in [
        (fields[1][6], -10.5),
        (fields[1][7], 0.5),
        (fields[2][4], -16.0),
        (fields[2][5], 0.25),
    ] {
        assert!((actual - expected).abs() < 1e-9, "{written}");
    }
    assert_eq!(graphic.opacity, 100.0);
    let keys = graphic.animations[0].keys();
    assert_eq!(keys.len(), 3);
    assert_eq!(keys[1].easing, PrKeyframeEasing::Linear);
    let PrKeyframeEasing::CubicBezier { x1, y1, x2, y2 } = keys[2].easing else {
        panic!("{:?}", keys[2].easing);
    };
    for (actual, expected) in [(x1, 0.5), (y1, 0.07875), (x2, 0.75), (y2, 0.94)] {
        assert!((actual - expected).abs() < 1e-12, "{:?}", keys[2].easing);
    }
}

#[test]
fn graphic_tracks_with_unverified_bezier_easing_are_reported_and_keep_their_static_values() {
    use PrKeyframeEasing::{Hold, Linear};
    // Position and Text Rotation, whose Bezier speeds were not measured.
    // `entry` with cubic Bezier easing on key `index`.
    let bezier = |mut entry: Value, index: usize| {
        entry["animator"]["keyframes"][index]["easing"] =
            json!({"type": "cubicBezier", "x1": 0.3, "y1": 0.0, "x2": 0.4, "y2": 1.0});
        entry
    };
    let reported = |omissions: &[Omission]| {
        omissions
            .iter()
            .map(|omission| {
                (
                    omission.scope,
                    omission.record.clone(),
                    omission.reason.clone(),
                )
            })
            .collect::<Vec<_>>()
    };
    let bezier_reason = |record: &str, property: &str| {
        (
            OmissionScope::Feature,
            record.to_owned(),
            format!(
                "{property} animation was not exported: Bezier graphic keys are unsupported until their speed unit is verified"
            ),
        )
    };
    let text = "layer 9 (\"Title\")";
    // Text tracks: Bezier Position and Rotation keep the static text, and
    // Hold Opacity still exports.
    let (graphic, omissions) = exported(
        vec![
            entry("opacity", &[(0, 100.0, "linear"), (500, 40.0, "hold")]),
            bezier(
                entry("positionX", &[(0, 480.0, "linear"), (500, 960.0, "linear")]),
                1,
            ),
            bezier(
                entry("positionY", &[(0, 270.0, "linear"), (500, 540.0, "linear")]),
                1,
            ),
            bezier(
                entry("rotation", &[(0, 0.0, "linear"), (500, 45.0, "linear")]),
                1,
            ),
        ],
        None,
    );
    assert_eq!(
        reported(&omissions),
        [
            bezier_reason(text, "Rotation"),
            bezier_reason(text, "Position"),
        ]
    );
    let at = |millis: i64| EXPORT_IN + millis * TICKS / 1000;
    assert_eq!(
        graphic.text().animations,
        [PrPropertyAnimation::Opacity(vec![
            scalar(at(0), 100.0, Linear),
            scalar(at(500), 40.0, Hold),
        ])]
    );
    let (unkeyed, _) = exported(Vec::new(), None);
    assert_eq!(graphic.text().transform, unkeyed.text().transform);
    // Any key counts, including the first, whose easing shapes no segment.
    let (graphic, omissions) = exported(
        vec![bezier(
            entry("rotation", &[(0, 0.0, "linear"), (500, 45.0, "linear")]),
            0,
        )],
        None,
    );
    assert!(graphic.text().animations.is_empty());
    assert_eq!(reported(&omissions), [bezier_reason(text, "Rotation")]);
    // Vector Motion tracks: Bezier Position keeps the group's static
    // position, and Linear Scale still makes keyed Vector Motion.
    let group = "layer 8 (\"Graphic\")";
    let (graphic, omissions) = exported_group(
        json!({}),
        vec![
            bezier(
                entry_on(
                    8,
                    "positionX",
                    &[(0, 960.0, "linear"), (500, 1056.0, "linear")],
                ),
                1,
            ),
            bezier(
                entry_on(
                    8,
                    "positionY",
                    &[(0, 540.0, "linear"), (500, 486.0, "linear")],
                ),
                1,
            ),
            entry_on(8, "scaleX", &[(0, 100.0, "linear"), (1000, 70.0, "linear")]),
            entry_on(8, "scaleY", &[(0, 100.0, "linear"), (1000, 70.0, "linear")]),
        ],
    );
    assert_eq!(reported(&omissions), [bezier_reason(group, "Position")]);
    let motion = graphic.unwrap().vector_motion.unwrap();
    assert_eq!(motion.position, [960.0, 540.0]);
    assert_eq!(
        motion.animations,
        [PrPropertyAnimation::UniformScale(vec![
            scalar(at(0), 100.0, Linear),
            scalar(at(1000), 70.0, Linear),
        ])]
    );
    // When every Vector Motion track is reported, the static group composes
    // into the text.
    let (graphic, omissions) = exported_group(
        json!({}),
        vec![
            bezier(
                entry_on(
                    8,
                    "positionX",
                    &[(0, 960.0, "linear"), (1000, 1056.0, "linear")],
                ),
                1,
            ),
            bezier(
                entry_on(
                    8,
                    "positionY",
                    &[(0, 540.0, "linear"), (1000, 486.0, "linear")],
                ),
                1,
            ),
        ],
    );
    assert_eq!(reported(&omissions), [bezier_reason(group, "Position")]);
    let graphic = graphic.unwrap();
    assert!(graphic.vector_motion.is_none());
    assert!(graphic.text().animations.is_empty());
}

/// `entry` with cubic Bezier `handles` (x1, y1, x2, y2) on its key `index`.
fn with_bezier(mut entry: Value, index: usize, [x1, y1, x2, y2]: [f64; 4]) -> Value {
    entry["animator"]["keyframes"][index]["easing"] =
        json!({"type": "cubicBezier", "x1": x1, "y1": y1, "x2": x2, "y2": y2});
    entry
}

/// Export `entries` for graphic group 8 and its text layer 9, write the
/// project and load it again: the written key lists, the loaded graphic and
/// every omission of the export and the load.
fn written_graphic(entries: Vec<Value>) -> (Vec<String>, PrGraphic, Vec<Omission>) {
    use std::io::Read;
    let (project, mut omissions) = export_over_canvas(vec![graphic_group(json!({}))], entries);
    let project = project.unwrap_or_else(|error| panic!("{error}: {omissions:?}"));
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("written.prproj");
    PremiereProjectXml::new(&project)
        .unwrap()
        .write_new(&path)
        .unwrap();
    let mut xml = String::new();
    flate2::read::GzDecoder::new(std::fs::File::open(&path).unwrap())
        .read_to_string(&mut xml)
        .unwrap();
    let key_lists = xml
        .split("<Keyframes>")
        .skip(1)
        .map(|rest| rest[..rest.find("</Keyframes>").unwrap()].to_owned())
        .collect();
    let (loaded, load_omissions) = PrProjectFile::load(&path).unwrap();
    omissions.extend(load_omissions);
    let graphic = loaded.sequences[0]
        .video_items()
        .find_map(PrVideoItem::graphic)
        .expect("the written graphic loads")
        .clone();
    (key_lists, graphic, omissions)
}

/// The key strings that Premiere 26.5.1 saved in the Bezier speed probe
///: one 1.2 s segment per parameter with Bezier on both keys, on a
/// placement that starts one hour into the generator, where export places a
/// graphic at 30 fps.
const PROBE_KEYS: [(&str, &str); 4] = [
    (
        "Vector Motion Scale",
        "914533804800000,100.,5,0,0,0.16666666666666666,-6.5,0.5;914838624000000,50.,5,0,-1,0.25,0,0.16666666666666666;",
    ),
    (
        "Vector Motion Rotation",
        "914914828800000,0.,5,0,0,0.16666666666666666,8,0.5;915219648000000,60.,5,0,12,0.25,0,0.16666666666666666;",
    ),
    (
        "Text Scale",
        "915295852800000,100.,5,0,0,0.16666666666666666,13,0.5;915600672000000,200.,5,0,20,0.25,0,0.16666666666666666;",
    ),
    (
        "Text Opacity",
        "915676876800000,100.,5,0,0,0.16666666666666666,-10.5,0.5;915981696000000,20.,5,0,-16,0.25,0,0.16666666666666666;",
    ),
];

#[test]
fn probed_bezier_tracks_export_the_speeds_premiere_saved() {
    // The probe's curves as FX tracks, in normalized handles: x1 is the out
    // influence and y1 = x1 * out speed * duration / change; x2 is one minus
    // the in influence and y2 = 1 - in influence * in speed * duration /
    // change.
    let vm_scale = [0.5, 0.078, 0.75, 0.994];
    let text_scale = [0.5, 0.078, 0.75, 0.94];
    let (key_lists, graphic, omissions) = written_graphic(vec![
        with_bezier(
            entry_on(
                8,
                "scaleX",
                &[(300, 100.0, "linear"), (1500, 50.0, "linear")],
            ),
            1,
            vm_scale,
        ),
        with_bezier(
            entry_on(
                8,
                "scaleY",
                &[(300, 100.0, "linear"), (1500, 50.0, "linear")],
            ),
            1,
            vm_scale,
        ),
        with_bezier(
            entry_on(
                8,
                "rotation",
                &[(1800, 0.0, "linear"), (3000, 60.0, "linear")],
            ),
            1,
            [0.5, 0.08, 0.75, 0.94],
        ),
        with_bezier(
            entry_on(
                9,
                "scaleX",
                &[(3300, 100.0, "linear"), (4500, 200.0, "linear")],
            ),
            1,
            text_scale,
        ),
        with_bezier(
            entry_on(
                9,
                "scaleY",
                &[(3300, 100.0, "linear"), (4500, 200.0, "linear")],
            ),
            1,
            text_scale,
        ),
        with_bezier(
            entry_on(
                9,
                "opacity",
                &[(4800, 100.0, "linear"), (6000, 20.0, "linear")],
            ),
            1,
            [0.5, 0.07875, 0.75, 0.94],
        ),
    ]);
    assert!(
        omissions
            .iter()
            .all(|omission| !omission.reason.contains("animation")),
        "{omissions:?}"
    );
    let fields = |keys: &str| -> Vec<Vec<String>> {
        keys.split_terminator(';')
            .map(|key| key.split(',').map(str::to_owned).collect())
            .collect()
    };
    let number = |field: &str| field.parse::<f64>().unwrap();
    for (name, saved) in PROBE_KEYS {
        let saved = fields(saved);
        let written = key_lists
            .iter()
            .find(|keys| keys.starts_with(&format!("{},", saved[0][0])))
            .unwrap_or_else(|| panic!("{name}: {key_lists:?}"));
        let written = fields(written);
        assert_eq!(written.len(), 2, "{name}");
        // Times and values, the start key's mode and outgoing speed and
        // influence, and the end key's incoming ones.
        for (key, indices) in [(0, &[0, 1, 2, 6, 7][..]), (1, &[0, 1, 4, 5][..])] {
            for &index in indices {
                let (written, saved) = (&written[key][index], &saved[key][index]);
                assert!(
                    (number(written) - number(saved)).abs() < 1e-9,
                    "{name}, key {key}, field {index}: written {written}, saved {saved}"
                );
            }
        }
        // A key takes the mode of the segment after it; the probe's last key
        // starts none.
        assert_eq!(written[1][2], "0", "{name}");
    }
    // The written project loads with the same curves.
    let motion = graphic.vector_motion.as_ref().expect("keyed Vector Motion");
    fn keys(
        animations: &[PrPropertyAnimation],
        property: PrAnimatedProperty,
    ) -> &[PrScalarKeyframe] {
        animations
            .iter()
            .find(|animation| animation.property() == property)
            .unwrap_or_else(|| panic!("{property:?}: {animations:?}"))
            .keys()
    }
    for (keys, handles) in [
        (
            keys(&motion.animations, PrAnimatedProperty::UniformScale),
            vm_scale,
        ),
        (
            keys(&motion.animations, PrAnimatedProperty::Rotation),
            [0.5, 0.08, 0.75, 0.94],
        ),
        (
            keys(&graphic.text().animations, PrAnimatedProperty::UniformScale),
            text_scale,
        ),
        (
            keys(&graphic.text().animations, PrAnimatedProperty::Opacity),
            [0.5, 0.07875, 0.75, 0.94],
        ),
    ] {
        let PrKeyframeEasing::CubicBezier { x1, y1, x2, y2 } = keys[1].easing else {
            panic!("{keys:?}");
        };
        let loaded = [x1, y1, x2, y2];
        assert!(
            loaded
                .iter()
                .zip(handles)
                .all(|(loaded, handle)| (loaded - handle).abs() < 1e-12),
            "{loaded:?} != {handles:?}"
        );
    }
}

#[test]
fn a_bezier_segment_after_a_linear_one_writes_a_neutral_in_handle() {
    // The key between the segments starts the Bezier one, so it is written
    // with mode 5, and the Linear segment into it gives it no incoming
    // handle: the reader keeps that segment Linear and the Bezier one intact.
    let (key_lists, graphic, omissions) = written_graphic(vec![with_bezier(
        entry_on(
            9,
            "opacity",
            &[
                (0, 100.0, "linear"),
                (500, 80.0, "linear"),
                (1500, 20.0, "linear"),
            ],
        ),
        2,
        [0.5, 0.07875, 0.75, 0.94],
    )]);
    let middle = format!("{},80,5,0,0,0,", EXPORT_IN + TICKS / 2);
    assert!(
        key_lists.iter().any(|keys| keys.contains(&middle)),
        "{key_lists:?}"
    );
    assert!(
        omissions
            .iter()
            .all(|omission| !omission.reason.contains("animation")),
        "{omissions:?}"
    );
    let keys = graphic.text().animations[0].keys();
    assert_eq!(keys[1].easing, PrKeyframeEasing::Linear);
    assert!(matches!(
        keys[2].easing,
        PrKeyframeEasing::CubicBezier { .. }
    ));
}

#[test]
fn imported_vector_motion_keys_edited_in_the_document_export_and_read_back() {
    let (mut document, _) = imported(vector_motion_graphic());
    // Move the last Scale key 250 ms earlier and change its value, on both axes.
    let entries = document["composition"]["dynamics"]["entries"]
        .as_array_mut()
        .unwrap();
    for entry in entries.iter_mut().filter(|entry| {
        entry["target"]["layerId"] == 3
            && matches!(
                entry["target"]["propertyType"].as_str(),
                Some("scaleX" | "scaleY")
            )
    }) {
        entry["animator"]["keyframes"][1]["layerTime"] = json!(750);
        entry["animator"]["keyframes"][1]["value"]["value"] = json!(60.0);
    }
    document["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .remove(1);
    let document = EditableFxCompositionDocument::from_json_value(document).unwrap();
    let mut omissions = Vec::new();
    let project = tesseract_to_premiere(
        &document,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        FrameRate::Fps30,
        &mut omissions,
    )
    .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("edited.prproj");
    PremiereProjectXml::new(&project)
        .unwrap()
        .write_new(&path)
        .unwrap();
    let (reopened, _) = PrProjectFile::load(&path).unwrap();
    let graphic = reopened.sequences[0]
        .video_items()
        .find_map(PrVideoItem::graphic)
        .unwrap();
    assert_eq!(graphic.timeline_ticks(), TICKS..3 * TICKS);
    let motion = graphic.vector_motion.as_ref().unwrap();
    let scale = motion
        .animations
        .iter()
        .find(|animation| animation.property() == PrAnimatedProperty::UniformScale)
        .unwrap();
    assert_eq!(
        scale.keys(),
        [
            scalar(EXPORT_IN, 100.0, PrKeyframeEasing::Linear),
            scalar(EXPORT_IN + 3 * TICKS / 4, 60.0, PrKeyframeEasing::Linear),
        ]
    );
    // The static anchor and the text's own keys come back unchanged.
    assert!((motion.anchor[0] - 768.0).abs() < 1e-9 && (motion.anchor[1] - 486.0).abs() < 1e-9);
    assert_eq!(graphic.text().animations.len(), 4);
}

#[test]
fn a_text_shadow_under_a_scaling_or_rotating_vector_motion_is_omitted_in_both_directions() {
    let mut graphic = vector_motion_graphic();
    graphic.text_mut().animations.clear();
    graphic.text_mut().transform.scale = 100.0;
    graphic.text_mut().transform.rotation = 0.0;
    graphic.text_mut().document.shadow = Some(PrTextShadow {
        color: crate::schema::text::PrRgb([0, 0, 0]),
        opacity: 100.0,
        angle: 135.0,
        distance: 3.0,
        size: 6.0,
        blur: 12.0,
    });
    let (document, omissions) = imported(graphic);
    assert!(document["composition"]["layers"][0]["layers"][0]
        .get("effects")
        .is_none());
    assert!(
        omissions.iter().any(|omission| omission.reason
            == "text shadow not converted: its graphic's Vector Motion scales or rotates it"),
        "{omissions:?}"
    );
    let shadow = json!([{"id": 5, "effect": {"type": "dropShadow", "offset": [3, 3], "color": [0, 0, 0, 1]}}]);
    let text_with_shadow = json!({"layers": [{
        "type": "Text", "id": 9, "name": "Title", "parent": 8,
        "activeRange": {"start": 0, "duration": 1000},
        "transform": {"anchorPoint": [0, 0], "position": [960, 540], "scale": [100, 100], "rotation": 0, "opacity": 100},
        "sourceText": {"text": "Keys", "fontFamily": "Inter-Bold", "fontStyle": "", "fontSize": 80, "fillColor": [1, 1, 1, 1]},
        "effects": shadow,
    }]});
    let (graphic, omissions) = exported_group(
        text_with_shadow.clone(),
        vec![entry_on(
            8,
            "rotation",
            &[(0, 0.0, "linear"), (500, 30.0, "linear")],
        )],
    );
    assert_eq!(graphic.unwrap().text().document.shadow, None);
    assert_eq!(
        omissions
            .iter()
            .map(|omission| omission.reason.as_str())
            .collect::<Vec<_>>(),
        ["drop shadow 5 was not exported: unsupported conversion: its graphic's Vector Motion scales or rotates it"]
    );
    // Position keys alone move the shadow with the text.
    let (graphic, omissions) = exported_group(
        text_with_shadow,
        vec![
            entry_on(
                8,
                "positionX",
                &[(0, 960.0, "linear"), (500, 900.0, "linear")],
            ),
            entry_on(
                8,
                "positionY",
                &[(0, 540.0, "linear"), (500, 500.0, "linear")],
            ),
        ],
    );
    assert!(
        graphic.unwrap().text().document.shadow.is_some(),
        "{omissions:?}"
    );
}

fn shape_mut(graphic: &mut PrGraphic) -> &mut crate::schema::text::PrShape {
    match graphic.objects.as_mut_slice() {
        [crate::schema::text::PrGraphicObject::Shape(shape)] => shape,
        objects => panic!("test graphic holds one shape, not {objects:?}"),
    }
}

/// The FX layer that importing `graphic` makes of its one object, as root
/// layer 9 over the first second, to export again.
fn imported_object(graphic: PrGraphic) -> Value {
    let (document, omissions) = imported(graphic);
    assert!(
        omissions
            .iter()
            .all(|omission| omission.scope != OmissionScope::Occurrence),
        "{omissions:?}"
    );
    let mut layer = document["composition"]["layers"][0].clone();
    layer["id"] = json!(9);
    layer["activeRange"] = json!({"start": 0, "duration": 1000});
    layer
}

/// The Shape that exporting `layer` next to [`sibling_text`] makes, if any,
/// and every omission.
fn exported_shape(
    layer: Value,
    entries: Vec<Value>,
) -> (Option<crate::schema::text::PrShape>, Vec<Omission>) {
    let (project, omissions) = export_over_canvas(vec![layer, sibling_text()], entries);
    let project = project.unwrap_or_else(|error| panic!("{error}: {omissions:?}"));
    let shape = project
        .single_sequence()
        .unwrap()
        .video_items()
        .filter_map(PrVideoItem::graphic)
        .find_map(|graphic| match graphic.objects.as_slice() {
            [crate::schema::text::PrGraphicObject::Shape(shape)] => Some(shape.clone()),
            _ => None,
        });
    (shape, omissions)
}

#[test]
fn legacy_json_appearance_placements_edit_apart_and_export_their_current_modern_paint() {
    use crate::format::shape_payload::{decode_appearance, encode_appearance};
    use crate::schema::text::PrRgb;
    use crate::tests::support::{legacy_appearance, legacy_json};
    use base64::{engine::general_purpose::STANDARD, Engine};
    use std::io::Read;
    // Our own version 1 legacy payload: a visible white fill, the stroke and
    // the shadow off.
    let legacy = legacy_appearance(&legacy_json(&[("mFillColor", Some("16777215"))]));
    let white = decode_appearance(&legacy).unwrap();
    assert_eq!(white.fill, Some(PrFill::Solid(PrRgb([255; 3]))));
    // Two placements of the decoded shape: A over 1-2 s, and B over 3-5 s,
    // which ends with the sequence and so keeps the exported duration.
    let placement = |id: &str, seconds: std::ops::Range<i64>| {
        let mut graphic = shape_graphic();
        graphic.id = Some(id.into());
        (graphic.start_ticks, graphic.end_ticks) = (seconds.start * TICKS, seconds.end * TICKS);
        shape_mut(&mut graphic).appearance = white.clone();
        graphic
    };
    let (mut document, omissions) =
        imported_graphics(vec![placement("20", 1..2), placement("21", 3..5)]);
    assert!(
        omissions
            .iter()
            .all(|omission| omission.scope != OmissionScope::Occurrence),
        "{omissions:?}"
    );
    // A graphic-only export: drop the video layer and keep the black canvas.
    let layers = document["composition"]["layers"].as_array_mut().unwrap();
    layers.retain(|layer| layer["type"] != "Video");
    let shape_from = |layers: &[Value], start: u64| {
        layers
            .iter()
            .position(|layer| layer["type"] == "Shape" && layer["activeRange"]["start"] == start)
            .unwrap_or_else(|| panic!("no shape layer from {start} ms: {layers:?}"))
    };
    let (a, b) = (shape_from(layers, 1000), shape_from(layers, 3000));
    let white_paint = json!({"type": "solid", "color": [1.0, 1.0, 1.0, 1.0]});
    for layer in [a, b] {
        assert_eq!(layers[layer]["shape"]["fills"][0]["paint"], white_paint);
    }
    assert_ne!(layers[a]["id"], layers[b]["id"]);
    // Edit only A: its fill and its position.
    let b_before = layers[b].clone();
    layers[a]["shape"]["fills"][0]["paint"]["color"] = json!([1.0, 0.4, 0.0, 1.0]);
    layers[a]["transform"]["position"] = json!([480.0, 270.0]);
    assert_eq!(layers[b], b_before);
    let document = EditableFxCompositionDocument::from_json_value(document).unwrap();
    let mut omissions = Vec::new();
    let project = tesseract_to_premiere(
        &document,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        FrameRate::Fps30,
        &mut omissions,
    )
    .unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    // Write the current content and read it back.
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("legacy-appearance.prproj");
    PremiereProjectXml::new(&project)
        .unwrap()
        .write_new(&path)
        .unwrap();
    let mut xml = String::new();
    flate2::read::GzDecoder::new(std::fs::File::open(&path).unwrap())
        .read_to_string(&mut xml)
        .unwrap();
    let (reopened, reopen_omissions) = PrProjectFile::load(&path).unwrap();
    assert!(reopen_omissions.is_empty(), "{reopen_omissions:?}");
    // Each placement keeps its own range, position and paint.
    let edited = PrAppearance {
        fill: Some(PrFill::Solid(PrRgb([255, 102, 0]))),
        ..white.clone()
    };
    let shapes = |project: &PrProjectFile| {
        let mut shapes: Vec<_> = project
            .single_sequence()
            .unwrap()
            .video_items()
            .filter_map(PrVideoItem::graphic)
            .map(|graphic| match graphic.objects.as_slice() {
                [PrGraphicObject::Shape(shape)] => (
                    graphic.start_ticks,
                    graphic.end_ticks,
                    shape.transform.position,
                    shape.appearance.clone(),
                ),
                objects => panic!("one shape, not {objects:?}"),
            })
            .collect();
        shapes.sort_by_key(|shape| shape.0);
        shapes
    };
    let expected = vec![
        (TICKS, 2 * TICKS, [480.0, 270.0], edited.clone()),
        (3 * TICKS, 5 * TICKS, [960.0, 540.0], white.clone()),
    ];
    assert_eq!(shapes(&project), expected);
    assert_eq!(shapes(&reopened), expected);
    // The written Appearance values are the FlatBuffer payloads of the
    // current paints; the legacy JSON is never replayed.
    let mut written: Vec<Vec<u8>> = xml
        .split("<Name>Appearance</Name>")
        .skip(1)
        .map(|record| {
            let value = &record[record.find("<StartKeyframeValue").unwrap()..];
            let start = value.find('>').unwrap() + 1;
            let end = value.find("</StartKeyframeValue>").unwrap();
            STANDARD.decode(&value[start..end]).unwrap()
        })
        .collect();
    written.sort();
    let mut current = vec![
        encode_appearance(&edited).unwrap(),
        encode_appearance(&white).unwrap(),
    ];
    current.sort();
    assert_eq!(written, current);
    assert!(written
        .iter()
        .all(|payload| payload != &legacy && payload.get(8..10) != Some(b"{\0".as_slice())));
}

#[test]
fn shape_stroke_joins_convert_only_at_the_corners_calibration_two_measured() {
    use crate::schema::text::{
        stroke_join, PrPathVertex, PrRgb, PrShapePath, PrShapeStroke, StrokeJoin,
    };
    let corner = |[x, y]: [f32; 2]| PrPathVertex {
        smooth: false,
        point: [x, y],
        in_tangent: [x, y],
        out_tangent: [x, y],
    };
    let open = |vertices: Vec<PrPathVertex>| PrShapePath {
        vertices,
        closed: false,
    };
    // Open paths from (100, 0) through a vertex at the origin to the f32 end
    // of a 100 px arm at the row's angle, so that each has one join.
    let (start, origin, twenty) = ([100.0, 0.0], [0.0, 0.0], [93.969_26, 34.202_015]);
    let turn = |end| open(vec![corner(start), corner(origin), corner(end)]);
    let smooth = open(vec![
        corner([100.0, 100.0]),
        PrPathVertex {
            smooth: true,
            point: origin,
            in_tangent: [50.0, 0.0],
            out_tangent: [-50.0, 0.0],
        },
        corner([-100.0, 100.0]),
    ]);
    let (miter, bevel) = (Ok(StrokeJoin::Miter(4.0)), Ok(StrokeJoin::Bevel));
    let unverified = Err("stroke joins are unverified");
    // The join that import proves, and whether export keeps an FX Miter of
    // limit 4, 2 and 1.5 and a Bevel. An FX Round join never exports. The
    // classes end at calibration-2's measured triangle corners (last row), so
    // 72°, 60° and 36° are inferred.
    for (angle, path, import, exports) in [
        ("90°", turn([0.0, 100.0]), miter, [true, true, true, false]),
        (
            "72°",
            turn([30.901_7, 95.105_65]),
            miter,
            [true, true, false, false],
        ),
        ("60°", turn([50.0, 86.602_54]), unverified, [false; 4]),
        (
            "36°",
            turn([80.901_7, 58.778_526]),
            bevel,
            [false, false, true, true],
        ),
        ("20°", turn(twenty), bevel, [false, true, true, true]),
        ("smooth", smooth, miter, [true; 4]),
        (
            "90° and 20°",
            open(vec![
                corner([100.0, 100.0]),
                corner(start),
                corner(origin),
                corner(twenty),
            ]),
            Err("strokes with both mitered and beveled corners are unsupported"),
            [false; 4],
        ),
        (
            "calibration-2 triangle",
            PrShapePath {
                vertices: vec![
                    corner([300.0, 0.0]),
                    corner([-300.0, 195.0]),
                    corner([-300.0, -195.0]),
                ],
                closed: true,
            },
            Err("strokes with both mitered and beveled corners are unsupported"),
            [false; 4],
        ),
    ] {
        let mut graphic = shape_graphic();
        let shape = shape_mut(&mut graphic);
        shape.path = path;
        shape.appearance.stroke = Some(PrShapeStroke {
            color: PrRgb([0, 255, 64]),
            width: 24.0,
        });
        assert_eq!(stroke_join(&shape.path, None), import, "{angle}");
        if let Ok(join) = import {
            let layer = imported_object(graphic.clone());
            let stroke = &layer["shape"]["strokes"][0];
            let (name, limit) = match join {
                StrokeJoin::Miter(limit) => ("miter", limit),
                _ => ("bevel", 4.0),
            };
            assert_eq!(
                (&stroke["join"], &stroke["cap"], &stroke["miterLimit"]),
                (&json!(name), &json!("butt"), &json!(limit)),
                "{angle}"
            );
        }
        // Export the importer's path under an FX stroke of each join.
        shape_mut(&mut graphic).appearance.stroke = None;
        let layer = imported_object(graphic.clone());
        for ((join, limit), exports) in [
            ("miter", 4.0),
            ("miter", 2.0),
            ("miter", 1.5),
            ("bevel", 4.0),
            ("round", 4.0),
        ]
        .into_iter()
        .zip(exports.into_iter().chain([false]))
        {
            let mut layer = layer.clone();
            layer["shape"]["strokes"] = json!([{
                "paint": {"type": "solid", "color": [0, 1, 0.25, 1]},
                "width": 24,
                "join": join,
                "miterLimit": limit,
            }]);
            let (shape, omissions) = exported_shape(layer, Vec::new());
            let context = format!("{angle}, {join} {limit}: {omissions:?}");
            if exports {
                let shape = shape.expect(&context);
                assert_eq!(shape.path, shape_mut(&mut graphic).path, "{context}");
                assert!(shape.appearance.stroke.is_some(), "{context}");
            } else {
                // A path that import rejects fails before the join does.
                let reason = match import {
                    Err(reason) => reason,
                    Ok(_) if join == "round" => "round stroke joins are unverified",
                    Ok(_) => "stroke joins are unverified",
                };
                assert!(shape.is_none(), "{context}");
                assert_eq!(
                    omissions
                        .iter()
                        .filter(|omission| omission.record == "layer 9 (\"Box\")")
                        .map(|omission| omission.reason.as_str())
                        .collect::<Vec<_>>(),
                    [format!(
                        "shape layer was not exported: unsupported conversion: {reason}"
                    )],
                    "{context}"
                );
            }
        }
    }
}

#[test]
fn shape_vertices_export_smooth_where_premiere_draws_their_tangents() {
    // FX paths of anchors without a mirror mode unless one is named, and the
    // smooth flag of each exported vertex.
    let layer = imported_object(shape_graphic());
    for (case, commands, smooth) in [
        // A cusp between two vertices with one handle each, then a corner.
        (
            "unmirrored curves",
            json!([
                {"type": "moveTo", "x": 0.0, "y": 0.0},
                {"type": "cubicTo", "c1x": 30.0, "c1y": -40.0, "c2x": 70.0, "c2y": -40.0, "x": 100.0, "y": 0.0},
                {"type": "cubicTo", "c1x": 130.0, "c1y": 20.0, "c2x": 120.0, "c2y": 80.0, "x": 100.0, "y": 100.0},
                {"type": "lineTo", "x": 0.0, "y": 100.0},
                {"type": "close"}
            ]),
            vec![true, true, true, false],
        ),
        // A curve whose second handle rests on its end leaves that vertex a
        // corner.
        (
            "a handle on its vertex",
            json!([
                {"type": "moveTo", "x": 0.0, "y": 0.0},
                {"type": "cubicTo", "c1x": 50.0, "c1y": -50.0, "c2x": 100.0, "c2y": 0.0, "x": 100.0, "y": 0.0},
                {"type": "lineTo", "x": 100.0, "y": 100.0},
                {"type": "close"}
            ]),
            vec![true, false, false],
        ),
        (
            "a symmetrical anchor without handles",
            json!([
                {"type": "moveTo", "x": 0.0, "y": 0.0, "mirror": "symmetrical"},
                {"type": "lineTo", "x": 100.0, "y": 0.0},
                {"type": "lineTo", "x": 100.0, "y": 100.0},
                {"type": "close"}
            ]),
            vec![true, false, false],
        ),
    ] {
        let mut layer = layer.clone();
        layer["shape"]["path"]["commands"] = commands.clone();
        let (shape, omissions) = exported_shape(layer, Vec::new());
        let shape = shape.unwrap_or_else(|| panic!("{case}: {omissions:?}"));
        assert_eq!(
            shape
                .path
                .vertices
                .iter()
                .map(|vertex| vertex.smooth)
                .collect::<Vec<_>>(),
            smooth,
            "{case}"
        );
        // The reimport keeps the tangents, which alone FX draws, and exports
        // the same path again.
        let mut graphic = shape_graphic();
        *shape_mut(&mut graphic) = shape.clone();
        let reimported = imported_object(graphic);
        let drawn = |commands: &Value| {
            let mut commands = commands.clone();
            for command in commands.as_array_mut().unwrap() {
                command.as_object_mut().unwrap().remove("mirror");
            }
            commands
        };
        assert_eq!(
            drawn(&reimported["shape"]["path"]["commands"]),
            drawn(&commands),
            "{case}"
        );
        let (again, omissions) = exported_shape(reimported, Vec::new());
        assert_eq!(
            again.map(|shape| shape.path),
            Some(shape.path),
            "{case}: {omissions:?}"
        );
    }
}

#[test]
fn graphic_shapes_premiere_cannot_hold_are_omitted_on_export() {
    let shape = imported_object(shape_graphic());
    let edited = |keys: &[&str], value: Value| {
        let mut layer = shape.clone();
        let target = keys
            .iter()
            .fold(&mut layer, |target, key| match key.parse::<usize>() {
                Ok(index) => &mut target[index],
                Err(_) => &mut target[*key],
            });
        *target = value;
        layer
    };
    let fill = &shape["shape"]["fills"][0];
    let stroke = |extra: Value| {
        let mut stroke = json!({"paint": {"type": "solid", "color": [0, 1, 0.25, 1]}, "width": 24});
        for (key, value) in extra.as_object().unwrap() {
            stroke[key] = value.clone();
        }
        json!([stroke])
    };
    let square = |x: f64| {
        json!([
            {"type": "moveTo", "x": x, "y": 0},
            {"type": "lineTo", "x": x + 10.0, "y": 0},
            {"type": "lineTo", "x": x + 10.0, "y": 10},
            {"type": "close"}
        ])
    };
    let two_contours: Vec<_> = [square(0.0), square(0.0)]
        .iter()
        .flat_map(|commands| commands.as_array().unwrap().clone())
        .collect();
    let gradient = |kind: &str, end: [f64; 2], offsets: [f64; 2]| {
        json!({
            "type": "gradient", "gradientType": kind, "start": [0, 0], "end": end,
            "stops": [
                {"offset": offsets[0], "color": [1, 0, 0, 1]},
                {"offset": offsets[1], "color": [0, 0, 1, 1]}
            ]
        })
    };
    let paint = |gradient: Value| edited(&["shape", "fills", "0", "paint"], gradient);
    let undrawable = "shape layer was not exported: invalid Premiere project: a gradient needs a finite axis of at least 0.0001 px and two or more stops in order within 0..=1";
    let rounded = |radius| {
        edited(
            &["shape", "path", "commands", "1", "cornerRadius"],
            json!(radius),
        )
    };
    let shape_layer = "shape layer was not exported: unsupported conversion";
    for (layer, reason) in [
        (
            paint(gradient("conic", [100.0, 0.0], [0.0, 1.0])),
            format!("{shape_layer}: reflected and conic gradient shape fills are unsupported"),
        ),
        (
            paint(gradient("radial", [100.0, 10.0], [0.0, 1.0])),
            format!("{shape_layer}: {GRADIENT_Y_UNCONVERTED}"),
        ),
        (
            paint(gradient("linear", [0.0, 0.0], [0.0, 1.0])),
            undrawable.to_owned(),
        ),
        (
            paint(gradient("linear", [100.0, 0.0], [1.0, 0.0])),
            undrawable.to_owned(),
        ),
        (
            edited(&["shape", "fills"], json!([fill, fill])),
            format!("{shape_layer}: a Premiere shape has one fill"),
        ),
        (
            edited(&["shape", "fills", "0", "opacity"], json!(0.5)),
            format!(
                "{shape_layer}: shape fills blend normally at full opacity with the nonzero rule"
            ),
        ),
        (
            edited(
                &["shape", "strokes"],
                json!([stroke(json!({}))[0], stroke(json!({}))[0]]),
            ),
            format!("{shape_layer}: a Premiere shape has one stroke"),
        ),
        (
            edited(&["shape", "strokes"], stroke(json!({"enabled": false}))),
            format!("{shape_layer}: a disabled shape stroke is unsupported"),
        ),
        (
            edited(&["shape", "strokes"], stroke(json!({"dashes": [10, 5]}))),
            format!("{shape_layer}: dashed shape strokes are unsupported"),
        ),
        (
            edited(&["shape", "strokes"], stroke(json!({"cap": "round"}))),
            format!("{shape_layer}: round and square stroke caps are unverified"),
        ),
        (
            edited(&["shape", "path", "commands"], json!(two_contours)),
            format!("{shape_layer}: contours 1 and 2 of the shape path are not certified apart"),
        ),
        (
            edited(&["shape", "roundCorners"], json!({"radius": 10})),
            format!("{shape_layer}: shape primitives and path modifiers are unsupported"),
        ),
        (
            rounded(10),
            format!("{shape_layer}: rounded shape path corners are unsupported"),
        ),
        (
            edited(&["transform", "skew"], json!(10)),
            format!("{shape_layer}: a graphic shape has no skew or 3D rotation"),
        ),
        (
            edited(
                &["masks"],
                json!([{"id": 50, "mode": "add", "path": {"commands": square(0.0)}}]),
            ),
            "graphic was not exported: its shape layer must have no masks".to_owned(),
        ),
    ] {
        assert_graphic_omitted(vec![layer], "layer 9 (\"Box\")", &reason);
    }
    // An explicit zero radius keeps the anchor sharp.
    assert!(exported_shape(rounded(0), Vec::new()).0.is_some());
    let (exported, omissions) = exported_shape(
        shape.clone(),
        vec![entry(
            "positionX",
            &[(0, 960.0, "linear"), (500, 900.0, "linear")],
        )],
    );
    assert!(exported.is_none());
    assert!(omissions.iter().any(|omission| omission.reason
        == format!("{shape_layer}: keyed graphic shapes are unsupported")));

    // A graphic of several objects: group 8 holds text 9 and shape 11.
    let child = |edit: &[(&str, Value)]| {
        let mut layer = shape.clone();
        layer["id"] = json!(11);
        layer["parent"] = json!(8);
        for (key, value) in edit {
            layer[*key] = value.clone();
        }
        let mut group = graphic_group(json!({}));
        group["layers"].as_array_mut().unwrap().push(layer);
        group
    };
    for (group, reason) in [
        (
            child(&[("isHidden", json!(true))]),
            "the objects of a graphic are hidden together",
        ),
        (
            child(&[("activeRange", json!({"start": 0, "duration": 500}))]),
            "its shape layer must span the group",
        ),
        (
            child(&[("shape", rounded(10)["shape"].clone())]),
            "layer 11 (\"Box\"): unsupported conversion: rounded shape path corners are unsupported",
        ),
    ] {
        assert_graphic_omitted(
            vec![group],
            "layer 8 (\"Graphic\")",
            &format!("graphic group was not exported: {reason}"),
        );
    }
    let (project, omissions) = export_over_canvas(
        vec![child(&[]), sibling_text()],
        vec![entry_on(
            11,
            "rotation",
            &[(0, 0.0, "linear"), (500, 45.0, "linear")],
        )],
    );
    assert_eq!(
        project
            .unwrap()
            .single_sequence()
            .unwrap()
            .video_items()
            .filter_map(PrVideoItem::graphic)
            .count(),
        1
    );
    assert!(
        omissions.iter().any(|omission| omission.record == "layer 8 (\"Graphic\")"
            && omission.reason
                == "graphic group was not exported: layer 11 (\"Box\"): unsupported conversion: keyed graphic shapes are unsupported"),
        "{omissions:?}"
    );
}

#[test]
fn gradient_shape_fills_convert_in_both_directions_in_the_rendered_form() {
    use crate::schema::text::{
        PrGradientKind::{Linear, Radial},
        PrRgb, OPAQUE_OPACITY_STOPS,
    };
    let stop = |position: f32, color: [u8; 3]| PrGradientStop {
        position,
        color: PrRgb(color),
    };
    // The gradient fixture's B and C: start and end x in
    // layer pixels, and C's middle stop at its saved f32 position.
    let linear = PrGradient {
        kind: Linear,
        start_x: -150.0,
        end_x: 150.0,
        stops: vec![stop(0.0, [0, 96, 254]), stop(1.0, [0, 200, 0])],
        opacity_stops: OPAQUE_OPACITY_STOPS.to_vec(),
    };
    let radial = PrGradient {
        kind: Radial,
        stops: vec![
            stop(0.0, [255; 3]),
            stop(f32::from_bits(0x3eff_99e3), [0, 96, 254]),
            stop(1.0, [0; 3]),
        ],
        ..linear.clone()
    };
    // Each opaque color stop stays one FX stop: on a grey ramp whose
    // channels do not interpolate exactly in f64, and at two coincident
    // stops of one color.
    let grey = PrGradient {
        stops: vec![stop(0.0, [255; 3]), stop(0.5, [100; 3]), stop(1.0, [10; 3])],
        ..linear.clone()
    };
    let coincident = PrGradient {
        stops: vec![stop(0.5, [0, 96, 254]); 2],
        ..linear.clone()
    };
    for (gradient, kind) in [
        (linear, "linear"),
        (radial.clone(), "radial"),
        (grey, "linear"),
        (coincident, "linear"),
    ] {
        let mut graphic = shape_graphic();
        shape_mut(&mut graphic).appearance.fill = Some(PrFill::Gradient(gradient.clone()));
        let layer = imported_object(graphic);
        let paint = &layer["shape"]["fills"][0]["paint"];
        let offsets: Vec<_> = paint["stops"]
            .as_array()
            .unwrap()
            .iter()
            .map(|stop| stop["offset"].as_f64().unwrap())
            .collect();
        assert_eq!(
            (&paint["gradientType"], &paint["start"], &paint["end"]),
            (&json!(kind), &json!([-150.0, 0.0]), &json!([150.0, 0.0]))
        );
        let positions: Vec<_> = gradient
            .stops
            .iter()
            .map(|stop| f64::from(stop.position))
            .collect();
        assert_eq!(offsets, positions);
        let (shape, omissions) = exported_shape(layer, Vec::new());
        assert!(
            omissions
                .iter()
                .all(|omission| omission.record != "layer 9 (\"Box\")"),
            "{omissions:?}"
        );
        let shape = shape.unwrap_or_else(|| panic!("{omissions:?}"));
        assert_eq!(shape.appearance.fill, Some(PrFill::Gradient(gradient)));
    }
    // An edited stop exports as edited.
    let mut graphic = shape_graphic();
    shape_mut(&mut graphic).appearance.fill = Some(PrFill::Gradient(radial.clone()));
    let mut layer = imported_object(graphic);
    let stops = &mut layer["shape"]["fills"][0]["paint"]["stops"];
    stops[1]["offset"] = json!(0.3);
    stops[2]["color"] = json!([1.0, 0.0, 0.0, 1.0]);
    let (shape, _) = exported_shape(layer, Vec::new());
    let mut edited = radial;
    edited.stops[1].position = 0.3;
    edited.stops[2].color = PrRgb([255, 0, 0]);
    assert_eq!(
        shape.unwrap().appearance.fill,
        Some(PrFill::Gradient(edited))
    );
}

#[test]
fn gradient_shapes_convert_with_one_warning_per_approximation_in_both_directions() {
    use crate::schema::text::{
        PrGradientKind::Linear, PrGraphicObject, PrRgb, GRADIENT_OPACITY_APPROXIMATION,
        GRADIENT_SHADOW_APPROXIMATION, OPAQUE_OPACITY_STOPS, SHAPE_SHADOW_ANGLE,
    };
    let stop = |position: f32, color: [u8; 3]| PrGradientStop {
        position,
        color: PrRgb(color),
    };
    // The gradient fixture's linear B.
    let linear = PrGradient {
        kind: Linear,
        start_x: -150.0,
        end_x: 150.0,
        stops: vec![stop(0.0, [0, 96, 254]), stop(1.0, [0, 200, 0])],
        opacity_stops: OPAQUE_OPACITY_STOPS.to_vec(),
    };
    let edited = |edit: &dyn Fn(&mut PrGraphic)| {
        let mut graphic = shape_graphic();
        shape_mut(&mut graphic).appearance.fill = Some(PrFill::Gradient(linear.clone()));
        edit(&mut graphic);
        graphic
    };
    let with_text = |graphic: &mut PrGraphic| graphic.objects.extend(text_graphic().objects);
    let shadow = |blur: f32| PrTextShadow {
        color: PrRgb([40; 3]),
        opacity: 100.0,
        angle: SHAPE_SHADOW_ANGLE,
        distance: 50.0,
        size: 20.0,
        blur,
    };
    let transformed = |parts: &str| {
        format!("gradient geometry under a shape transform ({parts}) is unmeasured against Premiere; converted in layer space")
    };
    let faded = |parts: &str| {
        format!("gradient fill under an Opacity below 100 ({parts}) is unmeasured against Premiere; converted as FX opacity")
    };
    let keyed_motion = vector_motion_graphic().vector_motion;
    let static_motion = PrVectorMotion {
        position: [960.0, 540.0],
        anchor: [960.0, 540.0],
        scale: 50.0,
        rotation: 90.0,
        animations: Vec::new(),
    };
    for (graphic, warnings) in [
        (
            edited(&|graphic| {
                let shape = shape_mut(graphic);
                shape.transform.scale = 80.0;
                shape.transform.rotation = 30.0;
            }),
            vec![transformed("Scale 80 / Rotation 30")],
        ),
        // Premiere's Horizontal Scale with Uniform Scale off.
        (
            edited(&|graphic| shape_mut(graphic).horizontal_scale = Some(150.0)),
            vec![transformed("Horizontal Scale 150")],
        ),
        // The kept Vector Motion of a graphic with several objects, and
        // keyed Vector Motion.
        (
            edited(&|graphic| {
                with_text(graphic);
                graphic.vector_motion = Some(static_motion.clone());
            }),
            vec![transformed("Vector Motion Scale 50 / Vector Motion Rotation 90")],
        ),
        (
            edited(&|graphic| graphic.vector_motion = keyed_motion.clone()),
            vec![transformed("Vector Motion keys")],
        ),
        // A shadow in the measured form converts beside the gradient; one
        // with a blur is omitted, so nothing draws under the gradient.
        (
            edited(&|graphic| shape_mut(graphic).appearance.shadow = Some(shadow(0.0))),
            vec![GRADIENT_SHADOW_APPROXIMATION.to_owned()],
        ),
        (
            edited(&|graphic| shape_mut(graphic).appearance.shadow = Some(shadow(10.0))),
            Vec::new(),
        ),
        // D's opacity stops.
        (
            edited(&|graphic| {
                let Some(PrFill::Gradient(gradient)) = &mut shape_mut(graphic).appearance.fill
                else {
                    unreachable!();
                };
                gradient.opacity_stops[1].opacity = 0.0;
            }),
            vec![GRADIENT_OPACITY_APPROXIMATION.to_owned()],
        ),
        (
            edited(&|graphic| {
                shape_mut(graphic).transform.opacity = 50.0;
                graphic.opacity = 80.0;
            }),
            vec![faded("Opacity 50 / graphic Opacity 80")],
        ),
        (
            edited(&|graphic| graphic.animations = clip_opacity_graphic().animations),
            vec![faded("graphic Opacity keys")],
        ),
        (
            edited(&|graphic| {
                let Some(PrFill::Gradient(gradient)) = &mut shape_mut(graphic).appearance.fill
                else {
                    unreachable!();
                };
                gradient.stops = [0.0, 0.25, 0.75, 1.0]
                    .map(|position| stop(position, [0, 96, 254]))
                    .to_vec();
            }),
            vec!["gradient with 4 color stops is unmeasured against Premiere, which rendered at most 3; converted stop for stop".to_owned()],
        ),
    ] {
        let PrGraphicObject::Shape(input) = &graphic.objects[0] else {
            unreachable!();
        };
        let input = input.appearance.fill.clone();
        // Import reports each under the graphic's record, beside the
        // omission of a shadow that it cannot keep.
        let (mut document, omissions) = imported(graphic);
        let reasons: Vec<_> = omissions
            .iter()
            .filter(|omission| {
                omission.record == "20"
                    && !omission.reason.starts_with("shape shadow not converted")
            })
            .map(|omission| {
                assert_eq!(omission.scope, OmissionScope::Feature);
                assert_eq!(omission.kind, OmissionKind::Approximated);
                omission.reason.as_str()
            })
            .collect();
        assert_eq!(reasons, warnings);
        // Export reports the same under the shape layer and keeps the fill.
        document["composition"]["layers"]
            .as_array_mut()
            .unwrap()
            .remove(1);
        let document = EditableFxCompositionDocument::from_json_value(document).unwrap();
        let mut omissions = Vec::new();
        let project = tesseract_to_premiere(
            &document,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            FrameRate::Fps30,
            &mut omissions,
        )
        .unwrap_or_else(|error| panic!("{error}: {omissions:?}"));
        let reasons: Vec<_> = omissions
            .iter()
            .filter(|omission| omission.record.ends_with("(\"Box\")"))
            .map(|omission| {
                assert_eq!(omission.kind, OmissionKind::Approximated);
                omission.reason.as_str()
            })
            .collect();
        assert_eq!(reasons, warnings);
        let exported = project
            .single_sequence()
            .unwrap()
            .video_items()
            .find_map(PrVideoItem::graphic)
            .unwrap();
        let PrGraphicObject::Shape(shape) = &exported.objects[0] else {
            panic!("{:?}", exported.objects);
        };
        assert_eq!(shape.appearance.fill, input);
    }
    // A disabled drop shadow draws nothing: it is reported, with no warning.
    let mut layer = imported_object(edited(&|_| {}));
    layer["effects"] = json!([{"id": 1, "enabled": false, "effect": {"type": "dropShadow", "offset": [3, 3], "color": [0, 0, 0, 1]}}]);
    let (shape, omissions) = exported_shape(layer, Vec::new());
    assert!(shape.is_some());
    let reasons: Vec<_> = omissions
        .iter()
        .filter(|omission| omission.record == "layer 9 (\"Box\")")
        .map(|omission| omission.reason.as_str())
        .collect();
    assert_eq!(
        reasons,
        ["drop shadow 1 was not exported: unsupported conversion: it is disabled"]
    );
}

#[test]
fn gradient_opacity_stops_join_the_color_stops_as_fx_stop_alpha() {
    use crate::schema::text::{PrGradientKind::Linear, PrGradientOpacityStop, PrRgb};
    let color = |position: f32, color: [u8; 3]| PrGradientStop {
        position,
        color: PrRgb(color),
    };
    let opacity = |position: f32, opacity: f32| PrGradientOpacityStop { position, opacity };
    // A red-to-blue ramp, whose channels interpolate exactly.
    let red_blue = vec![color(0.0, [255, 0, 0]), color(1.0, [0, 0, 255])];
    let fx = |offset: f64, red: f64, alpha: f64| {
        let color = [red, 0.0, 1.0 - red, alpha];
        json!({"offset": offset, "color": color})
    };
    // A grey ramp, whose channels do not interpolate exactly in f64.
    let grey_ramp = vec![
        color(0.0, [255; 3]),
        color(0.5, [100; 3]),
        color(1.0, [10; 3]),
    ];
    let grey = |offset: f64, level: u8, alpha: f64| {
        let level = f64::from(level) / 255.0;
        json!({"offset": offset, "color": [level, level, level, alpha]})
    };
    for (stops, opacity_stops, expected) in [
        // D: the stops share their positions.
        (
            red_blue.clone(),
            vec![opacity(0.0, 1.0), opacity(1.0, 0.0)],
            vec![fx(0.0, 1.0, 1.0), fx(1.0, 0.0, 0.0)],
        ),
        // Shared positions keep one stop each where the channels do not
        // interpolate exactly.
        (
            grey_ramp,
            vec![opacity(0.0, 1.0), opacity(0.5, 0.5), opacity(1.0, 0.0)],
            vec![grey(0.0, 255, 1.0), grey(0.5, 100, 0.5), grey(1.0, 10, 0.0)],
        ),
        // A constant opacity is each color stop's alpha.
        (
            red_blue.clone(),
            vec![opacity(0.0, 0.5), opacity(1.0, 0.5)],
            vec![fx(0.0, 1.0, 0.5), fx(1.0, 0.0, 0.5)],
        ),
        // An opacity stop between the color stops takes the color there,
        // and each color stop the opacity there, held beyond the ends.
        (
            red_blue.clone(),
            vec![opacity(0.25, 0.5), opacity(0.5, 0.0)],
            vec![
                fx(0.0, 1.0, 0.5),
                fx(0.25, 0.75, 0.5),
                fx(0.5, 0.5, 0.0),
                fx(1.0, 0.0, 0.0),
            ],
        ),
        // A step makes a stop on each side.
        (
            red_blue,
            vec![opacity(0.25, 1.0), opacity(0.25, 0.0)],
            vec![
                fx(0.0, 1.0, 1.0),
                fx(0.25, 0.75, 1.0),
                fx(0.25, 0.75, 0.0),
                fx(1.0, 0.0, 0.0),
            ],
        ),
    ] {
        let mut graphic = shape_graphic();
        shape_mut(&mut graphic).appearance.fill = Some(PrFill::Gradient(PrGradient {
            kind: Linear,
            start_x: -150.0,
            end_x: 150.0,
            stops,
            opacity_stops,
        }));
        let layer = imported_object(graphic);
        assert_eq!(
            layer["shape"]["fills"][0]["paint"]["stops"],
            json!(expected)
        );
    }
}

#[test]
fn a_keyed_vector_motion_moves_a_shape_and_a_text_as_one_group_in_both_directions() {
    use crate::schema::text::PrGraphicObject;
    let mut graphic = shape_graphic();
    graphic.objects.extend(text_graphic().objects);
    let motion = vector_motion_graphic().vector_motion.unwrap();
    graphic.vector_motion = Some(motion.clone());
    let (mut document, omissions) = imported(graphic);
    assert!(
        omissions
            .iter()
            .all(|omission| omission.scope != OmissionScope::Occurrence),
        "{omissions:?}"
    );
    // The group has the Vector Motion's keys; its layers are the objects in
    // chain order, the first listed in front.
    let group = &document["composition"]["layers"][0];
    let children: Vec<_> = group["layers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|layer| {
            (
                layer["type"].as_str().unwrap(),
                layer["name"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(children, [("Shape", "Box"), ("Text", "Title")]);
    let group_id = group["id"].as_u64().unwrap();
    assert_eq!(
        track(&document, group_id, "rotation"),
        keys(&[(0, 0.0, "linear"), (1000, 20.0, "hold")])
    );
    // Export writes the keyed Vector Motion over the same objects.
    document["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .remove(1);
    let document = EditableFxCompositionDocument::from_json_value(document).unwrap();
    let mut omissions = Vec::new();
    let project = tesseract_to_premiere(
        &document,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        FrameRate::Fps30,
        &mut omissions,
    )
    .unwrap();
    let exported = project
        .single_sequence()
        .unwrap()
        .video_items()
        .find_map(PrVideoItem::graphic)
        .unwrap();
    assert!(
        matches!(
            exported.objects.as_slice(),
            [PrGraphicObject::Shape(_), PrGraphicObject::Text(_)]
        ),
        "{omissions:?}"
    );
    // The same keys, in export's property order.
    let animations = &exported.vector_motion.as_ref().unwrap().animations;
    assert_eq!(animations.len(), motion.animations.len());
    assert!(
        motion
            .animations
            .iter()
            .all(|keys| animations.contains(keys)),
        "{animations:?}"
    );
}

#[test]
fn shape_shadows_convert_only_in_the_form_the_renders_measured() {
    use crate::schema::text::{PrRgb, PrShape, SHAPE_SHADOW_ANGLE};
    let measured = PrTextShadow {
        color: PrRgb([40; 3]),
        opacity: 100.0,
        angle: SHAPE_SHADOW_ANGLE,
        distance: 50.0,
        size: 20.0,
        blur: 0.0,
    };
    let shadowed = |edit: fn(&mut PrShape)| {
        let mut graphic = shape_graphic();
        let shape = shape_mut(&mut graphic);
        shape.appearance.shadow = Some(measured);
        edit(shape);
        graphic
    };
    // Import: the measured form is one drop shadow 50 px down and to the
    // right; any other form is reported and the shape imports.
    let (document, omissions) = imported(shadowed(|_| {}));
    assert!(omissions.is_empty(), "{omissions:?}");
    let effect = &document["composition"]["layers"][0]["effects"][0]["effect"];
    assert_eq!(effect["type"], "dropShadow");
    for axis in 0..2 {
        let offset = effect["offset"][axis].as_f64().unwrap();
        assert!((offset - 50.0 / 2_f64.sqrt()).abs() < 1e-9, "{effect}");
    }
    for (graphic, reason) in [
        (
            shadowed(|shape| shape.appearance.shadow.as_mut().unwrap().opacity = 60.0),
            "a shape shadow's opacity blend is not measured",
        ),
        (
            shadowed(|shape| shape.appearance.shadow.as_mut().unwrap().blur = 10.0),
            "a shape shadow's blur is not measured",
        ),
        (
            shadowed(|shape| shape.transform.scale = 50.0),
            "its shape is scaled or rotated",
        ),
        (
            shadowed(|shape| shape.horizontal_scale = Some(150.0)),
            "its shape is scaled or rotated",
        ),
    ] {
        let (document, omissions) = imported(graphic);
        let layer = &document["composition"]["layers"][0];
        assert_eq!(
            (&layer["type"], layer.get("effects")),
            (&json!("Shape"), None)
        );
        let reasons: Vec<_> = omissions
            .iter()
            .map(|omission| (omission.scope, omission.reason.as_str()))
            .collect();
        assert_eq!(
            reasons,
            [(
                OmissionScope::Feature,
                format!("shape shadow not converted: {reason}").as_str()
            )]
        );
    }
    // Export: the imported shadow exports unchanged; any other form is
    // reported and the shape exports without it.
    let layer = imported_object(shadowed(|_| {}));
    let (shape, _) = exported_shape(layer.clone(), Vec::new());
    assert_eq!(shape.unwrap().appearance.shadow, Some(measured));
    for (keys, value, reason) in [
        (
            ["effects", "0", "effect", "offset"],
            json!([0, 50]),
            "a shape shadow falls down and to the right, at 135°",
        ),
        (
            ["effects", "0", "effect", "blurRadius"],
            json!(5),
            "a shape shadow's blur is not measured",
        ),
        (
            ["transform", "scale", "0", ""],
            json!(50),
            "its shape is scaled or rotated",
        ),
    ] {
        let mut layer = layer.clone();
        let target = keys
            .iter()
            .filter(|key| !key.is_empty())
            .fold(&mut layer, |target, key| match key.parse::<usize>() {
                Ok(index) => &mut target[index],
                Err(_) => &mut target[*key],
            });
        *target = value;
        let (shape, omissions) = exported_shape(layer, Vec::new());
        assert_eq!(shape.unwrap().appearance.shadow, None, "{reason}");
        assert!(
            omissions
                .iter()
                .any(|omission| omission.scope == OmissionScope::Feature
                    && omission.reason
                        == format!(
                            "drop shadow 1 was not exported: unsupported conversion: {reason}"
                        )),
            "{reason}: {omissions:?}"
        );
    }
}

/// The keys of `layer`'s `property` track as (time, JSON value, easing type).
fn valued_track(document: &Value, layer: u64, property: &str) -> Vec<(i64, Value, String)> {
    let entries = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    let entry = entries
        .iter()
        .find(|entry| {
            entry["target"]["layerId"] == layer && entry["target"]["propertyType"] == property
        })
        .unwrap_or_else(|| panic!("no {property} track on layer {layer}"));
    entry["animator"]["keyframes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|key| {
            (
                key["layerTime"].as_i64().unwrap(),
                key["value"]["value"].clone(),
                key["easing"]["type"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

/// The property types of every track of `layer`.
fn track_properties(document: &Value, layer: u64) -> Vec<String> {
    document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["target"]["layerId"] == layer)
        .map(|entry| entry["target"]["propertyType"].as_str().unwrap().to_owned())
        .collect()
}

/// `text_graphic` whose text shows the documents `keys` from their generator
/// times, the first also before its key.
fn source_text_keyed_graphic(keys: Vec<(i64, crate::schema::text::PrTextDocument)>) -> PrGraphic {
    let mut graphic = text_graphic();
    let text = graphic.text_mut();
    text.document = keys[0].1.clone();
    text.source_text_keys = keys
        .into_iter()
        .map(
            |(source_ticks, document)| crate::schema::text::PrSourceTextKey {
                source_ticks,
                document,
            },
        )
        .collect();
    graphic
}

#[test]
fn mixed_line_shadow_obeys_the_whole_blocks_transform_and_fill_guards() {
    use crate::schema::text::{PrRgb, PrTextLines};
    let mut graphic = text_graphic();
    let mut text = graphic.text().clone();
    text.document.text = "First".into();
    text.document.leading = 0.0;
    text.document.frame = PrTextFrame::Point {
        vertical: PrVerticalAlign::Center,
    };
    text.transform.scale = 100.0;
    text.transform.rotation = 0.0;
    text.document.stroke = None;
    text.document.shadow = Some(PrTextShadow {
        color: PrRgb([0, 0, 0]),
        opacity: 100.0,
        angle: 135.0,
        distance: 3.0,
        size: 6.0,
        blur: 12.0,
    });
    let mut second = text.document.clone();
    second.text = "Second".into();
    second.size = 60.0;
    let block = PrTextLines {
        name: text.name,
        documents: vec![text.document, second],
        transform: text.transform,
        animations: Vec::new(),
    };
    for reason in [
        None,
        Some("its text has Scale or Rotation keys"),
        Some("its text has no fill"),
    ] {
        let mut block = block.clone();
        if reason == Some("its text has Scale or Rotation keys") {
            block.animations = vec![PrPropertyAnimation::Rotation(vec![
                scalar(graphic.in_ticks, 0.0, PrKeyframeEasing::Linear),
                scalar(graphic.in_ticks + TICKS, 45.0, PrKeyframeEasing::Linear),
            ])];
        } else if reason.is_some() {
            block.documents[1].fill = None;
        }
        graphic.objects = vec![PrGraphicObject::TextLines(block)];
        let (document, omissions) = imported(graphic.clone());
        let group = &document["composition"]["layers"][0];
        assert_eq!(group["layers"].as_array().unwrap().len(), 2);
        let effects = group
            .get("effects")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        assert_eq!(effects, usize::from(reason.is_none()));
        if let Some(reason) = reason {
            assert!(
                omissions
                    .iter()
                    .any(|omission| omission.reason
                        == format!("text shadow not converted: {reason}")),
                "{omissions:?}"
            );
        }
    }
}

#[test]
fn point_alignment_uses_local_geometry_and_keeps_empty_lines_and_single_line_controls() {
    let mut graphic = text_graphic();
    let text = graphic.text_mut();
    text.document.text = "First\n\nLast\n".into();
    text.document.frame = PrTextFrame::Point {
        vertical: PrVerticalAlign::Center,
    };
    text.document.size = 50.0;
    text.document.leading = 10.0;
    text.transform.anchor = [7.0, 11.0];
    text.transform.position = [480.0, 270.0];
    text.transform.scale = 150.0;
    text.transform.rotation = 90.0;
    let (document, _) = imported(graphic.clone());
    let layer = &document["composition"]["layers"][0];
    assert_eq!(layer["sourceText"]["text"], "First\n\nLast\n");
    assert!((layer["sourceText"]["leading"].as_f64().unwrap() - 70.0).abs() < 1e-4);
    assert_eq!(layer["transform"]["anchorPoint"][0], 7.0);
    assert!((layer["transform"]["anchorPoint"][1].as_f64().unwrap() - 116.0).abs() < 1e-4);
    // The aligned local first baseline rotates right by 90 degrees; subtracting
    // a world-space Y instead would put it above the unchanged source position.
    let (baseline, _) = place(&document, layer, [0.0, 0.0], 0.0);
    assert!((baseline[0] - 654.0).abs() < 1e-4);
    assert!((baseline[1] - 259.5).abs() < 1e-4);
    for (vertical, expected) in [
        (PrVerticalAlign::Top, 11.0),
        (PrVerticalAlign::Bottom, 221.0),
    ] {
        graphic.text_mut().document.frame = PrTextFrame::Point { vertical };
        let (document, _) = imported(graphic.clone());
        assert!(
            (document["composition"]["layers"][0]["transform"]["anchorPoint"][1]
                .as_f64()
                .unwrap()
                - expected)
                .abs()
                < 1e-4
        );
    }
    for content in ["Before", "After", ""] {
        graphic.text_mut().document.text = content.into();
        graphic.text_mut().document.frame = PrTextFrame::Point {
            vertical: PrVerticalAlign::Center,
        };
        let (document, _) = imported(graphic.clone());
        assert_eq!(
            document["composition"]["layers"][0]["transform"]["anchorPoint"],
            json!([7.0, 11.0])
        );
        assert_eq!(
            document["composition"]["layers"][0]["transform"]["position"],
            json!([480.0, 270.0])
        );
    }
}

#[test]
fn point_text_source_keys_hold_leading_and_alignment_together_and_export_their_meaning() {
    let mut first = text_graphic().text().document.clone();
    first.text = "One\nTwo".into();
    first.size = 50.0;
    first.leading = 0.0;
    first.frame = PrTextFrame::Point {
        vertical: PrVerticalAlign::Center,
    };
    let mut second = first.clone();
    second.text = "One\n\nThree".into();
    second.size = 80.0;
    second.leading = 4.0;
    let mut third = second.clone();
    third.text = "Single".into();
    let start = text_graphic().in_ticks;
    let mut graphic = source_text_keyed_graphic(vec![
        (start - TICKS / 2, first),
        (start + TICKS, second),
        (start + 5 * TICKS / 2, third),
    ]);
    graphic.text_mut().transform.anchor = [7.0, 11.0];
    let (mut document, _) = imported(graphic);
    assert_eq!(
        valued_track(&document, 2, "leading"),
        vec![
            (-500, json!(60.000003814697266), "hold".into()),
            (1000, json!(100.0), "hold".into()),
            (2500, json!(100.0), "hold".into()),
        ]
    );
    let anchors = valued_track(&document, 2, "anchorPointY");
    for ((time, value, easing), (expected_time, expected_value)) in
        anchors
            .iter()
            .zip([(-500, 41.00000190734863), (1000, 111.0), (2500, 11.0)])
    {
        assert_eq!((*time, easing.as_str()), (expected_time, "hold"));
        assert!((value.as_f64().unwrap() - expected_value).abs() < 1e-8);
    }
    // Export the editable imported text without the test video's unrelated media.
    document["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .retain(|layer| layer["type"] == "Text");
    // An enabled anchor track overrides this unused static fallback; changing
    // it must not change either the visible text or the exported alignment.
    document["composition"]["layers"][0]["transform"]["anchorPoint"][1] = json!(999.0);
    let anchor = document["composition"]["dynamics"]["entries"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|entry| entry["target"]["propertyType"] == "anchorPointY")
        .unwrap();
    // The first key has no incoming segment, so its easing is irrelevant.
    anchor["animator"]["keyframes"][0]["easing"]["type"] = json!("linear");
    document["duration"] = json!(3.0);
    let mut canvas = editable_document()["composition"]["layers"][1].clone();
    canvas["id"] = json!(900);
    canvas["activeRange"]["duration"] = json!(document["duration"].as_f64().unwrap() * 1000.0);
    document["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .push(canvas);
    let mut invalid = document.clone();
    let anchor = invalid["composition"]["dynamics"]["entries"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|entry| entry["target"]["propertyType"] == "anchorPointY")
        .unwrap();
    anchor["animator"]["keyframes"][1]["value"]["value"] = json!(112.0);
    let invalid = EditableFxCompositionDocument::from_json_value(invalid).unwrap();
    let mut losses = Vec::new();
    let rejected = tesseract_to_premiere(
        &invalid,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        FrameRate::Fps30,
        &mut losses,
    );
    assert!(rejected.is_err());
    assert!(
        losses.iter().any(|loss| loss
            .reason
            .contains("keyed text anchor does not match held point-text alignment")),
        "{losses:?}"
    );
    let editable = EditableFxCompositionDocument::from_json_value(document).unwrap();
    let mut omissions = Vec::new();
    let project = tesseract_to_premiere(
        &editable,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        FrameRate::Fps30,
        &mut omissions,
    )
    .unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let graphic = project.sequences[0]
        .video_items()
        .find_map(PrVideoItem::graphic)
        .unwrap();
    let text = graphic.text();
    assert_eq!(text.transform.anchor, [7.0, 11.0]);
    assert_eq!(text.source_text_keys.len(), 3);
    assert!(text.source_text_keys.iter().all(|key| key.document.frame
        == PrTextFrame::Point {
            vertical: PrVerticalAlign::Center
        }));
    assert_eq!(text.source_text_keys[1].document.leading, 4.0);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("held-point.prproj");
    PremiereProjectXml::new(&project)
        .unwrap()
        .write_new(&path)
        .unwrap();
    let (read, omissions) = PrProjectFile::load(&path).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let restored = read.sequences[0]
        .video_items()
        .find_map(PrVideoItem::graphic)
        .unwrap();
    let (reimported, _) = imported(restored.clone());
    assert_eq!(valued_track(&reimported, 2, "anchorPointY"), anchors);
}

#[test]
fn source_text_keys_become_hold_tracks_of_the_fields_that_change() {
    use crate::schema::text::{PrRgb, PrTextStroke};
    let base = {
        let mut document = text_graphic().text().document.clone();
        document.stroke = None;
        document
    };
    let start = text_graphic().in_ticks;
    let hold = |keys: &[(i64, Value)]| -> Vec<(i64, Value, String)> {
        keys.iter()
            .map(|(time, value)| (*time, value.clone(), "hold".to_owned()))
            .collect()
    };
    // (a) Text only, keys before, inside and after the placement.
    let texts = source_text_keyed_graphic(
        ["One", "Two", "Three"]
            .into_iter()
            .zip([start - TICKS / 2, start + TICKS, start + 5 * TICKS / 2])
            .map(|(text, ticks)| {
                let mut document = base.clone();
                document.text = text.into();
                (ticks, document)
            })
            .collect(),
    );
    let (document, omissions) = imported(texts);
    assert_eq!(omissions.len(), 1, "{omissions:?}");
    assert_eq!(track_properties(&document, 2), ["textContent"]);
    assert_eq!(
        valued_track(&document, 2, "textContent"),
        hold(&[
            (-500, json!("One")),
            (1000, json!("Two")),
            (2500, json!("Three"))
        ])
    );
    let layer = &document["composition"]["layers"][0];
    assert_eq!(layer["sourceText"]["text"], "One");
    assert_eq!(layer["sourceText"]["applyStroke"], false);

    // (b) One key changes the text, size and fill and switches a stroke on;
    // the static leading 50 keys the line spacing with the size, and the
    // stroke the keys switch on is the layer's static stroke.
    let mut second = base.clone();
    second.text = "Two".into();
    second.size = 100.0;
    second.fill = Some(PrRgb([0, 0, 255]));
    second.stroke = Some(PrTextStroke {
        color: PrRgb([0, 0, 0]),
        width: 3.0,
    });
    let styled = source_text_keyed_graphic(vec![(start, base.clone()), (start + TICKS, second)]);
    let (document, omissions) = imported(styled);
    assert_eq!(omissions.len(), 1, "{omissions:?}");
    let mut properties = track_properties(&document, 2);
    properties.sort();
    assert_eq!(
        properties,
        [
            "fillColor",
            "fontSize",
            "leading",
            "strokeEnabled",
            "textContent"
        ]
    );
    assert_eq!(
        valued_track(&document, 2, "fontSize"),
        hold(&[(0, json!(80.0)), (1000, json!(100.0))])
    );
    assert_eq!(
        valued_track(&document, 2, "fillColor"),
        hold(&[
            (0, json!([1.0, 0.0, 0.0, 1.0])),
            (1000, json!([0.0, 0.0, 1.0, 1.0]))
        ])
    );
    assert_eq!(
        valued_track(&document, 2, "leading"),
        hold(&[
            (0, json!(146.0)),
            (1000, json!(automatic_line_spacing(100.0) + 50.0))
        ])
    );
    assert_eq!(
        valued_track(&document, 2, "strokeEnabled"),
        hold(&[(0, json!(false)), (1000, json!(true))])
    );
    let layer = &document["composition"]["layers"][0];
    assert_eq!(layer["sourceText"]["applyStroke"], false);
    assert_eq!(
        layer["sourceText"]["strokeColor"],
        json!([0.0, 0.0, 0.0, 1.0])
    );
    assert_eq!(layer["sourceText"]["strokeWidth"], 6.0);

    // (c) Tracking and all caps key their own tracks and read back after
    // export.
    let mut spaced = base.clone();
    spaced.tracking = 40.0;
    spaced.all_caps = false;
    let styled = source_text_keyed_graphic(vec![(start, base.clone()), (start + TICKS, spaced)]);
    let (document, _) = imported(styled.clone());
    let mut properties = track_properties(&document, 2);
    properties.sort();
    assert_eq!(properties, ["allCaps", "textContent", "tracking"]);
    assert_eq!(
        valued_track(&document, 2, "tracking"),
        hold(&[(0, json!(-20.0)), (1000, json!(40.0))])
    );
    assert_eq!(
        valued_track(&document, 2, "allCaps"),
        hold(&[(0, json!(true)), (1000, json!(false))])
    );
    let back = round_tripped(styled);
    assert_eq!(
        back.text()
            .source_text_keys
            .iter()
            .map(|key| (key.document.tracking, key.document.all_caps))
            .collect::<Vec<_>>(),
        [(-20.0, true), (40.0, false)]
    );
}

/// `graphic` imported, exported again without the video layer, written and
/// read back by the crate reader.
fn round_tripped(graphic: PrGraphic) -> PrGraphic {
    let (mut document, _) = imported(graphic);
    document["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .remove(1);
    let document = EditableFxCompositionDocument::from_json_value(document).unwrap();
    let mut omissions = Vec::new();
    let project = tesseract_to_premiere(
        &document,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        FrameRate::Fps30,
        &mut omissions,
    )
    .unwrap_or_else(|error| panic!("{error}: {omissions:?}"));
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("round-trip.prproj");
    PremiereProjectXml::new(&project)
        .unwrap()
        .write_new(&path)
        .unwrap();
    let (reopened, _) = PrProjectFile::load(&path).unwrap();
    let graphic = reopened.sequences[0]
        .video_items()
        .find_map(PrVideoItem::graphic)
        .expect("the written graphic loads")
        .clone();
    graphic
}

#[test]
fn source_text_fill_keys_after_an_unfilled_key_keep_the_first_enabled_fill() {
    use crate::schema::text::PrRgb;
    let (red, blue) = (PrRgb([255, 0, 0]), PrRgb([0, 0, 255]));
    let start = text_graphic().in_ticks;
    // No stroke: Premiere text cannot be an outline without a fill.
    let unfilled = {
        let mut document = text_graphic().text().document.clone();
        document.fill = None;
        document.stroke = None;
        document
    };
    let hold = |keys: &[(i64, Value)]| -> Vec<(i64, Value, String)> {
        keys.iter()
            .map(|(time, value)| (*time, value.clone(), "hold".to_owned()))
            .collect()
    };
    // (fills of the keys, one second apart; the FillColor track when the
    // enabled fills differ; the fills read back after export)
    for (fills, color_track) in [
        (vec![None, Some(red)], None),
        (
            vec![None, Some(red), Some(blue)],
            Some(hold(&[
                (0, json!([1.0, 0.0, 0.0, 1.0])),
                (1000, json!([1.0, 0.0, 0.0, 1.0])),
                (2000, json!([0.0, 0.0, 1.0, 1.0])),
            ])),
        ),
    ] {
        let graphic = source_text_keyed_graphic(
            fills
                .iter()
                .enumerate()
                .map(|(index, fill)| {
                    let mut document = unfilled.clone();
                    document.fill = *fill;
                    (start + index as i64 * TICKS, document)
                })
                .collect(),
        );
        let (document, omissions) = imported(graphic.clone());
        assert_eq!(omissions.len(), 1, "{omissions:?}");
        let mut properties = track_properties(&document, 2);
        properties.sort();
        let mut expected = vec!["fillEnabled", "textContent"];
        if color_track.is_some() {
            expected.insert(0, "fillColor");
        }
        assert_eq!(properties, expected, "{fills:?}");
        // The fill the keys switch on is the layer's static color.
        let layer = &document["composition"]["layers"][0];
        assert_eq!(layer["sourceText"]["applyFill"], false, "{fills:?}");
        assert_eq!(
            layer["sourceText"]["fillColor"],
            json!([1.0, 0.0, 0.0, 1.0]),
            "{fills:?}"
        );
        assert_eq!(
            valued_track(&document, 2, "fillEnabled"),
            hold(
                &fills
                    .iter()
                    .enumerate()
                    .map(|(index, fill)| (1000 * index as i64, json!(fill.is_some())))
                    .collect::<Vec<_>>()
            ),
            "{fills:?}"
        );
        if let Some(color_track) = color_track {
            assert_eq!(valued_track(&document, 2, "fillColor"), color_track);
        }
        // Exported by Hold and read back, every key has its own fill.
        let back = round_tripped(graphic);
        assert_eq!(
            back.text()
                .source_text_keys
                .iter()
                .map(|key| key.document.fill)
                .collect::<Vec<_>>(),
            fills,
        );
    }
}

/// A keyframe track entry for text layer 9 with JSON `values`.
fn valued_entry(property: &str, keys: &[(i64, Value, &str)]) -> Value {
    json!({
        "target": {"kind": "layer", "layerId": 9, "propertyType": property},
        "animator": {"type": "keyframes", "enabled": true, "keyframes": keys.iter().enumerate().map(|(index, (time, value, easing))| json!({
            "id": format!("{property}-{index}"),
            "layerTime": time,
            "value": value,
            "easing": {"type": easing},
        })).collect::<Vec<_>>()}
    })
}

fn string_key(time: i64, text: &str) -> (i64, Value, &'static str) {
    (time, json!({"type": "string", "value": text}), "hold")
}

#[test]
fn source_text_tracks_export_as_one_held_document_per_key_time() {
    use fx_schema::animator::KeyframeId;
    let layer = text_layer(&text_graphic(), text_graphic().text(), LayerId::new(9), 0).unwrap();
    let key = |name: &str, millis: i64, value: PropertyValue| {
        PropertyKeyframe::new(
            KeyframeId::new(format!("{name}-{millis}")),
            TimeOffset::from_millis(millis),
            value,
            PropertyKeyframeEasing::Hold,
        )
    };
    let text = PropertyKeyframeTrack::new(vec![
        key("text", 0, PropertyValue::String("One".into())),
        key("text", 500, PropertyValue::String("Two".into())),
    ])
    .unwrap();
    let size = PropertyKeyframeTrack::new(vec![
        key("size", 0, PropertyValue::Float(80.0)),
        key("size", 1000, PropertyValue::Float(120.0)),
    ])
    .unwrap();
    let stroke = PropertyKeyframeTrack::new(vec![
        key("stroke", 250, PropertyValue::Bool(false)),
        key("stroke", 1000, PropertyValue::Bool(true)),
    ])
    .unwrap();
    let tracks = BTreeMap::from([
        (SourceTextField::Text, &text),
        (SourceTextField::Size, &size),
        (SourceTextField::StrokeEnabled, &stroke),
    ]);
    let keys = source_text_keys(&tracks, &layer.source_text, "OpenSans-Bold", EXPORT_IN).unwrap();
    // One key per distinct time; each field holds its last key, and the
    // first key before it. The FX line spacing 146 px stays, so the native
    // leading above 120 % of the size shrinks as the size grows.
    assert_eq!(
        keys.iter()
            .map(|key| {
                let document = &key.document;
                (
                    (key.source_ticks - EXPORT_IN) * 1000 / TICKS,
                    document.text.as_str(),
                    document.size,
                    document.stroke.is_some(),
                    document.leading,
                )
            })
            .collect::<Vec<_>>(),
        [
            (0, "One", 80.0, false, 50.0),
            (250, "One", 80.0, false, 50.0),
            (500, "Two", 80.0, false, 50.0),
            (1000, "Two", 120.0, true, 2.0),
        ]
    );
    assert_eq!(keys[3].document.stroke.unwrap().width, 5.0);
}

#[test]
fn source_text_tracks_write_as_keys_and_read_back_as_the_same_tracks() {
    // Text keys before, inside and after the one-second placement, and a
    // size key at a time the text lacks. The keys read back on the same
    // layer clock.
    let text = valued_entry(
        "textContent",
        &[
            string_key(-500, "One"),
            string_key(250, "Two"),
            string_key(1500, "Three"),
        ],
    );
    let size = entry("fontSize", &[(250, 80.0, "hold"), (750, 120.0, "hold")]);
    let (key_lists, graphic, omissions) = written_graphic(vec![text, size]);
    assert_eq!(omissions, []);
    // One key list, `ticks,base64;` per key on the generator clock, and one
    // document per key time of either track.
    assert_eq!(key_lists.len(), 1);
    let keys: Vec<_> = key_lists[0]
        .split_terminator(';')
        .map(|key| key.split(',').collect::<Vec<_>>())
        .collect();
    assert!(keys.iter().all(|fields| fields.len() == 2), "{keys:?}");
    let text = graphic.text();
    assert_eq!(
        text.source_text_keys
            .iter()
            .map(|key| {
                (
                    (key.source_ticks - EXPORT_IN) * 1000 / TICKS,
                    key.document.text.as_str(),
                    key.document.size,
                )
            })
            .collect::<Vec<_>>(),
        [
            (-500, "One", 80.0),
            (250, "Two", 80.0),
            (750, "Two", 120.0),
            (1500, "Three", 120.0),
        ]
    );
    assert_eq!(text.document, text.source_text_keys[0].document);
    // Reimported, each field that changes has one Hold key per Source Text
    // key, so the tracks hold the exported values at every time, with the
    // size repeated at the text times. FX leading is the whole line spacing,
    // so the size keys also key the leading at 120 % of each size.
    let (mut document, _) = imported(graphic.clone());
    assert_eq!(
        track_properties(&document, 2),
        ["textContent", "fontSize", "leading"]
    );
    assert_eq!(
        valued_track(&document, 2, "textContent"),
        [
            (-500, json!("One"), "hold".to_owned()),
            (250, json!("Two"), "hold".to_owned()),
            (750, json!("Two"), "hold".to_owned()),
            (1500, json!("Three"), "hold".to_owned()),
        ]
    );
    assert_eq!(
        valued_track(&document, 2, "fontSize"),
        [
            (-500, json!(80.0), "hold".to_owned()),
            (250, json!(80.0), "hold".to_owned()),
            (750, json!(120.0), "hold".to_owned()),
            (1500, json!(120.0), "hold".to_owned()),
        ]
    );
    assert_eq!(
        valued_track(&document, 2, "leading"),
        [(-500, 80.0), (250, 80.0), (750, 120.0), (1500, 120.0)].map(|(time, size)| (
            time,
            json!(automatic_line_spacing(size)),
            "hold".to_owned()
        ))
    );
    // Exported again without the video layer, the reimported tracks write the
    // same Source Text keys.
    document["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .remove(1);
    let document = EditableFxCompositionDocument::from_json_value(document).unwrap();
    let mut omissions = Vec::new();
    let project = tesseract_to_premiere(
        &document,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        FrameRate::Fps30,
        &mut omissions,
    )
    .unwrap();
    let reexported = project
        .single_sequence()
        .unwrap()
        .video_items()
        .find_map(PrVideoItem::graphic)
        .expect("the reimported text exports");
    assert_eq!(reexported.text().source_text_keys, text.source_text_keys);
}

#[test]
fn a_static_shadow_exports_on_every_source_text_key() {
    // The text's drop shadow reaches the document and each key document, so
    // the written project validates (the keys must share the document's
    // shadow) and reads back with the shadow on every key.
    let text = valued_entry(
        "textContent",
        &[string_key(0, "One"), string_key(500, "Two")],
    );
    let shadow = json!([{"id": 5, "effect": {"type": "dropShadow", "offset": [3, 3]}}]);
    let (graphic, omissions) = exported(vec![text.clone()], Some(shadow.clone()));
    assert_eq!(omissions, []);
    let exported_shadow = graphic.text().document.shadow.expect("the shadow exports");
    assert!(graphic
        .text()
        .source_text_keys
        .iter()
        .all(|key| key.document.shadow == Some(exported_shadow)));
    graphic.validate(FrameRate::Fps30).unwrap();

    // Written beside the canvas and read back.
    let (project, omissions) = export_over_canvas(
        vec![{
            let mut group = graphic_group(json!({}));
            group["layers"][0]["effects"] = shadow;
            group
        }],
        vec![text.clone()],
    );
    assert_eq!(omissions, []);
    let project = project.unwrap();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("shadowed.prproj");
    PremiereProjectXml::new(&project)
        .unwrap()
        .write_new(&path)
        .unwrap();
    let (reopened, omissions) = PrProjectFile::load(&path).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let read = reopened.sequences[0]
        .video_items()
        .find_map(PrVideoItem::graphic)
        .unwrap();
    assert_eq!(read.text().document.shadow, Some(exported_shadow));
    assert_eq!(read.text().source_text_keys.len(), 2);

    // What Premiere casts from a key without a fill is unmeasured, so such a
    // text keeps its keys and reports the shadow, as unfilled static text does.
    let fill_switch = valued_entry(
        "fillEnabled",
        &[
            (0, json!({"type": "bool", "value": true}), "hold"),
            (500, json!({"type": "bool", "value": false}), "hold"),
        ],
    );
    let (graphic, omissions) = exported(
        vec![text, fill_switch],
        Some(json!([{"id": 5, "effect": {"type": "dropShadow", "offset": [3, 3]}}])),
    );
    assert_eq!(
        omissions
            .iter()
            .map(|omission| (omission.scope, omission.reason.as_str()))
            .collect::<Vec<_>>(),
        [(
            OmissionScope::Feature,
            "drop shadow 5 was not exported: unsupported conversion: its text has a Source Text key without a fill"
        )]
    );
    assert_eq!(graphic.text().document.shadow, None);
    assert_eq!(graphic.text().source_text_keys.len(), 2);
}

#[test]
fn source_text_tracks_premiere_cannot_hold_omit_the_graphic() {
    // The FX document itself refuses a string or boolean key with continuous
    // easing, so only numeric and color tracks can arrive with one.
    let text = valued_entry(
        "textContent",
        &[string_key(0, "One"), string_key(500, "Two")],
    );
    for (entries, reason) in [
        (
            vec![
                text.clone(),
                entry("fontSize", &[(0, 80.0, "hold"), (500, 120.0, "linear")]),
            ],
            "Source Text size keys must hold: Premiere holds Source Text between keys",
        ),
        (
            vec![
                text,
                entry("fontSize", &[(0, 80.0, "hold"), (500, 0.0, "hold")]),
            ],
            "Source Text size keys must be positive",
        ),
    ] {
        let mut wire = editable_document();
        wire["composition"]["layers"][0] = json!({
            "type": "Text",
            "id": 9,
            "name": "Title",
            "activeRange": {"start": 0, "duration": 1000},
            "transform": {"anchorPoint": [0, 0], "position": [960, 540], "scale": [100, 100], "rotation": 0, "opacity": 100},
            "sourceText": {"text": "Keys", "fontFamily": "Inter-Bold", "fontStyle": "", "fontSize": 80, "fillColor": [1, 1, 1, 1]},
        });
        wire["composition"]["dynamics"] = json!({ "entries": entries });
        let document = EditableFxCompositionDocument::from_json_value(wire).unwrap();
        let mut omissions = Vec::new();
        let result = tesseract_to_premiere(
            &document,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            FrameRate::Fps30,
            &mut omissions,
        );
        // The text was the only layer, so its omission ends the export.
        assert!(result.is_err(), "{reason}");
        let expected = format!("text layer was not exported: unsupported conversion: {reason}");
        assert!(
            omissions
                .iter()
                .any(|omission| omission.scope == OmissionScope::Occurrence
                    && omission.record == "layer 9 (\"Title\")"
                    && omission.reason == expected),
            "{reason}: {omissions:?}"
        );
    }
}

/// A clip Motion like a Source Graphic placement's, with unequal axes.
fn placement_motion() -> crate::schema::PrStaticTransform {
    crate::schema::PrStaticTransform {
        position: [0.25, 0.75],
        anchor_point: [0.5, 0.5],
        scale: [50.0, 80.0],
        rotation: 30.0,
    }
}

#[test]
fn a_direct_graphic_with_clip_motion_and_an_opacity_mask_is_rejected() {
    let graphic = PrGraphic {
        clip_motion: placement_motion(),
        opacity_mask: Some(crate::tests::support::opacity_mask()),
        ..text_graphic()
    };
    let reason = "a graphic with clip Motion and a clip Opacity mask is not converted: its mask frame is unmeasured";
    assert!(graphic
        .validate(FrameRate::Fps30)
        .unwrap_err()
        .to_string()
        .contains(reason));

    // The typed mapper can also be called without model validation.
    let mut sequence = video_sequence();
    sequence.video_tracks.push(PrVideoTrack {
        items: vec![PrVideoItem::Graphic(graphic)],
        transitions: Vec::new(),
        nests: Vec::new(),
    });
    let media = video_media();
    let error = premiere_to_tesseract(
        &sequence,
        &media,
        &crate::tesseract_output::asset_ids_in_order(&sequence, &media),
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(error.to_string().contains(reason), "{error}");
}

#[test]
fn a_clip_motion_moves_the_graphic_root_once_from_a_group() {
    use crate::schema::PrBlendMode;
    // A disabled graphic in Screen at clip Opacity 50: its content group
    // keeps the Opacity and blend, as without the Motion, and the Motion group
    // takes the placement's range, visibility and Motion.
    let (document, _) = imported(PrGraphic {
        clip_motion: placement_motion(),
        opacity: 50.0,
        blend_mode: PrBlendMode::Screen,
        enabled: false,
        ..text_graphic()
    });
    let moved = &document["composition"]["layers"][0];
    let [content] = moved["layers"].as_array().unwrap().as_slice() else {
        panic!("a Motion group holds one root: {moved}");
    };
    let [text] = content["layers"].as_array().unwrap().as_slice() else {
        panic!("the content group holds the text: {content}");
    };
    assert_eq!(
        (&moved["type"], &moved["name"], &moved["isHidden"]),
        (
            &json!("Group"),
            &json!("Premiere graphic Motion 2"),
            &json!(true)
        )
    );
    assert_eq!(
        *crate::test_support::layer_range(moved),
        json!({"start": 1000, "duration": 2000})
    );
    let transform = &moved["transform"];
    assert_eq!(
        ["anchorPoint", "position", "scale", "rotation", "opacity"].map(|field| &transform[field]),
        [
            &json!([960.0, 540.0]),
            &json!([480.0, 810.0]),
            &json!([50.0, 80.0]),
            &json!(30.0),
            &json!(100.0)
        ]
    );
    assert_eq!(moved["blendMode"], "normal");
    assert_eq!(
        (
            &content["name"],
            &content["parent"],
            content.get("isHidden")
        ),
        (&json!("Premiere graphic 2"), &moved["id"], None)
    );
    assert_eq!(
        *crate::test_support::layer_range(content),
        json!({"start": 0, "duration": 2000})
    );
    assert_eq!(
        (&content["blendMode"], &content["transform"]["opacity"]),
        (&json!("screen"), &json!(50.0))
    );
    // The text keeps the item's id and its own transform; the groups take
    // fresh ones.
    assert_eq!((&text["id"], &text["parent"]), (&json!(2), &content["id"]));
    assert_eq!(text["transform"]["scale"], json!([80.0, 80.0]));
    let ids = [&moved["id"], &content["id"], &text["id"], &json!(1)];
    let unique: std::collections::BTreeSet<_> = ids.iter().map(|id| id.as_u64()).collect();
    assert_eq!(unique.len(), ids.len(), "{ids:?}");

    // An ungrouped root keeps its keys on its own clock under the group.
    let (unmoved, _) = imported(keyed_graphic());
    let (moved, _) = imported(PrGraphic {
        clip_motion: placement_motion(),
        ..keyed_graphic()
    });
    let text = &moved["composition"]["layers"][0]["layers"][0];
    assert_eq!(
        (&text["type"], &text["parent"], &text["activeRange"]),
        (
            &json!("Text"),
            &moved["composition"]["layers"][0]["id"],
            &json!({"start": 0, "duration": 2000})
        )
    );
    for property in ["positionX", "scaleX", "rotation", "opacity"] {
        assert_eq!(
            track(&moved, 2, property),
            track(&unmoved, 2, property),
            "{property}"
        );
    }
}

/// A shape object named `name`: a `size` px square about `centre`, filled
/// with `color`, and a Mask with Shape when `mask_source` says so.
fn square_object(
    name: &str,
    centre: [f64; 2],
    size: f32,
    color: [u8; 3],
    mask_source: Option<crate::schema::text::PrMaskSource>,
) -> PrGraphicObject {
    use crate::schema::text::{PrFill, PrRgb};
    let Some(PrGraphicObject::Shape(mut shape)) = shape_graphic().objects.pop() else {
        unreachable!("the test graphic holds one shape");
    };
    let half = size / 2.0;
    for (vertex, [x, y]) in shape.path.vertices.iter_mut().zip([
        [-half, -half],
        [half, -half],
        [half, half],
        [-half, half],
    ]) {
        vertex.point = [x, y];
        vertex.in_tangent = [x, y];
        vertex.out_tangent = [x, y];
    }
    shape.name = name.into();
    shape.transform.position = centre;
    shape.appearance.fill = Some(PrFill::Solid(PrRgb(color)));
    shape.appearance.mask_source = mask_source;
    PrGraphicObject::Shape(shape)
}

fn subgroup(name: &str, objects: Vec<PrGraphicObject>) -> PrGraphicObject {
    PrGraphicObject::Group(crate::schema::text::PrGraphicGroup {
        name: name.into(),
        objects,
    })
}

const INVERTED: Option<crate::schema::text::PrMaskSource> =
    Some(crate::schema::text::PrMaskSource { inverted: true });
const NOT_INVERTED: Option<crate::schema::text::PrMaskSource> =
    Some(crate::schema::text::PrMaskSource { inverted: false });

/// `graphic` imported beside the video, exported without the video layer,
/// written, read back by the crate reader, and every omission of the three.
fn written_back(graphic: PrGraphic) -> (PrGraphic, Vec<Omission>) {
    let (mut document, mut omissions) = imported(graphic);
    document["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .retain(|layer| layer["type"] != "Video");
    let document = EditableFxCompositionDocument::from_json_value(document).unwrap();
    let project = tesseract_to_premiere(
        &document,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        FrameRate::Fps30,
        &mut omissions,
    )
    .unwrap_or_else(|error| panic!("{error}: {omissions:?}"));
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("masks.prproj");
    PremiereProjectXml::new(&project)
        .unwrap()
        .write_new(&path)
        .unwrap();
    let (reopened, read) = PrProjectFile::load(&path).unwrap();
    omissions.extend(read);
    let graphic = reopened.sequences[0]
        .video_items()
        .find_map(PrVideoItem::graphic)
        .expect("the written graphic loads")
        .clone();
    (graphic, omissions)
}

#[test]
fn masks_with_shape_and_subgroups_round_trip_through_fx_and_the_written_project() {
    let blue = [0, 96, 255];
    let square = |name: &str, x: f64, mask| square_object(name, [x, 540.0], 200.0, blue, mask);
    for (case, objects) in [
        (
            "an inverted mask over two objects (s4)",
            vec![
                square("M", 960.0, INVERTED),
                square("T1", 900.0, None),
                square("T2", 1020.0, None),
            ],
        ),
        (
            "an object above a mask (s4 A)",
            vec![
                square("A", 700.0, None),
                square("M", 960.0, NOT_INVERTED),
                square("T", 960.0, None),
            ],
        ),
        (
            "two masks with an object between (s9)",
            vec![
                square("M1", 800.0, INVERTED),
                square("X", 700.0, None),
                square("M2", 1100.0, INVERTED),
                square("T", 960.0, None),
            ],
        ),
        (
            "a mask in a SubGroup over its member (b0)",
            vec![
                subgroup(
                    "Group 01",
                    vec![square("M", 960.0, INVERTED), square("T1", 900.0, None)],
                ),
                square("T2", 1020.0, None),
            ],
        ),
        (
            "a mask with nothing below it (s10)",
            vec![square("T", 960.0, None), square("M", 960.0, INVERTED)],
        ),
        (
            "a SubGroup without masks",
            vec![
                square("A", 700.0, None),
                subgroup(
                    "Pair",
                    vec![square("B", 900.0, None), square("C", 1100.0, None)],
                ),
            ],
        ),
    ] {
        let graphic = PrGraphic {
            objects,
            ..shape_graphic()
        };
        let (written, omissions) = written_back(graphic.clone());
        assert_eq!(written.objects, graphic.objects, "{case}: {omissions:?}");
        // The export without the video ends with the graphic.
        assert!(
            omissions
                .iter()
                .all(|omission| omission.kind == OmissionKind::Approximated
                    || omission.record == "document.duration"),
            "{case}: {omissions:?}"
        );
    }
}

fn exported_and_read(layers: Vec<Value>) -> (Vec<PrGraphic>, Vec<Omission>) {
    let (project, mut omissions) = export_over_canvas(layers, Vec::new());
    let project = project.unwrap_or_else(|error| panic!("{error}: {omissions:?}"));
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("edited.prproj");
    PremiereProjectXml::new(&project)
        .unwrap()
        .write_new(&path)
        .unwrap();
    let (reopened, read) = PrProjectFile::load(&path).unwrap();
    omissions.extend(read);
    let graphics = reopened.sequences[0]
        .video_items()
        .filter_map(PrVideoItem::graphic)
        .cloned()
        .collect();
    (graphics, omissions)
}

/// The FX graphic group that importing `objects` makes, on a one-second
/// clock from zero with every layer inside it, to edit and export again.
fn imported_graphic_group(objects: Vec<PrGraphicObject>) -> Value {
    fn reclock(layers: &mut Value) {
        for layer in layers.as_array_mut().unwrap() {
            if layer["type"] == "Group" {
                layer["playback"] = crate::test_support::linear_playback(
                    json!({"start": 0, "duration": 1000}),
                    json!({"start": 0, "duration": 1000}),
                );
            } else {
                layer["activeRange"] = json!({"start": 0, "duration": 1000});
            }
            if let Some(children) = layer.get_mut("layers") {
                reclock(children);
            }
        }
    }
    let (document, _) = imported(PrGraphic {
        objects,
        ..shape_graphic()
    });
    let mut graphic = document["composition"]["layers"][0].clone();
    graphic["playback"] = crate::test_support::linear_playback(
        json!({"start": 0, "duration": 1000}),
        json!({"start": 0, "duration": 1000}),
    );
    reclock(&mut graphic["layers"]);
    graphic
}

/// The names of `objects` at every depth, a SubGroup's as `name{…}`.
fn part_names(objects: &[PrGraphicObject]) -> String {
    objects
        .iter()
        .map(|object| match object {
            PrGraphicObject::Text(text) => text.name.clone(),
            PrGraphicObject::TextLines(text) => text.name.clone(),
            PrGraphicObject::Shape(shape) => shape.name.clone(),
            PrGraphicObject::Group(group) => {
                format!("{}{{{}}}", group.name, part_names(&group.objects))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The object names of each of `graphics` but [`sibling_text`]'s.
fn exported_names(graphics: &[PrGraphic]) -> Vec<String> {
    graphics
        .iter()
        .map(|graphic| part_names(&graphic.objects))
        .filter(|names| names != "Sibling")
        .collect()
}

#[test]
fn a_mask_with_shape_whose_shadow_does_not_import_takes_its_composite_with_it() {
    use crate::schema::text::{PrRgb, SHAPE_SHADOW_ANGLE};
    let blue = [0, 96, 255];
    let hard = PrTextShadow {
        color: PrRgb([40; 3]),
        opacity: 100.0,
        angle: SHAPE_SHADOW_ANGLE,
        distance: 20.0,
        size: 30.0,
        blur: 0.0,
    };
    let graphic = |mask, shadow: PrTextShadow, scale: f64| {
        let mut source = square_object("M", [960.0, 540.0], 200.0, blue, mask);
        if let PrGraphicObject::Shape(shape) = &mut source {
            shape.appearance.shadow = Some(shadow);
            shape.transform.scale = scale;
        }
        PrGraphic {
            objects: vec![
                square_object("A", [700.0, 540.0], 200.0, blue, None),
                source,
                square_object("T1", [900.0, 540.0], 200.0, blue, None),
                square_object("T2", [1020.0, 540.0], 200.0, blue, None),
            ],
            ..shape_graphic()
        }
    };
    let layer_names = |document: &Value| -> Vec<String> {
        document["composition"]["layers"][0]["layers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|layer| {
                format!(
                    "{} {}",
                    layer["type"].as_str().unwrap(),
                    layer["name"].as_str().unwrap()
                )
            })
            .collect()
    };
    // A shadow that imports stays in the mask: FX draws it into the matte,
    // as it joins Premiere's hole (s7).
    let (document, omissions) = imported(graphic(INVERTED, hard, 100.0));
    assert_eq!(
        layer_names(&document),
        ["Shape A", "Shape M", "Group Premiere masked objects 2"],
        "{omissions:?}"
    );
    assert_eq!(
        document["composition"]["layers"][0]["layers"][1]["effects"][0]["effect"]["type"],
        "dropShadow"
    );
    // One that does not import would change what the mask keeps, in either
    // polarity: the mask and the objects it masks stay out, and A stays.
    for (case, mask, shadow, scale, reason) in [
        (
            "an inverted mask with a soft shadow",
            INVERTED,
            PrTextShadow { blur: 40.0, ..hard },
            100.0,
            "a shape shadow's blur is not measured",
        ),
        (
            "a mask with a soft shadow",
            NOT_INVERTED,
            PrTextShadow { blur: 40.0, ..hard },
            100.0,
            "a shape shadow's blur is not measured",
        ),
        (
            "an inverted mask with a translucent shadow",
            INVERTED,
            PrTextShadow {
                opacity: 60.0,
                ..hard
            },
            100.0,
            "a shape shadow's opacity blend is not measured",
        ),
        (
            "a scaled inverted mask with a hard shadow",
            INVERTED,
            hard,
            50.0,
            "its shape is scaled or rotated",
        ),
    ] {
        let (document, omissions) = imported(graphic(mask, shadow, scale));
        assert_eq!(layer_names(&document), ["Shape A"], "{case}: {omissions:?}");
        assert!(!document.to_string().contains("trackMatte"), "{case}");
        let reports: Vec<_> = omissions
            .iter()
            .filter(|omission| omission.reason.contains(reason))
            .collect();
        assert_eq!(reports.len(), 1, "{case}: {omissions:?}");
        assert_eq!(reports[0].scope, OmissionScope::Feature, "{case}");
        assert!(
            reports[0]
                .reason
                .ends_with("the mask and the 2 objects below it are not converted"),
            "{case}: {omissions:?}"
        );
        assert!(
            omissions
                .iter()
                .all(|omission| omission.reason != super::MASK_SOURCE_LINEAR_LIGHT_APPROXIMATION),
            "{case}: {omissions:?}"
        );
    }
    // A mask above everything takes the whole graphic, which then is not
    // converted at all, as the reader omits a graphic with no object left.
    let mut graphic = graphic(INVERTED, PrTextShadow { blur: 40.0, ..hard }, 100.0);
    graphic.objects.remove(0);
    let (document, omissions) = imported(graphic);
    let types: Vec<_> = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|layer| layer["type"].as_str().unwrap())
        .collect();
    assert_eq!(types, ["Video", "Rect"], "{omissions:?}");
    assert!(
        omissions
            .iter()
            .any(|omission| omission.scope == OmissionScope::Occurrence
                && omission.reason == "graphic was not converted: none of its objects converts"),
        "{omissions:?}"
    );
}

/// Freshly parse the pinned native source, import its selected ordinary graphic,
/// and retain both source semantics and the editable root. This is structural
/// source evidence, not an independent render or Adobe writer acceptance test.
fn native_mask_graphic(case: &str, occurrence: &str) -> (PrGraphic, Value) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
        "tests/fixtures/feature_graphic_masks_{case}_26_5.prproj"
    ));
    let (project, read) = PrProjectFile::load(&path).unwrap();
    let sequence = project.single_sequence().unwrap();
    let native = sequence
        .video_items()
        .filter_map(PrVideoItem::graphic)
        .find(|graphic| graphic.id() == Some(occurrence))
        .unwrap_or_else(|| panic!("{occurrence}: {read:?}"))
        .clone();
    let mut omissions = Vec::new();
    let document = premiere_to_tesseract(
        sequence,
        &project.media,
        &crate::tesseract_output::asset_ids_in_order(sequence, &project.media),
        &mut omissions,
    )
    .unwrap();
    let start = (native.start_ticks / (TICKS / 1000)) as u64;
    let root = document
        .composition()
        .layers()
        .iter()
        .find(|layer| {
            matches!(layer.data(),
        LayerData::Group(group) if group.playback.input_range().start.as_millis() == start
            && group.name.starts_with("Premiere graphic"))
        })
        .unwrap_or_else(|| panic!("{occurrence}: {omissions:?}"));
    (native, serde_json::to_value(root).unwrap())
}

#[test]
fn adobe_shape_and_uniform_text_mask_sources_import_composites_and_export_current_edits() {
    for (case, occurrence, kind) in [
        ("a", "VideoClipTrackItem:67", "Shape"),
        ("b", "VideoClipTrackItem:68", "Text"),
    ] {
        let (native, mut root) = native_mask_graphic(case, occurrence);
        assert!(native.objects[0].mask_source().is_some());
        let layers = root["layers"].as_array_mut().unwrap();
        assert_eq!(layers.len(), 2);
        assert_eq!(layers[0]["type"], kind);
        assert_eq!(layers[1]["type"], "Group");
        assert_eq!(layers[1]["trackMatte"]["layer"], layers[0]["id"]);
        assert!(!layers[1]["layers"].as_array().unwrap().is_empty());
        layers[0]["transform"]["opacity"] = json!(37.0);
        if kind == "Text" {
            layers[0]["sourceText"]["text"] = json!("Edited mask");
        }
        // Place this edited root in the existing two-second offline export harness.
        fn reclock(layer: &mut Value) {
            let range = json!({"start": 0, "duration": 1000});
            if layer["type"] == "Group" {
                layer["playback"] = crate::test_support::linear_playback(range.clone(), range);
                for child in layer["layers"].as_array_mut().unwrap() {
                    reclock(child);
                }
            } else {
                layer["activeRange"] = range;
            }
        }
        reclock(&mut root);
        // A distinct sibling avoids colliding with IDs allocated by native import.
        let mut sibling = sibling_text();
        sibling["id"] = json!(90000);
        let (graphics, omissions) = exported_and_read(vec![root, sibling]);
        let written = graphics
            .iter()
            .find(|graphic| {
                graphic
                    .objects
                    .iter()
                    .any(|object| object.mask_source().is_some())
            })
            .unwrap_or_else(|| panic!("{case}: {omissions:?}"));
        assert_eq!(
            written.objects[0].mask_source(),
            native.objects[0].mask_source()
        );
        match &written.objects[0] {
            PrGraphicObject::Shape(shape) => assert_eq!(shape.transform.opacity, 37.0),
            PrGraphicObject::Text(text) => {
                assert_eq!(text.transform.opacity, 37.0);
                assert_eq!(text.document.text, "Edited mask");
            }
            other => panic!("{other:?}"),
        }
    }
}

#[test]
fn adobe_subgroup_bounds_the_mask_and_retains_the_outside_object() {
    let (native, root) = native_mask_graphic("b", "VideoClipTrackItem:65");
    let [PrGraphicObject::Group(group), PrGraphicObject::Shape(outside)] =
        native.objects.as_slice()
    else {
        panic!("{:?}", native.objects);
    };
    assert!(group.objects[0].mask_source().is_some());
    let inner = &root["layers"][0];
    assert_eq!(
        inner["layers"][1]["trackMatte"]["layer"],
        inner["layers"][0]["id"]
    );
    assert_eq!(root["layers"][1]["name"], outside.name);
    let (written, omissions) = written_back(PrGraphic {
        objects: native.objects,
        ..shape_graphic()
    });
    assert!(
        matches!(
            written.objects.as_slice(),
            [PrGraphicObject::Group(_), PrGraphicObject::Shape(_)]
        ),
        "{omissions:?}"
    );
}

#[test]
fn failed_graphic_mask_export_suppresses_only_its_composite_and_prunes_empty_subgroups() {
    let square = |name, mask| square_object(name, [960.0, 540.0], 200.0, [0, 96, 255], mask);
    for upper in [false, true] {
        let mut members = vec![square("M", INVERTED), square("T", None)];
        if upper {
            members.insert(0, square("A", None));
        }
        let mut root =
            imported_graphic_group(vec![subgroup("G", members), square("Outside", None)]);
        let mask = &mut root["layers"][0]["layers"][usize::from(upper)];
        mask["effects"] =
            json!([{"id": 90000, "effect": {"type": "stroke", "color": [0,0,1,1], "width": 4}}]);
        let (graphics, omissions) = exported_and_read(vec![root, sibling_text()]);
        assert_eq!(
            exported_names(&graphics),
            if upper {
                vec!["G{A} Outside"]
            } else {
                vec!["Outside"]
            },
            "{omissions:?}"
        );
        assert!(
            omissions.iter().any(|report| report
                .reason
                .contains("the objects that it masks are not exported either")),
            "{omissions:?}"
        );
    }
}

#[test]
fn graphic_subgroup_classification_preserves_keyed_moved_and_text_failure_nest_routes() {
    let square = |name| square_object(name, [960.0, 540.0], 200.0, [0, 96, 255], None);
    let base = imported_graphic_group(vec![subgroup("G", vec![square("A")]), square("Outside")]);
    let subgroup_id = base["layers"][0]["id"].as_u64().unwrap();
    let classify = |root: Value, entries: Vec<Value>| {
        let mut wire = editable_document();
        wire["composition"]["layers"] = json!([root]);
        wire["composition"]["dynamics"] = json!({"entries": entries});
        let document = EditableFxCompositionDocument::from_json_value(wire).unwrap();
        let LayerData::Group(group) = document.composition().layers()[0].data() else {
            panic!()
        };
        super::graphic_objects(group, document.composition().dynamics()).is_some()
    };
    assert!(classify(base.clone(), Vec::new()));
    let mut wrapper = base.clone();
    wrapper["layers"].as_array_mut().unwrap().pop();
    assert!(classify(wrapper.clone(), Vec::new()));
    wrapper["transform"]["position"] = json!([20, 0]);
    assert!(!classify(wrapper.clone(), Vec::new()));
    wrapper["name"] = json!("Unrelated wrapper name");
    wrapper["layers"][0]["name"] = json!("Unrelated child name");
    assert!(!classify(wrapper, Vec::new()));
    let mask = square_object("M", [960.0, 540.0], 200.0, [0, 96, 255], NOT_INVERTED);
    let mut masked = imported_graphic_group(vec![subgroup("G", vec![mask, square("T")])]);
    masked["transform"]["position"] = json!([20, 0]);
    assert!(classify(masked, Vec::new()));
    assert!(!classify(
        base.clone(),
        vec![entry_on(
            subgroup_id,
            "positionX",
            &[(0, 0.0, "linear"), (500, 20.0, "linear")]
        )]
    ));
    let mut moved = base.clone();
    moved["layers"][0]["transform"]["position"] = json!([20, 0]);
    assert!(!classify(moved, Vec::new()));
    let mut text = text_graphic().objects.remove(0);
    if let PrGraphicObject::Text(text) = &mut text {
        text.document.stroke = None;
    }
    let text_group = imported_graphic_group(vec![subgroup("G", vec![text]), square("Outside")]);
    let mut width_keyed = text_group.clone();
    let text = &mut width_keyed["layers"][0]["layers"][0];
    text["sourceText"]["strokeColor"] = json!([0.0, 0.0, 0.0, 1.0]);
    text["sourceText"]["applyStroke"] = json!(true);
    text["animators"] = json!([{"id": 90002, "strokeWidth": 0.0}]);
    let mut width = entry("strokeWidth", &[(0, 0.0, "hold"), (500, 2.0, "hold")]);
    width["target"] =
        json!({"kind": "fxItemProperty", "itemId": 90002, "propertyName": "strokeWidth"});
    assert!(!classify(width_keyed, vec![width]));
    let mut unconvertible = text_group.clone();
    unconvertible["layers"][0]["layers"][0]["transform"]["scale"] = json!([100, 90]);
    assert!(classify(unconvertible.clone(), Vec::new()));
    unconvertible["layers"][0]["layers"][0]["sourceText"]["underline"] = json!(true);
    assert!(!classify(unconvertible, Vec::new()));
    // Missing packaging alone is handled by export, not by the structural classifier.
    let mut fontless = text_group;
    fontless["layers"][0]["layers"][0]["sourceText"]["fontFamily"] = json!("Unpackaged");
    fontless["layers"][0]["layers"][0]["sourceText"]["fontStyle"] = json!("Regular");
    assert!(classify(fontless.clone(), Vec::new()));
    let (graphics, omissions) = exported_and_read(vec![fontless, sibling_text()]);
    assert_eq!(exported_names(&graphics), ["Outside"], "{omissions:?}");
}

#[test]
fn a_missing_font_mask_takes_its_composite_but_keeps_outside_siblings() {
    let mut text = text_graphic().objects.remove(0);
    if let PrGraphicObject::Text(text) = &mut text {
        text.document.stroke = None;
        text.mask_source = INVERTED;
    }
    let square = |name| square_object(name, [960.0, 540.0], 200.0, [0, 96, 255], None);
    let mut root = imported_graphic_group(vec![
        subgroup("G", vec![text, square("T")]),
        square("Outside"),
    ]);
    let mask = &mut root["layers"][0]["layers"][0];
    mask["sourceText"]["fontFamily"] = json!("Unpackaged");
    mask["sourceText"]["fontStyle"] = json!("Regular");
    let (graphics, omissions) = exported_and_read(vec![root, sibling_text()]);
    assert_eq!(exported_names(&graphics), ["Outside"], "{omissions:?}");
    assert!(
        omissions.iter().any(|report| report
            .reason
            .contains("the objects that it masks are not exported either")),
        "{omissions:?}"
    );
}

#[test]
fn graphic_mask_field_losses_keep_typed_reports_and_do_not_reveal_the_composite() {
    use crate::export_loss::{ExportField, ExportLossKind, ExportLossSource, LossCollector};
    let square = |name, mask| square_object(name, [960.0, 540.0], 200.0, [0, 96, 255], mask);
    for (property, value, field) in [
        ("skew", 20.0, ExportField::Skew),
        ("rotationX", 30.0, ExportField::Rotation3d),
        ("motionBlur", 180.0, ExportField::MotionBlur),
    ] {
        let mut source = text_graphic().objects.remove(0);
        if let PrGraphicObject::Text(text) = &mut source {
            text.name = "M".into();
            text.document.stroke = None;
            text.mask_source = INVERTED;
        }
        let mut root = imported_graphic_group(vec![square("A", None), source, square("T", None)]);
        let id = root["layers"][1]["id"].as_u64().unwrap();
        if property == "motionBlur" {
            root["layers"][1]["motionBlur"] = json!(true);
        } else {
            root["layers"][1]["transform"][property] = json!(value);
        }
        let mut wire = editable_document();
        let mut canvas = wire["composition"]["layers"][1].clone();
        canvas["id"] = json!(90001);
        wire["composition"]["layers"] = json!([root, canvas]);
        if property == "motionBlur" {
            wire["composition"]["motionBlur"] = json!({"enabled": true, "shutterAngle": value});
        }
        let document = EditableFxCompositionDocument::from_json_value(wire).unwrap();
        let mut reports = LossCollector::default();
        crate::convert::lower_document(
            &document,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            FrameRate::Fps30,
            &mut reports,
        )
        .unwrap();
        let report = reports.finish(true);
        assert!(
            report.losses.iter().any(|loss| loss.source
                == ExportLossSource::Layer(LayerId::new(id))
                && loss.kind == ExportLossKind::Field(field)),
            "{report:?}"
        );
        assert!(
            report.losses.iter().any(|loss| loss
                .omission
                .reason
                .contains("the objects that it masks are not exported either")),
            "{report:?}"
        );
    }
}

#[test]
fn a_graphic_mask_losing_import_keys_leaves_no_composite_or_animation_targets() {
    let mut graphic = keyed_graphic();
    let start = graphic.in_ticks;
    let source = graphic.text_mut();
    source.document.stroke = None;
    source.mask_source = INVERTED;
    source.animations[1] = PrPropertyAnimation::UniformScale(vec![
        scalar(start, 80.0, PrKeyframeEasing::Linear),
        scalar(start + TICKS / 5000, 96.0, PrKeyframeEasing::Linear),
    ]);
    graphic.objects.push(square_object(
        "Target",
        [960.0, 540.0],
        200.0,
        [0, 96, 255],
        None,
    ));
    let (document, omissions) = imported(graphic);
    assert!(
        omissions
            .iter()
            .any(|report| report.reason.contains("none of its objects converts")),
        "{omissions:?}"
    );
    assert!(document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .all(|layer| layer["type"] != "Group" && layer["type"] != "Text"));
    assert!(document["composition"]["dynamics"]["entries"]
        .as_array()
        .is_none_or(Vec::is_empty));
}

#[test]
fn a_graphic_object_used_as_a_mask_across_subgroup_scope_stays_on_the_nest_route() {
    let square = |name| square_object(name, [960.0, 540.0], 200.0, [0, 96, 255], None);
    let mut root =
        imported_graphic_group(vec![subgroup("G", vec![square("Target")]), square("Guide")]);
    let guide = root["layers"][1]["id"].clone();
    root["layers"][0]["masks"] = json!([{"id": 90000, "mode": "add", "layer": guide}]);
    let mut wire = editable_document();
    wire["composition"]["layers"] = json!([root]);
    let document = EditableFxCompositionDocument::from_json_value(wire).unwrap();
    let LayerData::Group(group) = document.composition().layers()[0].data() else {
        panic!()
    };
    assert!(super::graphic_objects(group, document.composition().dynamics()).is_none());
}

#[test]
fn numeric_mask_graphic_generator_clock_exports_edited_owner_keys() {
    let mut graphic = shape_graphic();
    let mut mask = crate::tests::support::opacity_mask();
    mask.expansion_keys = vec![
        scalar(graphic.in_ticks - TICKS / 4, -8.0, PrKeyframeEasing::Linear),
        scalar(graphic.in_ticks + TICKS, 12.0, PrKeyframeEasing::Linear),
    ];
    mask.feather_keys = vec![
        scalar(graphic.in_ticks, 4.0, PrKeyframeEasing::Linear),
        scalar(
            graphic.in_ticks + TICKS,
            20.0,
            PrKeyframeEasing::CubicBezier {
                x1: 0.3,
                y1: 0.0,
                x2: 0.7,
                y2: 1.0,
            },
        ),
    ];
    mask.opacity_keys = vec![
        scalar(graphic.in_ticks, 100.0, PrKeyframeEasing::Linear),
        scalar(graphic.in_ticks + TICKS, 60.0, PrKeyframeEasing::Hold),
    ];
    graphic.opacity_mask = Some(mask);
    let (mut wire, omissions) = imported(graphic);
    wire["duration"] = json!(3.0);
    assert!(
        omissions
            .iter()
            .any(|o| o.reason.contains("negative expansion remains editable")),
        "{omissions:?}"
    );
    let layers = wire["composition"]["layers"].as_array_mut().unwrap();
    layers.retain(|layer| layer["type"] != "Video");
    let owner = layers
        .iter()
        .find(|layer| layer["masks"].as_array().is_some_and(|m| !m.is_empty()))
        .unwrap();
    let id = owner["masks"][0]["id"].clone();
    let entry = wire["composition"]["dynamics"]["entries"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|entry| {
            entry["target"]["itemId"] == id && entry["target"]["propertyName"] == "expansion"
        })
        .unwrap();
    assert_eq!(entry["animator"]["keyframes"][0]["layerTime"], -250);
    entry["animator"]["keyframes"][1]["layerTime"] = json!(1500);
    entry["animator"]["keyframes"][1]["value"]["value"] = json!(-24.0);
    let document = EditableFxCompositionDocument::from_json_value(wire).unwrap();
    let mut omissions = Vec::new();
    let exported = crate::convert::export_document(
        &document,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        FrameRate::Fps30,
        &mut omissions,
    )
    .unwrap();
    assert!(
        omissions
            .iter()
            .all(|o| o.kind == OmissionKind::Approximated),
        "{omissions:?}"
    );
    for entry in document.composition().dynamics().entries() {
        assert!(exported.written.contains(&entry.target));
    }
    let graphic = exported.project.sequences[0]
        .video_items()
        .find_map(PrVideoItem::graphic)
        .unwrap();
    let mask = graphic.opacity_mask.as_ref().unwrap();
    assert_eq!(mask.expansion_keys[0].source_ticks, EXPORT_IN - TICKS / 4);
    assert_eq!(
        mask.expansion_keys[1].source_ticks,
        EXPORT_IN + 3 * TICKS / 2
    );
    assert_eq!(mask.expansion_keys[1].value, -24.0);
    assert_eq!(mask.opacity_keys[1].value, 60.0);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("numeric-mask.prproj");
    PremiereProjectXml::new(&exported.project)
        .unwrap()
        .write_new(&path)
        .unwrap();
    let (read, _) = PrProjectFile::load(&path).unwrap();
    let written = read.sequences[0]
        .video_items()
        .find_map(PrVideoItem::graphic)
        .unwrap()
        .opacity_mask
        .as_ref()
        .unwrap();
    assert_eq!(written.numeric_keys(), mask.numeric_keys());
}

/// A 200 × 200 px square mask in unit fractions of the sequence frame.
fn frame_mask(left: f32, top: f32) -> crate::schema::PrMask {
    use crate::schema::text::{PrPathVertex, PrShapePath};
    let [width, height] = [200.0 / 1920.0, 200.0 / 1080.0];
    let corner = |x: f32, y: f32| PrPathVertex {
        smooth: false,
        point: [x, y],
        in_tangent: [x, y],
        out_tangent: [x, y],
    };
    crate::schema::PrMask {
        raster: None,
        feather_keys: Vec::new(),
        expansion: 0.0,
        expansion_keys: Vec::new(),
        opacity_keys: Vec::new(),
        path: PrShapePath {
            vertices: vec![
                corner(left, top),
                corner(left + width, top),
                corner(left + width, top + height),
                corner(left, top + height),
            ],
            closed: true,
        },
        path_keys: Vec::new(),
        feather: 0.0,
        opacity: 100.0,
        inverted: true,
    }
}

#[test]
fn attached_and_clip_opacity_masks_import_as_path_masks_beside_their_owners() {
    use crate::schema::text::PrVectorMotion;
    let blue = [0, 96, 255];
    let mut owner = square_object("Owner", [700.0, 500.0], 300.0, blue, None);
    if let PrGraphicObject::Shape(shape) = &mut owner {
        // The attached mask stays in the graphic frame after the Shape's own
        // transform (c1): an asymmetric anchor, scale and rotation.
        shape.transform.anchor = [40.0, -20.0];
        shape.transform.scale = 80.0;
        shape.transform.rotation = 30.0;
        shape.mask = Some(frame_mask(0.3, 0.4));
    }
    let graphic = PrGraphic {
        // The clip mask acts after the Vector Motion (d1).
        vector_motion: Some(PrVectorMotion {
            position: [864.0, 540.0],
            anchor: [960.0, 540.0],
            scale: 100.0,
            rotation: 15.0,
            animations: Vec::new(),
        }),
        opacity_mask: Some(frame_mask(0.45, 0.35)),
        objects: vec![
            owner,
            square_object("Other", [1200.0, 540.0], 200.0, blue, None),
        ],
        ..shape_graphic()
    };
    let (document, _) = imported(graphic.clone());
    let layers = document["composition"]["layers"].as_array().unwrap();
    let group = &layers[0];
    let guide = &layers[1];
    // The clip mask's guide is the group's sibling, at the identity over the
    // group's range, without paint, and consumed by the group's one mask.
    assert_eq!(group["type"], "Group");
    assert_eq!(group["masks"][0]["layer"], guide["id"]);
    assert_eq!(group["masks"][0]["inverted"], true);
    assert_eq!(guide.get("parent"), None);
    assert_eq!(guide["activeRange"], group["playback"]["inputRange"]);
    assert_eq!(guide["transform"]["position"], json!([0.0, 0.0]));
    assert_eq!(guide["shape"].get("fills"), None);
    let shape = &group["layers"][0];
    let shape_guide = &group["layers"][1];
    assert_eq!(shape["masks"][0]["layer"], shape_guide["id"]);
    assert_eq!(shape_guide["parent"], group["id"]);
    assert_eq!(shape_guide["transform"]["rotation"], json!(0.0));
    assert_eq!(shape_guide["activeRange"], shape["activeRange"]);
    // Every layer and mask id is its own.
    let mut ids = BTreeSet::new();
    fn collect(layers: &Value, ids: &mut BTreeSet<u64>) {
        for layer in layers.as_array().unwrap() {
            assert!(ids.insert(layer["id"].as_u64().unwrap()), "{layer}");
            for mask in layer["masks"].as_array().into_iter().flatten() {
                assert!(ids.insert(mask["id"].as_u64().unwrap()), "{mask}");
            }
            if let Some(children) = layer.get("layers") {
                collect(children, ids);
            }
        }
    }
    collect(&document["composition"]["layers"], &mut ids);
    // And both masks export and read back in their own hosts.
    let (written, omissions) = written_back(graphic.clone());
    assert_eq!(written.opacity_mask, graphic.opacity_mask, "{omissions:?}");
    assert_eq!(written.objects, graphic.objects, "{omissions:?}");
    assert_eq!(
        written.vector_motion, graphic.vector_motion,
        "{omissions:?}"
    );
}

/// The closed square contour from `min` to `max` in FX commands, clockwise
/// on screen, or the other way with `reversed`.
fn square_contour(min: f64, max: f64, reversed: bool) -> Vec<Value> {
    let mut points = [[min, min], [max, min], [max, max], [min, max]];
    if reversed {
        points.reverse();
    }
    let mut commands = vec![json!({"type": "moveTo", "x": points[0][0], "y": points[0][1]})];
    commands.extend(
        points[1..]
            .iter()
            .map(|[x, y]| json!({"type": "lineTo", "x": x, "y": y})),
    );
    commands.push(json!({"type": "close"}));
    commands
}

/// A piece of an exported shape: its kind, first vertex and opacity.
type Piece = (&'static str, [f32; 2], f64);

/// The pieces of the graphic that exporting shape layer `layer` next to
/// [`sibling_text`] makes, and every omission; `None` when the shape does
/// not export as pieces.
fn exported_pieces(layer: Value) -> (Option<Vec<Piece>>, Vec<Omission>) {
    let (project, omissions) = export_over_canvas(vec![layer, sibling_text()], Vec::new());
    let project = project.unwrap_or_else(|error| panic!("{error}: {omissions:?}"));
    let pieces = project
        .single_sequence()
        .unwrap()
        .video_items()
        .filter_map(PrVideoItem::graphic)
        .find_map(|graphic| match graphic.objects.as_slice() {
            [PrGraphicObject::Group(group)] => Some(group.objects.clone()),
            _ => None,
        })
        .map(|objects| {
            objects
                .iter()
                .map(|object| {
                    let PrGraphicObject::Shape(shape) = object else {
                        panic!("a piece is a Shape: {object:?}");
                    };
                    let appearance = &shape.appearance;
                    let kind = match (appearance.mask_source, &appearance.fill, &appearance.stroke)
                    {
                        (Some(mask), Some(_), None) if mask.inverted && shape.path.closed => "hole",
                        (None, Some(_), _) => "fill",
                        (None, None, Some(_)) => "stroke",
                        _ => panic!("an unexpected piece: {shape:?}"),
                    };
                    (kind, shape.path.vertices[0].point, shape.transform.opacity)
                })
                .collect()
        });
    (pieces, omissions)
}

#[test]
fn compound_shape_paths_export_as_certified_pieces() {
    let shape = imported_object(shape_graphic());
    let with = |contours: Vec<Vec<Value>>, rule: &str, stroke: Option<f64>| {
        let mut layer = shape.clone();
        layer["shape"]["path"]["commands"] = json!(contours.concat());
        layer["shape"]["fills"][0]["fillRule"] = json!(rule);
        layer["transform"]["opacity"] = json!(60);
        if let Some(width) = stroke {
            layer["shape"]["strokes"] =
                json!([{"paint": {"type": "solid", "color": [0, 1, 0.25, 1]}, "width": width}]);
        }
        layer
    };
    let (outer, inner, island) = (
        square_contour(0.0, 100.0, false),
        square_contour(30.0, 70.0, false),
        square_contour(45.0, 55.0, false),
    );
    // Holes are opaque, and visible pieces keep the layer's Opacity 60.
    for (case, layer, expected) in [
        (
            "even-odd donut",
            with(vec![outer.clone(), inner.clone()], "evenOdd", None),
            vec![("hole", [30.0, 30.0], 100.0), ("fill", [0.0, 0.0], 60.0)],
        ),
        (
            "nonzero donut of opposite windings",
            with(
                vec![outer.clone(), square_contour(30.0, 70.0, true)],
                "nonZeroWinding",
                None,
            ),
            vec![("hole", [30.0, 70.0], 100.0), ("fill", [0.0, 0.0], 60.0)],
        ),
        (
            "nonzero contours of one winding fill both",
            with(vec![outer.clone(), inner.clone()], "nonZeroWinding", None),
            vec![("fill", [0.0, 0.0], 60.0)],
        ),
        (
            "an island in an even-odd hole",
            with(
                vec![outer.clone(), inner.clone(), island.clone()],
                "evenOdd",
                None,
            ),
            vec![
                ("fill", [45.0, 45.0], 60.0),
                ("hole", [30.0, 30.0], 100.0),
                ("fill", [0.0, 0.0], 60.0),
            ],
        ),
        (
            "disjoint contours",
            with(
                vec![
                    square_contour(0.0, 40.0, false),
                    square_contour(60.0, 100.0, true),
                ],
                "nonZeroWinding",
                None,
            ),
            vec![("fill", [0.0, 0.0], 60.0), ("fill", [60.0, 100.0], 60.0)],
        ),
        (
            "a stroked donut: the hole's stroke above its mask",
            with(vec![outer.clone(), inner.clone()], "evenOdd", Some(4.0)),
            vec![
                ("stroke", [30.0, 30.0], 60.0),
                ("hole", [30.0, 30.0], 100.0),
                ("fill", [0.0, 0.0], 60.0),
            ],
        ),
    ] {
        let (pieces, omissions) = exported_pieces(layer);
        let reported = |reason: &str| omissions.iter().any(|omission| omission.reason == reason);
        // Only the stroked translucent donut blends a stroke over a fill, and
        // only shapes with a hole antialias its edge as a mask.
        let stroked = expected.iter().any(|(kind, _, _)| *kind == "stroke");
        let holed = expected.iter().any(|(kind, _, _)| *kind == "hole");
        assert_eq!(
            reported(super::COMPOUND_OPACITY_APPROXIMATION),
            stroked,
            "{case}: {omissions:?}"
        );
        assert_eq!(
            reported(super::COMPOUND_HOLE_EDGE_APPROXIMATION),
            holed,
            "{case}: {omissions:?}"
        );
        assert_eq!(pieces, Some(expected), "{case}: {omissions:?}");
    }
    // Contours within twice the reach of a 30 px stroke (15 px each, times
    // FX's miter limit of 4) are not certified apart.
    let (pieces, omissions) = exported_pieces(with(vec![outer, inner], "evenOdd", Some(30.0)));
    assert_eq!(pieces, None);
    assert!(
        omissions.iter().any(|omission| omission.reason
            == "shape layer was not exported: unsupported conversion: contours 1 and 2 of the shape path are not certified 120 px apart"),
        "{omissions:?}"
    );
}

#[test]
fn compound_shape_contours_within_a_pixel_at_the_graphics_scale_export_and_are_reported() {
    let shape = imported_object(shape_graphic());
    let donut = |gap: f64, scale: [f64; 2]| {
        let mut layer = shape.clone();
        layer["shape"]["path"]["commands"] = json!([
            square_contour(0.0, 100.0, false),
            square_contour(gap, 100.0 - gap, false)
        ]
        .concat());
        layer["shape"]["fills"][0]["fillRule"] = json!("evenOdd");
        layer["transform"]["scale"] = json!(scale);
        layer
    };
    let near = super::compound_near_approximation(1, 2);
    let reasons = |omissions: &[Omission]| -> Vec<String> {
        omissions
            .iter()
            .filter(|omission| {
                omission.record == "layer 9 (\"Box\")" && omission.scope == OmissionScope::Feature
            })
            .map(|omission| omission.reason.clone())
            .collect()
    };
    let hole_edge = super::COMPOUND_HOLE_EDGE_APPROXIMATION.to_owned();
    // At the layer's own scale: a 1 px band was refused before; it converts
    // and is reported, as is the band that a nonuniform scale narrows.
    for (case, layer, expected) in [
        (
            "a 3 px band",
            donut(3.0, [100.0, 100.0]),
            vec![hole_edge.clone()],
        ),
        (
            "a 1 px band",
            donut(1.0, [100.0, 100.0]),
            vec![hole_edge.clone(), near.clone()],
        ),
        (
            "a 3 px band scaled 100 x 25",
            donut(3.0, [100.0, 25.0]),
            vec![hole_edge.clone(), near.clone()],
        ),
    ] {
        let (pieces, omissions) = exported_pieces(layer);
        assert!(pieces.is_some(), "{case}: {omissions:?}");
        assert_eq!(reasons(&omissions), expected, "{case}: {omissions:?}");
    }
    // A shape that exports no pieces reports none of their approximations:
    // Premiere's Horizontal Scale does not reach a reflection.
    let (pieces, omissions) = exported_pieces(donut(1.0, [-100.0, 100.0]));
    assert_eq!(pieces, None);
    assert_eq!(reasons(&omissions), Vec::<String>::new(), "{omissions:?}");
    // Under a Vector Motion that shrinks it, static or by a Scale key, the
    // same 3 px band may share a device pixel.
    let mut child = donut(3.0, [100.0, 100.0]);
    child["parent"] = json!(8);
    child["activeRange"] = json!({"start": 0, "duration": 1000});
    for (case, scale, entries) in [
        ("Vector Motion 100", 100.0, Vec::new()),
        ("Vector Motion 25", 25.0, Vec::new()),
        (
            "Vector Motion keyed to 20",
            100.0,
            vec![
                entry_on(8, "scaleX", &[(0, 100.0, "linear"), (1000, 20.0, "linear")]),
                entry_on(8, "scaleY", &[(0, 100.0, "linear"), (1000, 20.0, "linear")]),
            ],
        ),
    ] {
        let group = graphic_group(json!({
            "transform": {"anchorPoint": [0, 0], "position": [960, 540], "scale": [scale, scale], "rotation": 0, "opacity": 100},
            "layers": [child.clone()],
        }));
        let (project, omissions) = export_over_canvas(vec![group], entries);
        assert!(project.is_ok(), "{case}: {omissions:?}");
        let expected = if scale == 100.0 && case == "Vector Motion 100" {
            vec![hole_edge.clone()]
        } else {
            vec![hole_edge.clone(), near.clone()]
        };
        assert_eq!(reasons(&omissions), expected, "{case}: {omissions:?}");
    }
}

#[test]
fn compound_shape_shadows_export_visible_pieces_but_not_hole_masks() {
    // Fresh native A import, then explicit editable compound-path/shadow edits.
    // This is fixture-backed structure proof, not a compound-shadow Adobe oracle.
    let (_, root) = native_mask_graphic("a", "VideoClipTrackItem:67");
    let mut layer = root["layers"][0].clone();
    assert_eq!(layer["type"], "Shape");
    layer["activeRange"] = json!({"start": 0, "duration": 1000});
    layer["transform"]["scale"] = json!([100, 100]);
    layer["transform"]["rotation"] = json!(0);
    layer["shape"]["path"]["commands"] = json!([
        square_contour(0.0, 100.0, false),
        square_contour(30.0, 70.0, false),
        square_contour(45.0, 55.0, false),
    ]
    .concat());
    layer["shape"]["fills"][0]["fillRule"] = json!("evenOdd");
    layer["shape"]["strokes"] = json!([
        {"paint": {"type": "solid", "color": [0, 1, 0, 1]}, "width": 2}
    ]);
    let mut topology = None;
    for offset in [10.0, 20.0] {
        layer["effects"] = json!([
            {"id": 901, "effect": {"type": "dropShadow", "offset": [offset, offset],
                "color": [0.2, 0.4, 0.6, 1], "blurRadius": 0, "spreadRadius": 0}},
            {"id": 902, "effect": {"type": "invert"}}
        ]);
        let (graphics, omissions) = exported_and_read(vec![layer.clone(), sibling_text()]);
        let group = graphics
            .iter()
            .flat_map(|graphic| &graphic.objects)
            .find_map(|object| match object {
                PrGraphicObject::Group(group) => Some(group),
                _ => None,
            })
            .expect("compound pieces survive serialization");
        let mut visible = 0;
        let mut holes = 0;
        for object in &group.objects {
            let PrGraphicObject::Shape(piece) = object else {
                panic!("compound piece is not a Shape");
            };
            if piece.appearance.mask_source.is_some() {
                holes += 1;
                assert_eq!(piece.appearance.mask_source, INVERTED);
                assert_eq!(piece.transform.opacity, 100.0);
                assert!(piece.appearance.shadow.is_none());
            } else {
                visible += 1;
                let shadow = piece.appearance.shadow.expect("visible piece shadow");
                assert_eq!(shadow.color, PrRgb([51, 102, 153]));
                assert_eq!(shadow.opacity, 100.0);
                assert_eq!(shadow.angle, crate::schema::text::SHAPE_SHADOW_ANGLE);
                assert!((f64::from(shadow.distance) - offset * 2_f64.sqrt()).abs() < 0.001);
                assert_eq!((shadow.blur, shadow.size), (0.0, 0.0));
            }
        }
        assert_eq!((visible, holes), (3, 1));
        assert_eq!(
            omissions
                .iter()
                .filter(|report| {
                    report.kind == OmissionKind::Approximated
                        && report.reason.contains("compound shape shadow")
                })
                .count(),
            1,
            "{omissions:?}"
        );
        assert_eq!(
            omissions
                .iter()
                .filter(|report| {
                    report.reason.contains("902") && report.reason.contains("not exported")
                })
                .count(),
            1,
            "{omissions:?}"
        );
        let mut unshadowed = group.clone();
        for object in &mut unshadowed.objects {
            if let PrGraphicObject::Shape(piece) = object {
                piece.appearance.shadow = None;
            }
        }
        if let Some(previous) = &topology {
            assert_eq!(&unshadowed, previous);
        }
        topology = Some(unshadowed);
    }
}

#[test]
fn compound_shape_pieces_read_back_and_export_again_as_the_same_pieces() {
    let mut layer = imported_object(shape_graphic());
    layer["shape"]["path"]["commands"] = json!([
        square_contour(0.0, 100.0, false),
        square_contour(30.0, 70.0, false),
        square_contour(45.0, 55.0, false),
    ]
    .concat());
    layer["shape"]["fills"][0]["fillRule"] = json!("evenOdd");
    let (project, omissions) = export_over_canvas(vec![layer, sibling_text()], Vec::new());
    let project = project.unwrap_or_else(|error| panic!("{error}: {omissions:?}"));
    let graphic = |project: &PrProjectFile| {
        project
            .single_sequence()
            .unwrap()
            .video_items()
            .filter_map(PrVideoItem::graphic)
            .find(|graphic| matches!(graphic.objects.as_slice(), [PrGraphicObject::Group(_)]))
            .cloned()
            .expect("the pieces export")
    };
    let first = graphic(&project);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("pieces.prproj");
    PremiereProjectXml::new(&project)
        .unwrap()
        .write_new(&path)
        .unwrap();
    let (reopened, _) = PrProjectFile::load(&path).unwrap();
    let read = graphic(&reopened);
    assert_eq!(read.objects, first.objects);
    // Import makes the SubGroup an FX group whose hole mattes the outer
    // piece, which exports as the same pieces.
    let (document, omissions) = imported(read);
    let group = &document["composition"]["layers"][0]["layers"][0];
    assert_eq!(group["type"], "Group", "{omissions:?}");
    let members: Vec<_> = group["layers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|layer| layer["type"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(members, ["Shape", "Shape", "Group"]);
    assert_eq!(group["layers"][2]["trackMatte"]["mode"], "alphaInverted");
    let (again, omissions) = written_back(PrGraphic {
        objects: first.objects.clone(),
        ..shape_graphic()
    });
    assert_eq!(again.objects, first.objects, "{omissions:?}");
    let mut edited = imported_graphic_group(first.objects);
    let hole = &mut edited["layers"][0]["layers"][1];
    hole["shape"]["path"]["commands"][0]["x"] = json!(28.0);
    let (graphics, omissions) = exported_and_read(vec![edited, sibling_text()]);
    let hole = graphics
        .iter()
        .flat_map(|graphic| &graphic.objects)
        .find_map(|object| {
            let PrGraphicObject::Group(group) = object else {
                return None;
            };
            group.objects.iter().find_map(|piece| match piece {
                PrGraphicObject::Shape(shape) if shape.appearance.mask_source == INVERTED => {
                    Some(shape)
                }
                _ => None,
            })
        })
        .unwrap_or_else(|| panic!("{omissions:?}"));
    assert_eq!(hole.path.vertices[0].point, [28.0, 30.0]);
}

#[test]
fn a_masked_shape_alone_keeps_its_vector_motion_apart_from_its_mask() {
    use crate::schema::text::PrVectorMotion;
    // One Shape would fold a static Vector Motion into its own transform,
    // which would move its attached mask with it.
    let mut owner = square_object("Owner", [700.0, 500.0], 300.0, [0, 96, 255], None);
    if let PrGraphicObject::Shape(shape) = &mut owner {
        shape.mask = Some(frame_mask(0.3, 0.4));
    }
    let graphic = PrGraphic {
        vector_motion: Some(PrVectorMotion {
            position: [864.0, 540.0],
            anchor: [960.0, 540.0],
            scale: 100.0,
            rotation: 15.0,
            animations: Vec::new(),
        }),
        objects: vec![owner],
        ..shape_graphic()
    };
    let (written, omissions) = written_back(graphic.clone());
    assert_eq!(
        written.vector_motion, graphic.vector_motion,
        "{omissions:?}"
    );
    assert_eq!(written.objects, graphic.objects, "{omissions:?}");
}

#[test]
fn a_shape_of_several_contours_in_a_subgroup_exports_without_its_unverified_hole() {
    let shape = imported_object(shape_graphic());
    let member = |id: u64, name: &str, parent: u64, contours: Vec<Vec<Value>>| {
        let mut layer = shape.clone();
        layer["id"] = json!(id);
        layer["name"] = json!(name);
        layer["parent"] = json!(parent);
        layer["activeRange"] = json!({"start": 0, "duration": 1000});
        // Only a path of several contours reads the even-odd rule.
        if contours.len() > 1 {
            layer["shape"]["fills"][0]["fillRule"] = json!("evenOdd");
        }
        layer["shape"]["path"]["commands"] = json!(contours.concat());
        layer
    };
    let donut = || {
        vec![
            square_contour(0.0, 100.0, false),
            square_contour(30.0, 70.0, false),
        ]
    };
    let outside = || member(11, "Outside", 8, vec![square_contour(400.0, 500.0, false)]);
    // Its pieces would be a SubGroup inside the SubGroup, whose hole mask no
    // render covers and the reader omits: the donut stays out alone.
    let inner = identity_subgroup(
        12,
        "Inner",
        vec![
            member(13, "Donut", 12, donut()),
            member(14, "Square", 12, vec![square_contour(200.0, 300.0, false)]),
        ],
    );
    let group = graphic_group(json!({"layers": [inner, outside()]}));
    let (graphics, omissions) = exported_and_read(vec![group, sibling_text()]);
    assert_eq!(
        exported_names(&graphics),
        ["Inner{Square} Outside"],
        "{omissions:?}"
    );
    assert!(
        omissions.iter().any(|omission| omission.scope == OmissionScope::Feature
            && omission.reason
                == format!(
                    "layer 13 (\"Donut\") was not exported: its pieces cannot export: {}",
                    "a Mask with Shape or Text inside a nested SubGroup is unverified against Premiere"
                )),
        "{omissions:?}"
    );
    // At the graphic's own level its pieces are one SubGroup deep, a
    // rendered form, and export.
    let group = graphic_group(json!({"layers": [member(13, "Donut", 8, donut()), outside()]}));
    let (graphics, omissions) = exported_and_read(vec![group, sibling_text()]);
    assert_eq!(
        exported_names(&graphics),
        ["Donut{Donut Donut} Outside"],
        "{omissions:?}"
    );
}

#[test]
fn compound_shape_contours_count_the_scale_of_the_nests_and_keys_that_draw_them() {
    let mut donut = imported_object(shape_graphic());
    donut["shape"]["path"]["commands"] = json!([
        square_contour(0.0, 100.0, false),
        square_contour(3.0, 97.0, false)
    ]
    .concat());
    donut["shape"]["fills"][0]["fillRule"] = json!("evenOdd");
    let hole_edge = super::COMPOUND_HOLE_EDGE_APPROXIMATION.to_owned();
    let near = super::compound_near_approximation(1, 2);
    let reasons = |omissions: &[Omission]| -> Vec<String> {
        omissions
            .iter()
            .filter(|omission| {
                omission.record == "layer 9 (\"Box\")" && omission.scope == OmissionScope::Feature
            })
            .map(|omission| omission.reason.clone())
            .collect()
    };
    // The donut in a nest beside a solid: the nest's Motion scales it.
    let mut child = donut.clone();
    child["parent"] = json!(7);
    let mut solid = editable_document()["composition"]["layers"][1].clone();
    solid["id"] = json!(10);
    solid["parent"] = json!(7);
    solid["activeRange"] = json!({"start": 0, "duration": 1000});
    let nest = |scale: f64| {
        json!({
            "type": "Group",
            "id": 7,
            "name": "Outer",
            "playback": crate::test_support::linear_playback(json!({"start": 1000, "duration": 1000}), json!({"start": 0, "duration": 1000})),
            "transform": {"anchorPoint": [960, 540], "position": [960, 540], "scale": [scale, scale], "rotation": 0, "opacity": 100},
            "layers": [child.clone(), solid.clone()],
        })
    };
    let nest_keys = vec![
        entry_on(7, "scaleX", &[(0, 100.0, "linear"), (1000, 20.0, "linear")]),
        entry_on(7, "scaleY", &[(0, 100.0, "linear"), (1000, 20.0, "linear")]),
    ];
    for (case, scale, entries, expected) in [
        ("a nest at 100", 100.0, Vec::new(), vec![hole_edge.clone()]),
        (
            "a nest at 25",
            25.0,
            Vec::new(),
            vec![hole_edge.clone(), near.clone()],
        ),
        (
            "a nest keyed to 20",
            100.0,
            nest_keys,
            vec![hole_edge.clone(), near.clone()],
        ),
    ] {
        let (project, omissions) = export_over_canvas(vec![nest(scale)], entries);
        let project = project.unwrap_or_else(|error| panic!("{case}: {error}: {omissions:?}"));
        assert_eq!(
            project
                .single_sequence()
                .unwrap()
                .nest_occurrences()
                .count(),
            1,
            "{case}: {omissions:?}"
        );
        assert_eq!(reasons(&omissions), expected, "{case}: {omissions:?}");
    }
    // A Vector Motion Scale key whose Bezier easing passes below its keys,
    // or one that reaches 0, bounds no clearance: the contours are reported
    // near, and the shape still exports.
    let mut child = donut;
    child["parent"] = json!(8);
    child["activeRange"] = json!({"start": 0, "duration": 1000});
    let group = graphic_group(json!({
        "transform": {"anchorPoint": [0, 0], "position": [960, 540], "scale": [100, 100], "rotation": 0, "opacity": 100},
        "layers": [child],
    }));
    let overshoot = |axis: &str| {
        with_bezier(
            entry_on(8, axis, &[(0, 100.0, "linear"), (1000, 90.0, "linear")]),
            1,
            [0.5, -3.0, 0.5, 1.0],
        )
    };
    let from_zero = |axis: &str| entry_on(8, axis, &[(0, 0.0, "linear"), (1000, 100.0, "linear")]);
    let flip = |axis: &str| entry_on(8, axis, &[(0, 100.0, "linear"), (1000, -100.0, "linear")]);
    for (case, entries) in [
        (
            "a Scale key that eases past its keys",
            vec![overshoot("scaleX"), overshoot("scaleY")],
        ),
        (
            "a Scale key from 0",
            vec![from_zero("scaleX"), from_zero("scaleY")],
        ),
        // Keys of both signs pass through 0; export reports them and keeps
        // the static Scale, so this report is conservative.
        (
            "Scale keys of both signs",
            vec![flip("scaleX"), flip("scaleY")],
        ),
    ] {
        let (project, omissions) = export_over_canvas(vec![group.clone()], entries);
        let project = project.unwrap_or_else(|error| panic!("{case}: {error}: {omissions:?}"));
        assert!(
            project
                .single_sequence()
                .unwrap()
                .video_items()
                .filter_map(PrVideoItem::graphic)
                .any(|graphic| matches!(graphic.objects.as_slice(), [PrGraphicObject::Group(_)])),
            "{case}: {omissions:?}"
        );
        assert_eq!(
            reasons(&omissions),
            [hole_edge.clone(), super::compound_unbounded_approximation()],
            "{case}: {omissions:?}"
        );
    }
}

fn identity_subgroup(id: u64, name: &str, layers: Vec<Value>) -> Value {
    json!({
        "type": "Group",
        "id": id,
        "name": name,
        "parent": 8,
        "playback": crate::test_support::linear_playback(json!({"start": 0, "duration": 1000}), json!({"start": 0, "duration": 1000})),
        "transform": {"anchorPoint": [0, 0], "position": [0, 0], "scale": [100, 100], "rotation": 0, "opacity": 100},
        "layers": layers,
    })
}

#[test]
fn adobe_shape_attached_masks_keep_the_graphic_frame_and_export_owner_and_guide_edits() {
    for (case, occurrence, inverted, transformed) in [
        ("b", "VideoClipTrackItem:66", false, false),
        ("b", "VideoClipTrackItem:67", true, false),
        ("c", "VideoClipTrackItem:65", false, true),
    ] {
        let (native, mut root) = native_mask_graphic(case, occurrence);
        let owner = native
            .objects
            .iter()
            .find_map(|object| match object {
                PrGraphicObject::Shape(shape) if shape.mask.is_some() => Some(shape),
                _ => None,
            })
            .unwrap();
        let native_mask = owner.mask.as_ref().unwrap();
        assert_eq!(native_mask.inverted, inverted);
        assert!(native_mask.path_keys.is_empty());
        if transformed {
            assert_eq!(owner.transform.scale, 80.0);
            assert_eq!(owner.transform.rotation, 30.0);
        }
        let children = root["layers"].as_array_mut().unwrap();
        let owner_index = children
            .iter()
            .position(|layer| layer["name"] == owner.name)
            .unwrap();
        assert_eq!(
            children[owner_index]["transform"]["rotation"],
            if transformed { json!(30.0) } else { json!(0.0) }
        );
        assert_eq!(
            children[owner_index]["transform"]["scale"],
            if transformed {
                json!([80.0, 80.0])
            } else {
                json!([100.0, 100.0])
            }
        );
        let guide_id = children[owner_index]["masks"][0]["layer"].clone();
        let guide_index = children
            .iter()
            .position(|layer| layer["id"] == guide_id)
            .unwrap();
        assert_eq!(
            children[owner_index]["parent"],
            children[guide_index]["parent"]
        );
        assert_eq!(
            children[guide_index]["transform"]["position"],
            json!([0.0, 0.0])
        );
        assert_eq!(children[guide_index]["transform"]["rotation"], 0.0);
        assert_eq!(
            children[guide_index]["transform"]["scale"],
            json!([100.0, 100.0])
        );
        assert!(children
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != owner_index)
            .all(|(_, layer)| layer.get("masks").is_none()));
        let commands = children[guide_index]["shape"]["path"]["commands"]
            .as_array()
            .unwrap();
        for (axis, expected) in [("x", [864.0, 1248.0]), ("y", [324.0, 756.0])] {
            let values: Vec<_> = commands
                .iter()
                .filter_map(|command| command[axis].as_f64())
                .collect();
            let bounds = [
                values.iter().copied().fold(f64::INFINITY, f64::min),
                values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
            ];
            for (actual, expected) in bounds.into_iter().zip(expected) {
                assert!(
                    (actual - expected).abs() < 0.001,
                    "{case} {axis}: {bounds:?}"
                );
            }
        }
        let x = children[guide_index]["shape"]["path"]["commands"][0]["x"]
            .as_f64()
            .unwrap();
        assert!((x - f64::from(native_mask.path.vertices[0].point[0]) * 1920.0).abs() < 0.001);
        children[guide_index]["shape"]["path"]["commands"][0]["x"] = json!(x + 24.0);
        children[owner_index]["transform"]["rotation"] = json!(17.0);
        children[owner_index]["shape"]["fills"][0]["paint"]["color"] = json!([1.0, 0.0, 0.0, 1.0]);
        for layer in children {
            layer["activeRange"] = json!({"start": 0, "duration": 1000});
        }
        root["playback"] = crate::test_support::linear_playback(
            json!({"start": 0, "duration": 1000}),
            json!({"start": 0, "duration": 1000}),
        );
        let mut sibling = sibling_text();
        sibling["id"] = json!(90000);
        let (graphics, omissions) = exported_and_read(vec![root, sibling]);
        let written = graphics
            .iter()
            .flat_map(|graphic| &graphic.objects)
            .find_map(|object| match object {
                PrGraphicObject::Shape(shape) if shape.name == owner.name => Some(shape),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{case}: {omissions:?}"));
        assert_eq!(written.transform.rotation, 17.0);
        assert_eq!(
            written.appearance.fill,
            Some(crate::schema::text::PrFill::Solid(
                crate::schema::text::PrRgb([255, 0, 0])
            ))
        );
        let mask = written.mask.as_ref().unwrap();
        assert_eq!(mask.inverted, inverted);
        assert!((f64::from(mask.path.vertices[0].point[0]) * 1920.0 - x - 24.0).abs() < 0.001);
        let written_count: usize = graphics.iter().filter(|graphic| graphic.objects.iter().any(|object| matches!(object, PrGraphicObject::Shape(shape) if shape.name == owner.name))).map(|graphic| graphic.objects.len()).sum();
        assert_eq!(
            written_count,
            native.objects.len(),
            "siblings: {omissions:?}"
        );
    }
}

#[test]
fn attached_numeric_mask_keys_use_the_owner_clock_and_export_edits() {
    // The human-saved Shape mask has Linear keys with nonzero stored speeds.
    let (mut graphic, _) = native_mask_graphic("numeric", "VideoClipTrackItem:65");
    // Supplementary clock shifts must not move owner-local keys.
    graphic.start_ticks = 2 * TICKS;
    graphic.end_ticks = 5 * TICKS;
    graphic.in_ticks += 2 * TICKS;
    let owner = graphic
        .objects
        .iter_mut()
        .find_map(|object| match object {
            PrGraphicObject::Shape(shape) if shape.mask.is_some() => Some(shape),
            _ => None,
        })
        .unwrap();
    let owner_name = owner.name.clone();
    let mask = owner.mask.as_mut().unwrap();
    for (keys, values) in [
        (&mut mask.feather_keys, [0.0, 24.0]),
        (&mut mask.expansion_keys, [-12.0, 20.0]),
        (&mut mask.opacity_keys, [100.0, 40.0]),
    ] {
        assert_eq!(
            *keys,
            [
                scalar(0, values[0], PrKeyframeEasing::Linear),
                scalar(2 * TICKS / 3, values[1], PrKeyframeEasing::Linear),
            ]
        );
        for key in keys {
            key.source_ticks += 2 * TICKS;
        }
    }
    let (mut wire, omissions) = imported(graphic);
    let layers = wire["composition"]["layers"].as_array_mut().unwrap();
    layers.retain(|layer| layer["type"] != "Video");
    let root = &layers[0];
    let owner = root["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["name"] == owner_name)
        .unwrap_or_else(|| panic!("{omissions:?}"));
    assert_eq!(owner["activeRange"]["start"], 0);
    assert_eq!(owner["transform"]["rotation"], 30.0);
    let id = owner["masks"][0]["id"].clone();
    let guide_id = owner["masks"][0]["layer"].clone();
    let guide = root["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["id"] == guide_id)
        .unwrap();
    assert_eq!(owner["parent"], guide["parent"]);
    assert_eq!(guide["transform"]["rotation"], 0.0);
    let mut canvas = editable_document()["composition"]["layers"][1].clone();
    canvas["id"] = json!(90000);
    canvas["activeRange"]["duration"] = json!(5000);
    layers.push(canvas);
    let entries = wire["composition"]["dynamics"]["entries"]
        .as_array_mut()
        .unwrap();
    assert_eq!(entries.len(), 3);
    for entry in entries {
        assert_eq!(entry["target"]["itemId"], id);
        let name = entry["target"]["propertyName"].as_str().unwrap().to_owned();
        let keys = entry["animator"]["keyframes"].as_array_mut().unwrap();
        assert_eq!(keys[0]["layerTime"], 0);
        // Native 2/3 second rounds to the editable model's millisecond clock.
        assert_eq!(keys[1]["layerTime"], 667);
        match name.as_str() {
            "feather" => assert_eq!(keys[1]["value"]["value"], json!([24.0, 24.0])),
            "opacity" => assert_eq!(keys[1]["value"]["value"], 0.4),
            "expansion" => {
                keys[1]["layerTime"] = json!(1500);
                keys[1]["value"]["value"] = json!(-24.0);
            }
            _ => panic!("unexpected target {name}"),
        }
    }
    wire["duration"] = json!(5.0);
    let document = EditableFxCompositionDocument::from_json_value(wire).unwrap();
    let mut omissions = Vec::new();
    let exported = crate::convert::export_document(
        &document,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        FrameRate::Fps30,
        &mut omissions,
    )
    .unwrap();
    for entry in document.composition().dynamics().entries() {
        assert!(exported.written.contains(&entry.target), "{omissions:?}");
    }
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("attached-numeric-mask.prproj");
    PremiereProjectXml::new(&exported.project)
        .unwrap()
        .write_new(&path)
        .unwrap();
    let (read, reports) = PrProjectFile::load(&path).unwrap();
    let graphic = read.sequences[0]
        .video_items()
        .filter_map(PrVideoItem::graphic)
        .find(|graphic| {
            graphic.objects.iter().any(|object| {
                matches!(object,
            PrGraphicObject::Shape(shape) if shape.name == owner_name)
            })
        })
        .unwrap_or_else(|| panic!("{reports:?}"));
    let shape = graphic
        .objects
        .iter()
        .find_map(|object| match object {
            PrGraphicObject::Shape(shape) if shape.name == owner_name => Some(shape),
            _ => None,
        })
        .unwrap();
    let mask = shape.mask.as_ref().unwrap();
    assert_eq!(mask.expansion_keys[0].source_ticks, graphic.in_ticks);
    assert_eq!(
        mask.expansion_keys[1].source_ticks,
        graphic.in_ticks + 3 * TICKS / 2
    );
    assert_eq!(mask.expansion_keys[1].value, -24.0);
    assert_eq!(mask.feather_keys[1].value, 24.0);
    assert_eq!(mask.opacity_keys[1].value, 40.0);
    assert_eq!(mask.opacity_keys[1].easing, PrKeyframeEasing::Linear);
    assert!(graphic.opacity_mask.is_none());
    assert_eq!(shape.transform.rotation, 30.0);
}

#[test]
fn an_unsupported_edited_attachment_keeps_the_owners_sibling() {
    let mut owner = square_object("Owner", [700.0, 500.0], 300.0, [0, 96, 255], None);
    if let PrGraphicObject::Shape(shape) = &mut owner {
        shape.mask = Some(frame_mask(0.3, 0.4));
    }
    let mut root = imported_graphic_group(vec![
        owner,
        square_object("Other", [1200.0, 540.0], 200.0, [0, 96, 255], None),
    ]);
    let guide = &mut root["layers"][1];
    guide["transform"]["position"] = json!([1.0, 0.0]);
    let (graphics, omissions) = exported_and_read(vec![root, sibling_text()]);
    assert_eq!(exported_names(&graphics), ["Other"]);
    assert!(
        omissions
            .iter()
            .any(|report| report.reason.contains("mask guide is not at the identity")),
        "{omissions:?}"
    );
}

#[test]
fn native_narrowing_rejects_a_collapsed_hole_without_losing_a_sibling() {
    const A: f64 = 16_777_216.0;
    let shape = |size: f64| {
        let mut layer = imported_object(shape_graphic());
        let mut commands = square_contour(A - 100.0, A + 100.0, false);
        commands.extend([
            json!({"type": "moveTo", "x": A, "y": A}),
            json!({"type": "lineTo", "x": A + size, "y": A}),
            json!({"type": "lineTo", "x": A + size, "y": A + size}),
            json!({"type": "close"}),
        ]);
        layer["shape"]["path"]["commands"] = json!(commands);
        layer["shape"]["fills"][0]["fillRule"] = json!("evenOdd");
        layer
    };
    let (graphics, omissions) = exported_and_read(vec![shape(0.5), sibling_text()]);
    assert_eq!(
        graphics
            .iter()
            .map(|graphic| part_names(&graphic.objects))
            .collect::<Vec<_>>(),
        ["Sibling"],
        "{omissions:?}"
    );
    assert!(
        omissions.iter().any(|report| report.record.contains("Box")
            && report.reason.contains("native contour topology")),
        "{omissions:?}"
    );
    let (pieces, omissions) = exported_pieces(shape(32.0));
    assert_eq!(pieces.unwrap_or_else(|| panic!("{omissions:?}")).len(), 2);
}

#[test]
fn a_failed_attached_mask_source_takes_its_lower_composite_only() {
    let square = |name, role| square_object(name, [700.0, 500.0], 300.0, [0, 96, 255], role);
    let mut owner = square("Owner", None);
    if let PrGraphicObject::Shape(shape) = &mut owner {
        shape.mask = Some(frame_mask(0.3, 0.4));
    }
    let attached = imported_graphic_group(vec![owner]);
    let mut guide = attached["layers"][1].clone();
    guide["id"] = json!(90001);
    guide["transform"]["position"] = json!([1.0, 0.0]);
    let mut masks = attached["layers"][0]["masks"].clone();
    masks[0]["layer"] = json!(90001);
    masks[0]["id"] = json!(90002);
    let mut root = imported_graphic_group(vec![
        square("Above", None),
        square("Source", INVERTED),
        square("Lower", None),
    ]);
    guide["parent"] = root["id"].clone();
    root["layers"][1]["masks"] = masks;
    root["layers"].as_array_mut().unwrap().insert(2, guide);
    let (graphics, omissions) = exported_and_read(vec![root, sibling_text()]);
    assert_eq!(exported_names(&graphics), ["Above"], "{omissions:?}");
    assert!(
        omissions.iter().any(
            |report| report.reason.contains("mask guide is not at the identity")
                && report
                    .reason
                    .contains("objects that it masks are not exported")
        ),
        "{omissions:?}"
    );
}

#[test]
fn invalid_attached_numeric_keys_typed_import_preserves_only_healthy_scope() {
    for property in ["feather", "expansion", "opacity"] {
        for role in [None, INVERTED] {
            let square =
                |name, role| square_object(name, [700.0, 500.0], 300.0, [0, 96, 255], role);
            let mut owner = square("Owner", role);
            if let PrGraphicObject::Shape(shape) = &mut owner {
                let mut mask = frame_mask(0.3, 0.4);
                let keys = vec![scalar(EXPORT_IN, 1001.0, PrKeyframeEasing::Linear)];
                match property {
                    "feather" => mask.feather_keys = keys,
                    "expansion" => mask.expansion_keys = keys,
                    _ => mask.opacity_keys = keys,
                }
                shape.mask = Some(mask);
            }
            assert!(owner
                .validate()
                .unwrap_err()
                .to_string()
                .contains("key must be finite and within"));
            let (document, omissions) = imported(PrGraphic {
                objects: vec![square("Above", None), owner, square("Lower", None)],
                ..shape_graphic()
            });
            let names: Vec<_> = document["composition"]["layers"][0]["layers"]
                .as_array()
                .unwrap()
                .iter()
                .map(|layer| layer["name"].as_str().unwrap())
                .collect();
            assert_eq!(
                names,
                if role.is_some() {
                    vec!["Above"]
                } else {
                    vec!["Above", "Lower"]
                },
                "{property}: {omissions:?}"
            );
            assert!(
                omissions
                    .iter()
                    .any(|o| o.reason.contains("key must be finite and within")),
                "{omissions:?}"
            );
        }
    }
}

#[test]
fn invalid_attached_numeric_keys_edited_export_preserves_only_healthy_scope() {
    for property in ["feather", "expansion", "opacity"] {
        for role in [None, INVERTED] {
            let square =
                |name, role| square_object(name, [700.0, 500.0], 300.0, [0, 96, 255], role);
            let mut owner = square("Owner", None);
            if let PrGraphicObject::Shape(shape) = &mut owner {
                shape.mask = Some(frame_mask(0.3, 0.4));
            }
            let attached = imported_graphic_group(vec![owner]);
            let mut guide = attached["layers"][1].clone();
            guide["id"] = json!(90001);
            let mut masks = attached["layers"][0]["masks"].clone();
            masks[0]["layer"] = json!(90001);
            masks[0]["id"] = json!(90002);
            let mut root = imported_graphic_group(vec![
                square("Above", None),
                square("Owner", role),
                square("Lower", None),
            ]);
            guide["parent"] = root["id"].clone();
            root["layers"][1]["masks"] = masks;
            root["layers"].as_array_mut().unwrap().insert(2, guide);
            let mut track = entry(property, &[(0, 1.0, "linear"), (1000, 1.0, "linear")]);
            track["animator"]["keyframes"][1]["easing"] = json!({
                "type": "cubicBezier", "x1": 0.3, "y1": 0.2, "x2": 0.7, "y2": 0.8
            });
            track["target"] =
                json!({"kind": "fxItemProperty", "itemId": 90002, "propertyName": property});
            if property == "feather" {
                for key in track["animator"]["keyframes"].as_array_mut().unwrap() {
                    key["value"] = json!({"type": "vector2", "value": [1.0, 1.0]});
                }
            }
            let (project, omissions) = export_over_canvas(vec![root, sibling_text()], vec![track]);
            let project = project.unwrap();
            let graphics: Vec<_> = project.sequences[0]
                .video_items()
                .filter_map(PrVideoItem::graphic)
                .cloned()
                .collect();
            assert_eq!(
                exported_names(&graphics),
                if role.is_some() {
                    vec!["Above"]
                } else {
                    vec!["Above Lower"]
                },
                "{property}: {omissions:?}"
            );
            assert!(graphics.iter().any(|g| part_names(&g.objects) == "Sibling"));
            assert!(
                omissions.iter().any(|o| o
                    .reason
                    .contains("supports only Linear, Hold or zero-speed Bezier")),
                "{omissions:?}"
            );
        }
    }
}

#[test]
fn attached_static_signed_expansion_survives_with_approximation_in_both_directions() {
    for expansion in [-12.0, 12.0] {
        let mut owner = square_object("Owner", [700.0, 500.0], 300.0, [0, 96, 255], None);
        if let PrGraphicObject::Shape(shape) = &mut owner {
            let mut mask = frame_mask(0.3, 0.4);
            mask.expansion = expansion;
            shape.mask = Some(mask);
        }
        let graphic = PrGraphic {
            objects: vec![owner],
            ..shape_graphic()
        };
        let (document, omissions) = imported(graphic.clone());
        assert_eq!(
            document["composition"]["layers"][0]["layers"][0]["masks"][0]["expansion"],
            expansion
        );
        assert!(
            omissions
                .iter()
                .any(|o| o.kind == OmissionKind::Approximated
                    && o.reason.contains("negative expansion remains editable")),
            "{omissions:?}"
        );
        let (written, omissions) = written_back(graphic);
        let PrGraphicObject::Shape(shape) = &written.objects[0] else {
            panic!()
        };
        assert_eq!(shape.mask.as_ref().unwrap().expansion, expansion);
        assert!(
            omissions
                .iter()
                .filter(|o| o.kind == OmissionKind::Approximated
                    && o.reason.contains("negative expansion remains editable"))
                .count()
                >= 2,
            "{omissions:?}"
        );
    }
}

#[test]
fn vertical_text_keys_export_and_import_without_horizontal_keys() {
    let (graphic, omissions) = exported(
        vec![entry(
            "scaleY",
            &[(0, 5.0, "linear"), (500, 120.0, "linear")],
        )],
        None,
    );
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(graphic.text().horizontal_scale, Some(100.0));
    assert_eq!(
        graphic.text().animations,
        [PrPropertyAnimation::UniformScale(vec![
            scalar(EXPORT_IN, 5.0, PrKeyframeEasing::Linear),
            scalar(EXPORT_IN + TICKS / 2, 120.0, PrKeyframeEasing::Linear),
        ])]
    );
    let (document, omissions) = imported(graphic);
    assert_eq!(omissions.len(), 1, "{omissions:?}");
    assert!(
        omissions[0]
            .reason
            .contains("font \"Inter-Bold\" is not packaged"),
        "{omissions:?}"
    );
    let scales: Vec<_> = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| {
            entry["target"]["propertyType"]
                .as_str()
                .unwrap()
                .starts_with("scale")
        })
        .collect();
    assert_eq!(scales.len(), 1);
    assert_eq!(scales[0]["target"]["propertyType"], "scaleY");
}
