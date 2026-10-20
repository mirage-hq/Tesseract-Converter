//! Public native-derived Group regression through the archive export adapter.
//! The clock/effect edits are explicit FX inputs, not new Adobe render proof.

use super::*;
use crate::{AfterEffects, effects::native::read_effects, properties::read_transform};
use fx_conv::{ConversionMode, ConversionReport, ExportFromTesseract};
use fx_schema::{LayerId, PropType, PropertyValue};
use sha2::{Digest, Sha256};
use tesseract_file::{AssetKind, TesseractFileBuilder};

const MOVIE: &[u8] =
    include_bytes!("../../../tests/fixtures/source-clock-opacity/primary-input.mp4");

fn containing_adjustment(layer: &Value) -> Option<&Value> {
    let children = layer["layers"].as_array()?;
    if children
        .iter()
        .any(|child| child["name"] == "nested-local-adjustment")
    {
        return Some(layer);
    }
    children.iter().find_map(containing_adjustment)
}

fn input() -> Value {
    const NATIVE: &[u8] = include_bytes!("../../../tests/fixtures/adjustment/native_controls.aep");
    assert_eq!(
        format!("{:x}", Sha256::digest(NATIVE)),
        "ed45319ed014ba979a9e0b4868aa635775f288fe851c39d9242aab097020f26f"
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(MOVIE)),
        "d54833c618c2d5b1b013939f154859a19164838469d3e6356c0c2122ea21af6c"
    );
    let native = read_project(NATIVE).unwrap();
    let imported = to_structural_fx_document(&native, Some(126)).unwrap();
    let mut value = imported.document.to_json_value().unwrap();
    let mut owner = value["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(containing_adjustment)
        .expect("pinned native nested Adjustment scope")
        .clone();
    owner["id"] = json!(19000);
    owner["name"] = json!("Public final-scene Group");
    owner["parent"] = Value::Null;
    owner["transform"] = serde_json::to_value(identity_fx_transform()).unwrap();
    owner.as_object_mut().unwrap().remove("activeRange");
    owner["playback"] = fixture_linear_playback(
        json!({"start":16500,"duration":12500}),
        json!({"start":0,"duration":12500}),
    );
    let children = owner["layers"].as_array_mut().unwrap();
    for child in children.iter_mut() {
        child["parent"] = json!(19000);
        if child["name"] == "nested-local-adjustment" {
            child["id"] = json!(14800);
            child["activeRange"] = json!({"start":0,"duration":4000});
            child["effects"] = json!([
                {"id":14801,"enabled":true,"effect":{
                    "type":"exposure","exposure":0.6,"offset":0.0,"gammaCorrection":1.0
                }},
                {"id":14802,"enabled":true,"effect":{
                    "type":"tintTritone","amount":18.0,
                    "blackR":0.0,"blackG":0.015,"blackB":0.025,
                    "whiteR":0.68,"whiteG":1.0,"whiteB":1.0
                }}
            ]);
            child["transform"]["opacity"] = json!(0.0);
        }
    }
    for (id, name, start, duration) in [
        (18010, "Public landing", 0, 4000),
        (18020, "Public getaway", 4000, 8500),
    ] {
        children.push(json!({
            "type":"Video","id":id,"name":name,"parent":19000,
            "playback":fixture_linear_playback(
                json!({"start":start,"duration":duration}),
                json!({"start":0,"duration":duration})
            ),
            "sourceRange":{"start":0,"duration":duration},
            "sourceIntrinsicDuration":18000,
            "transform":{"anchorPoint":[0,0],"position":[0,0],"scale":[100,100],
                "rotation":0,"opacity":100},
            "source":{"assetId":name,"fit":"contain"}
        }));
    }
    // These edited source-local keys reproduce the rejected pointwise stack;
    // all original public native sibling geometry remains intact.
    let opacity = keyed_entry(
        LayerId::new(14800),
        PropType::Opacity,
        [
            (0, PropertyValue::Float(0.0)),
            (2180, PropertyValue::Float(0.0)),
            (2220, PropertyValue::Float(100.0)),
            (2263, PropertyValue::Float(50.0)),
            (2310, PropertyValue::Float(0.0)),
            (4000, PropertyValue::Float(0.0)),
        ],
    );
    value["duration"] = json!(29.0);
    value["composition"]["layers"] = json!([owner]);
    value["composition"]["dynamics"] = json!({"entries":[opacity]});
    value
}

fn published(
    value: Value,
) -> (
    tempfile::TempDir,
    StructuralProject,
    ConversionReport<ExportDiagnostic>,
) {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("input.tsrct");
    let movie = root.path().join("primary-input.mp4");
    std::fs::write(&movie, MOVIE).unwrap();
    let document = EditableFxCompositionDocument::from_json_value(value).unwrap();
    TesseractFileBuilder::try_new(document)
        .unwrap()
        .add_asset("Public landing", &movie, AssetKind::Video)
        .unwrap()
        .add_asset("Public getaway", &movie, AssetKind::Video)
        .unwrap()
        .write(&source)
        .unwrap();
    let output = root.path().join("output");
    let report = AfterEffects
        .export_from_tesseract(&source, &output, &Default::default(), ConversionMode::Write)
        .unwrap();
    let native = read_project(&std::fs::read(output.join("project.aep")).unwrap()).unwrap();
    (root, native, report)
}

#[test]
fn pointwise_adjustment_group_public_export_keeps_editable_children_and_clocks() {
    let (root, native, report) = published(input());
    let owner = layers(&native)
        .iter()
        .find(|layer| layer.name.as_ref() == "Public final-scene Group")
        .unwrap_or_else(|| panic!("missing actual Group: {:?}", report.diagnostics));
    assert_eq!(owner.record.parent_id(), 0);
    assert_eq!(owner.record.start_time_fraction(), (33, 2));
    assert_eq!(owner.record.in_point_fraction(), (0, 1));
    assert_eq!(owner.record.out_point_fraction(), (25, 2));
    assert_eq!(owner.record.stretch_fraction(), (1, 1));
    let ItemKind::Composition(source) = &native.item(owner.record.source_id()).unwrap().kind else {
        panic!("retained editable Group source")
    };
    assert_eq!([source.width, source.height], [320, 180]);
    assert_eq!(source.duration_secs, 12.5);
    assert_eq!(
        source
            .layers
            .iter()
            .map(|layer| layer.name.as_ref())
            .collect::<Vec<_>>(),
        [
            "upper-sentinel-37x23",
            "nested-local-adjustment",
            "lower-amber-83x117",
            "lower-cobalt-127x71",
            "Public landing",
            "Public getaway"
        ]
    );
    // Positions are independently recorded in adjustment/native-readback.json,
    // composition126. The temporal wrapper must not shift its native siblings.
    for (index, expected) in [
        (0, [287.0, 19.0, 0.0]),
        (2, [238.0, 108.0, 0.0]),
        (3, [77.0, 58.0, 0.0]),
    ] {
        let position = read_transform(&source.layers[index].content)
            .unwrap()
            .into_iter()
            .find(|property| property.match_name == "ADBE Position")
            .unwrap()
            .numeric
            .unwrap();
        assert_eq!(position.values, expected);
    }
    let adjustment = &source.layers[1];
    assert!(adjustment.record.flags().adjustment_layer);
    assert_eq!(adjustment.record.parent_id(), 0);
    assert_eq!(adjustment.record.in_point(), Some(0.0));
    assert_eq!(adjustment.record.out_point(), Some(4.0));
    let (effects, _) = read_effects(&adjustment.content, [320.0, 180.0]);
    assert_eq!(
        effects
            .iter()
            .map(|effect| effect.match_name.as_str())
            .collect::<Vec<_>>(),
        ["ADBE Exposure2", "ADBE Tint"]
    );
    for (effect, name, expected) in [
        (&effects[0], "ADBE Exposure2-0003", 0.6),
        (&effects[1], "ADBE Tint-0003", 18.0),
    ] {
        let control = effect
            .parameters
            .iter()
            .find(|parameter| parameter.match_name == name)
            .unwrap()
            .numeric
            .as_ref()
            .unwrap();
        assert_eq!(control.values, [expected]);
    }
    let opacity = read_transform(&adjustment.content)
        .unwrap()
        .into_iter()
        .find(|property| property.match_name == "ADBE Opacity")
        .unwrap()
        .numeric
        .unwrap();
    assert_eq!(
        opacity
            .keyframes
            .iter()
            .map(|key| key.values.clone())
            .collect::<Vec<_>>(),
        [
            vec![0.0],
            vec![0.0],
            vec![1.0],
            vec![0.5],
            vec![0.0],
            vec![0.0]
        ]
    );
    for (key, expected) in opacity
        .keyframes
        .iter()
        .zip([0.0, 2.18, 2.22, 2.263, 2.31, 4.0])
    {
        // Ordinary scalar keys retain the existing nearest24576-Hz clock.
        assert!((key.time_secs - expected).abs() <= 0.5 / 24576.0);
    }
    for (index, start, duration) in [(4, (0, 1), (4, 1)), (5, (4, 1), (17, 2))] {
        let movie = &source.layers[index];
        assert_eq!(movie.record.parent_id(), 0);
        assert_eq!(movie.record.start_time_fraction(), start);
        assert_eq!(movie.record.in_point_fraction(), (0, 1));
        assert_eq!(movie.record.out_point_fraction(), duration);
        assert_eq!(movie.record.stretch_fraction(), (1, 1));
        assert!(
            native
                .item(movie.record.source_id())
                .unwrap()
                .media
                .is_some()
        );
        let position = read_transform(&movie.content)
            .unwrap()
            .into_iter()
            .find(|property| property.match_name == "ADBE Position")
            .unwrap()
            .numeric
            .unwrap();
        assert_eq!(position.values, [0.0, 0.0, 0.0]);
    }
    let staged_movies = report
        .artifacts
        .iter()
        .filter(|artifact| artifact.path.starts_with("media/"))
        .map(|artifact| std::fs::read(root.path().join("output").join(&artifact.path)).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(staged_movies.len(), 2);
    assert!(staged_movies.iter().all(|bytes| bytes == MOVIE));
}

#[test]
fn pointwise_adjustment_group_rejects_spatial_and_unknown_effects() {
    for effect in [
        json!({"type":"gaussianBlur","blurriness":25,"repeatEdgePixels":true}),
        json!({"type":"unknownFutureEffect","radius":25}),
    ] {
        let mut value = input();
        value["composition"]["layers"][0]["layers"][1]["effects"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id":14803,"enabled":true,"effect":effect}));
        let (_, native, report) = published(value);
        assert!(
            layers(&native)
                .iter()
                .all(|layer| layer.name.as_ref() != "Public final-scene Group")
        );
        assert!(native.items.iter().all(|item| item.media.is_none()));
        assert!(
            report
                .diagnostics
                .iter()
                .any(
                    |diagnostic| diagnostic.layer_id == Some(LayerId::new(19000))
                        && diagnostic.message.contains("sampling-domain enclosure")
                ),
            "{:?}",
            report.diagnostics
        );
    }
}
