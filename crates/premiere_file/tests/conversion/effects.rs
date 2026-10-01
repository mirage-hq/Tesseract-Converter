//! Standard clip effects through the public conversion API.
//!
//! `feature_effect_stack_strict.prproj` is derived from the isolated two-track
//! fixture: V1 plays `video-30fps-10s.mp4` 0-6 s under an active Gaussian Blur
//! (25) at chain `Index` 0 and a bypassed one (80) at Index 1; V2 plays 2-4 s
//! under a real `AE.ADBE Tint` (Premiere 12.1: Map Black To (163, 247, 143),
//! Map White To (240, 242, 22), Amount 100). Premiere applies a chain in
//! descending `Index` (Oracle run C6, F24), so the bypassed blur is first in
//! the stack. The static Blurriness records are inferred, because every corpus
//! Blurriness is keyframed. Its pinned AME render is a scored `video_reference`
//! gate since the Tint converts (JRB-2001); while the Tint was omitted it was a
//! diagnostic mismatch. Premiere UI controls were not inspected.
//!
//! `feature_gaussian_blur_strict.prproj` derives from it for a blur-only Adobe
//! comparison on textured media: V1 plays the timecoded
//! `feature_timecoded_source.mp4` 0-3 s under one active Gaussian Blur (25)
//! and, after a cut, source 3-6 s under one active Gaussian Blur (80); V2 is
//! empty.
//!
//! `feature_gaussian_blur_keys_26_5_strict.prproj` is saved by Premiere 26.5.1
//! (authored through its scripting bridge; UI not inspected). V1 plays the
//! timecoded source 0-10 s without effects. V2 holds four 2.5 s clips whose
//! "Gaussian Blur (Legacy)" Blurriness is keyed: A Linear 80 to 0; B Hold
//! 0/40/0 at chain Index 0 with a static blur of 10 at Index 1, which applies
//! first; C Bezier 60/10/50/0 from source In 1 s, with keys before the In and
//! after the Out; D Linear 120 to 150, a blur that hides the burned-in
//! timecode. Every keyed first key is 0.5 s after its clip's In except C's, and
//! every StartKeyframe keeps the static 25.
//!
//! `feature_corner_pin_strict.prproj` is saved by Premiere 26.5.1 (Oracle run
//! E1: authored through its scripting bridge, the corner keys added by one
//! logged XML edit that Premiere re-saved; UI not inspected). V1 plays the
//! timecoded source 0-10 s. V2 plays `feature_linked_av_source.mp4` under one
//! Corner Pin per clip: A a static skew; B off-frame corners; C, from source
//! In 1 s, Upper Left keys (Linear, then Hold) whose first key is before the
//! In; D Upper Left and Upper Right keys (a spin that widens the top edge
//! off-frame); E the identity quad.
//!
//! `feature_directional_blur_strict.prproj` is saved by Premiere 26.5.1 (Oracle
//! run E2): one "Directional Blur (Legacy)" per V2 clip, as Direction/Blur
//! Length: A 90/10; B 0/30; C 45/20; F 0/30 on Scale 50 and Rotation 30; D 90
//! with Blur Length keys from source In 0.5 s (10 before the In, Linear to 0
//! and to 30, which holds until 0); E 30/90.
//!
//! `feature_levels_strict.prproj` is saved by Premiere 26.5.1 (Oracle run E4:
//! authored through its scripting bridge; UI not inspected). V1 plays
//! `feature_linked_av_source.mp4` under one Levels per clip, its (R), (G) and
//! (B) rows neutral: A black input 3; B input 20-235 and output 16-240; C
//! Gamma 150; D, from source In 0.5 s, (RGB) White Output keys 255, Linear to
//! 128, then a Hold to 200; E Gamma 70.
//!
//! `feature_brightness_contrast_strict.prproj` is saved by Premiere 26.5.1
//! (Oracle run E3; UI not inspected): V1 clips of `feature_linked_av_source.mp4`
//! under "Brightness & Contrast", as Brightness/Contrast: A 37/-25; B 37/-25
//! then -10/25 in its chain; C -40/85; D, from source In 0.5 s, Brightness keys
//! 0, Linear to 60, which holds until 0, over Contrast 20; E 37/-25 bypassed.
//!
//! `feature_invert_strict.prproj` is saved by Premiere 26.5.1 (Oracle run E5;
//! UI not inspected): V1 clips of `feature_linked_av_source.mp4` under "Invert"
//! of every channel (Channel 0), as Blend With Original: A 0; B 30; C, from
//! source In 1 s, keys 100 at source 1.5 s, Linear to 20 at 2 s, held from 3 s
//! until 100 at 3.5 s; D 0 from source In 2 s. Every record carries an opaque
//! `PremiereFilterPrivateData` (A's and D's stored, B's and C's empty copies).
//!
//! `feature_tint_strict.prproj` is saved by Premiere 26.5.1 (Oracle run E6;
//! colours by one logged XML edit that Premiere re-saved; UI not inspected):
//! V1 clips of `feature_linked_av_source.mp4` under "Tint", as Map Black To /
//! Map White To / Amount to Tint: A the defaults, black to white, 100; B
//! (163, 247, 143) to (240, 242, 22), 100; C black to (255, 128, 0), 50; D as
//! C from source In 0.5 s, Amount keys 0 at source 1 s, Linear to 100 at 1.5 s,
//! held until 50 at 2.5 s; E black to white keys white at source 0.5 s, Linear
//! to (0, 128, 255) at 1 s, 100.
//!
//! `feature_black_white_strict.prproj` is saved by Premiere 26.5.1 (Oracle run
//! E7; UI not inspected): V1 plays the same source twice, 0-5 s and 5-10 s
//! (source 0-5 s each), each clip under one "Black & White", a record without
//! parameters.
//!
//! `feature_transform_strict.prproj` is saved by Premiere 26.5.1 (Oracle run
//! E11; UI not inspected): V1 plays the timecoded source 0-10 s; V2 plays
//! `feature_linked_av_source.mp4` under one "Transform" per clip: A Position
//! 0.75:0.5 and Opacity 50 (0-2 s); B Anchor Point 0.75:0.5, Uniform Scale 50
//! (Scale Width 100 saved) and Rotation 30 (2-4 s); C Skew 30, Skew Axis 45
//! (4-5 s); F Position keys with the composition's shutter angle off and
//! Shutter Angle 180 (5-6 s); D, from source In 0.5 s, Scale Height keys 100
//! at source 1 s, Linear to 200 at 1.5 s, held until 50 at 2.5 s, and Rotation
//! keys 0 to 90 (6-8.5 s); E Position 0.75:0.5 and Uniform Scale 150 under
//! Motion Scale 50 and Rotation 30 (8.5-10 s).

use super::support::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use premiere_file::PrProjectFile;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use tesseract_file::{TesseractFile, TesseractFileBuilder};

const SEQUENCE: &str = "c8acf9c1-34b2-4086-9f55-d528950a7059";
const KEYS_SEQUENCE: &str = "134988e1-16ad-4d21-bd02-eedc067977f5";
const KEYS_FIXTURE: &str = "feature_gaussian_blur_keys_26_5_strict.prproj";
const CORNER_PIN_SEQUENCE: &str = "8272f008-9d6c-4cb9-82ea-1b5533679b62";
const CORNER_PIN_FIXTURE: &str = "feature_corner_pin_strict.prproj";
const DIRECTIONAL_BLUR_SEQUENCE: &str = "584967aa-9e0f-4443-a4b4-eaec0c026771";
const DIRECTIONAL_BLUR_FIXTURE: &str = "feature_directional_blur_strict.prproj";
const LEVELS_SEQUENCE: &str = "0edbae34-77df-4111-abe0-0a5f9f40365b";
const LEVELS_FIXTURE: &str = "feature_levels_strict.prproj";
const BRIGHTNESS_CONTRAST_SEQUENCE: &str = "1ccb6ec4-94bf-47f2-8f4f-377727321f84";
const BRIGHTNESS_CONTRAST_FIXTURE: &str = "feature_brightness_contrast_strict.prproj";
const INVERT_SEQUENCE: &str = "d4f92305-b5e5-415d-8607-f32fda7b0778";
const INVERT_FIXTURE: &str = "feature_invert_strict.prproj";
const TINT_SEQUENCE: &str = "4e85c3c5-e459-4e3d-b38d-4813cc71c5a4";
const TINT_FIXTURE: &str = "feature_tint_strict.prproj";
const BLACK_WHITE_SEQUENCE: &str = "79ee48e5-47c7-4a95-bccf-c1f1be95aefd";
const BLACK_WHITE_FIXTURE: &str = "feature_black_white_strict.prproj";
const RAMP_SEQUENCE: &str = "2d806b19-f9dc-479f-a77d-faba4ed7e2bb";
const RAMP_FIXTURE: &str = "feature_ramp_strict.prproj";
const MOSAIC_SEQUENCE: &str = "3d1dcc32-4054-4d9b-97ec-f5d24b9dafb3";
const MOSAIC_FIXTURE: &str = "feature_mosaic_strict.prproj";
const TRANSFORM_SEQUENCE: &str = "3379b147-d652-4736-bcda-996213f56410";
const TRANSFORM_FIXTURE: &str = "feature_transform_strict.prproj";

/// Each Blurriness track of a converted document, by the start of the video
/// layer whose effect it animates: the layer time, value and easing of each key.
fn blurriness_tracks(document: &Value) -> BTreeMap<i64, Vec<(i64, f64, Value)>> {
    let owners: BTreeMap<u64, i64> = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|layer| layer["type"] == "Video")
        .flat_map(|layer| {
            let start = (*crate::test_support::layer_range(layer))["start"]
                .as_i64()
                .unwrap();
            layer["effects"]
                .as_array()
                .into_iter()
                .flatten()
                .map(move |effect| (effect["id"].as_u64().unwrap(), start))
        })
        .collect();
    document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            assert_eq!(entry["target"]["kind"], "effectProperty", "{entry}");
            assert_eq!(entry["target"]["paramName"], "blurriness", "{entry}");
            let keys = entry["animator"]["keyframes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|key| {
                    (
                        key["layerTime"].as_i64().unwrap(),
                        key["value"]["value"].as_f64().unwrap(),
                        key["easing"].clone(),
                    )
                })
                .collect();
            let effect_id = entry["target"]["effectId"].as_u64().unwrap();
            (owners[&effect_id], keys)
        })
        .collect()
}

/// Asserts the four cubic handles of an FX easing.
fn assert_cubic(easing: &Value, [x1, y1, x2, y2]: [f64; 4]) {
    assert_eq!(easing["type"], "cubicBezier", "{easing}");
    for (name, expected) in [("x1", x1), ("y1", y1), ("x2", x2), ("y2", y2)] {
        let actual = easing[name].as_f64().unwrap();
        assert!((actual - expected).abs() < 1e-12, "{name}: {easing}");
    }
}

/// The Blurriness tracks of the keys fixture, by clip start: layer times on
/// each clip's clock, and C's Bezier handles from the native speeds as written
/// (its last in-speed `-5` is Premiere's truncated -50/s, read literally).
fn assert_fixture_tracks(
    tracks: &BTreeMap<i64, Vec<(i64, f64, Value)>>,
    first_a_key: f64,
    d_start: i64,
) {
    let linear = || json!({"type": "linear"});
    let hold = || json!({"type": "hold"});
    assert_eq!(
        tracks.keys().copied().collect::<Vec<_>>(),
        [0, 2500, 5000, d_start]
    );
    assert_eq!(
        tracks[&0],
        [(500, first_a_key, linear()), (1500, 0.0, linear())]
    );
    assert_eq!(
        tracks[&2500],
        [
            (500, 0.0, linear()),
            (1000, 40.0, hold()),
            (1500, 0.0, hold())
        ]
    );
    let c = &tracks[&5000];
    assert_eq!(
        c.iter()
            .map(|(time, value, _)| (*time, *value))
            .collect::<Vec<_>>(),
        [(-500, 60.0), (1000, 10.0), (2000, 50.0), (3000, 0.0)]
    );
    assert_eq!(c[0].2, linear());
    let sixth = 1.0 / 6.0;
    assert_cubic(&c[1].2, [sixth, 0.0, 1.0 - sixth, 1.0 - sixth]);
    assert_cubic(&c[2].2, [sixth, 0.0, 1.0 - sixth, 1.0 - sixth]);
    assert_cubic(&c[3].2, [sixth, 0.0, 1.0 - sixth, 1.0 - sixth * 0.1]);
    assert_eq!(
        tracks[&d_start],
        [(500, 120.0, linear()), (2000, 150.0, linear())]
    );
}

/// Every current Gaussian Blur Amount in document order: its static value and keys.
fn native_amount(project: &Path) -> Vec<(String, Option<String>)> {
    let xml = read_xml(project);
    let document = roxmltree::Document::parse(&xml).unwrap();
    let text = |node: roxmltree::Node<'_, '_>, tag: &str| {
        node.children()
            .find(|child| child.has_tag_name(tag))
            .and_then(|child| child.text())
            .map(str::to_owned)
    };
    document
        .root_element()
        .children()
        .filter(|node| text(*node, "Name").as_deref() == Some("Amount"))
        .map(|param| {
            (
                text(param, "StartKeyframe").unwrap(),
                text(param, "Keyframes"),
            )
        })
        .collect()
}

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn effect_stack(document: &Value, start_millis: i64) -> Value {
    document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| {
            layer["type"] == "Video"
                && (*crate::test_support::layer_range(layer))["start"] == start_millis
        })
        .unwrap()["effects"]
        .clone()
}

/// Current Gaussian Blur Bypass and Amount values of a native project, in the V1
/// chain's document order, which is its ascending `Index` order in these projects.
fn native_blurs(project: &Path) -> Vec<(String, String)> {
    let xml = read_xml(project);
    let document = roxmltree::Document::parse(&xml).unwrap();
    let root = document.root_element();
    let record = |id: &str| {
        root.children()
            .find(|node| node.attribute("ObjectID") == Some(id))
            .unwrap()
    };
    let child_text = |node: roxmltree::Node<'_, '_>, tag: &str| {
        node.descendants()
            .find(|child| child.has_tag_name(tag))
            .and_then(|child| child.text())
            .unwrap()
            .to_owned()
    };
    let chain = root
        .children()
        .filter(|node| node.has_tag_name("VideoComponentChain"))
        .find(|node| {
            node.descendants()
                .any(|child| child.has_tag_name("Components"))
        })
        .unwrap();
    chain
        .descendants()
        .filter(|node| node.has_tag_name("Component") && node.attribute("ObjectRef").is_some())
        .map(|reference| record(reference.attribute("ObjectRef").unwrap()))
        .map(|filter| {
            assert_eq!(child_text(filter, "MatchName"), "AE.Impact_Blur_FX");
            let amount = filter
                .descendants()
                .find(|node| node.has_tag_name("Param") && node.attribute("Index") == Some("5"))
                .map(|param| record(param.attribute("ObjectRef").unwrap()))
                .map(|param| child_text(param, "StartKeyframe"))
                .unwrap();
            let bypass = filter
                .descendants()
                .find(|node| node.has_tag_name("Bypass"))
                .and_then(|node| node.text())
                .unwrap_or("false")
                .to_owned();
            (bypass, amount)
        })
        .collect()
}

#[test]
fn adobe_derived_stack_imports_order_bypass_and_its_tint() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("converted");
    let omissions = premiere_to_tesseract(
        fixture_path("feature_effect_stack_strict.prproj"),
        &output,
        Some(SEQUENCE),
        false,
    )
    .unwrap();
    // The Premiere 12.1 Tint converts too (JRB-2001); before, it was the
    // reported unknown effect of this fixture.
    assert!(omissions.is_empty(), "{omissions:?}");
    let document = TesseractFile::open(first_project(&output))
        .unwrap()
        .project_json()
        .unwrap();
    // Effect ids follow import order, V2's Tint first.
    assert_eq!(
        effect_stack(&document, 2000),
        json!([fx_tint(1, ([163, 247, 143], [240, 242, 22], 100.0))])
    );
    assert_eq!(
        effect_stack(&document, 0),
        json!([
            {"id": 2, "enabled": false, "effect": {"type": "gaussianBlur", "blurriness": 80.0}},
            {"id": 3, "enabled": true, "effect": {"type": "gaussianBlur", "blurriness": 25.0}},
        ])
    );
}

#[test]
fn textured_blur_fixture_imports_one_active_blur_per_clip() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("converted");
    let omissions = premiere_to_tesseract(
        fixture_path("feature_gaussian_blur_strict.prproj"),
        &output,
        Some(SEQUENCE),
        false,
    )
    .unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let document = TesseractFile::open(first_project(&output))
        .unwrap()
        .project_json()
        .unwrap();
    // Each clip keeps its own blur, and the second clip continues the source
    // where the first ends, so the burned-in timecode stays continuous.
    let videos: Vec<_> = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|layer| layer["type"] == "Video")
        .map(|layer| {
            json!({
                "activeRange": (*crate::test_support::layer_range(layer)),
                "sourceRange": layer["sourceRange"],
                "effects": layer["effects"],
            })
        })
        .collect();
    assert_eq!(
        videos,
        [
            json!({
                "activeRange": {"start": 0, "duration": 3000},
                "sourceRange": {"start": 0, "duration": 3000},
                "effects": [{"id": 1, "enabled": true, "effect": {"type": "gaussianBlur", "blurriness": 25.0}}],
            }),
            json!({
                "activeRange": {"start": 3000, "duration": 3000},
                "sourceRange": {"start": 3000, "duration": 3000},
                "effects": [{"id": 2, "enabled": true, "effect": {"type": "gaussianBlur", "blurriness": 80.0}}],
            }),
        ]
    );
}

#[test]
fn edited_stack_order_and_bypass_survive_export_and_reimport() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    premiere_to_tesseract(
        fixture_path("feature_effect_stack_strict.prproj"),
        root.join("converted"),
        Some(SEQUENCE),
        false,
    )
    .unwrap();
    let converted = first_project(&root.join("converted"));

    // Unedited export keeps the imported order and bypass: the fixture's
    // chain, with the active blur, which applies last, at Index 0.
    let omissions = tesseract_to_premiere(&converted, root.join("native"), false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let exported = root.join("native/project.prproj");
    assert_eq!(
        native_blurs(&exported),
        [
            (
                "false".to_owned(),
                "-91445760000000000,4.385964912280701,0,0,0,0,0,0".to_owned()
            ),
            (
                "true".to_owned(),
                "-91445760000000000,14.035087719298245,0,0,0,0,0,0".to_owned()
            ),
        ]
    );
    let (reloaded, omissions) = PrProjectFile::load(&exported).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(
        reloaded
            .sequences()
            .next()
            .unwrap()
            .video_occurrences()
            .count(),
        2
    );

    // An edit moves the bypassed blur last in the stack and enables it.
    let mut document = TesseractFile::open(&converted)
        .unwrap()
        .project_json()
        .unwrap();
    let layer = document["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|layer| {
            (*crate::test_support::layer_range(layer))["start"] == 0 && layer["type"] == "Video"
        })
        .unwrap();
    let effects = layer["effects"].as_array_mut().unwrap();
    effects.swap(0, 1);
    effects[1]["enabled"] = json!(true);
    let edited = archive(root, &document, &fixture_path("video-30fps-10s.mp4"));
    tesseract_to_premiere(&edited, root.join("edited-native"), false).unwrap();
    let edited_native = root.join("edited-native/project.prproj");
    // The blur that now applies last, 80, is at Index 0.
    assert_eq!(
        native_blurs(&edited_native),
        [
            (
                "false".to_owned(),
                "-91445760000000000,14.035087719298245,0,0,0,0,0,0".to_owned()
            ),
            (
                "false".to_owned(),
                "-91445760000000000,4.385964912280701,0,0,0,0,0,0".to_owned()
            ),
        ]
    );

    // Reimport restores the edited stack as editable effects: blur 25 at
    // Index 1 applies first. Effect ids follow import order, V2's Tint first.
    let omissions =
        premiere_to_tesseract(&edited_native, root.join("reimported"), None, false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let reimported = TesseractFile::open(first_project(&root.join("reimported")))
        .unwrap()
        .project_json()
        .unwrap();
    assert_eq!(
        effect_stack(&reimported, 0),
        json!([
            {"id": 2, "enabled": true, "effect": {"type": "gaussianBlur", "blurriness": 25.0}},
            {"id": 3, "enabled": true, "effect": {"type": "gaussianBlur", "blurriness": 80.0}},
        ])
    );
}

#[test]
fn adobe_keyed_blurs_import_as_editable_blurriness_tracks() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("converted");
    let omissions = premiere_to_tesseract(
        fixture_path(KEYS_FIXTURE),
        &output,
        Some(KEYS_SEQUENCE),
        false,
    )
    .unwrap();
    // Only the track items' UI nodes are reported; every blur converts.
    assert!(
        omissions
            .iter()
            .all(|omission| omission.reason.contains("ClipTrackItem/TrackItem/Node")),
        "{omissions:?}"
    );
    let document = TesseractFile::open(first_project(&output))
        .unwrap()
        .project_json()
        .unwrap();
    // Premiere applies a chain in descending `Index` (Oracle run C6): B's
    // static blur (component ID 3) at Index 1 applies first, then its keyed
    // blur (component ID 4) at Index 0. A keyed Blurriness starts at its first
    // key, not at the StartKeyframe 25.
    let blur = |id: u64, blurriness: f64, repeat_edge_pixels: bool| {
        let mut effect = json!({"type": "gaussianBlur", "blurriness": blurriness});
        if repeat_edge_pixels {
            effect["repeatEdgePixels"] = json!(true);
        }
        json!({"id": id, "enabled": true, "effect": effect})
    };
    for (start, effects) in [
        (0, json!([blur(1, 80.0, true)])),
        (2500, json!([blur(2, 10.0, true), blur(3, 0.0, false)])),
        (5000, json!([blur(4, 60.0, true)])),
        (7500, json!([blur(5, 120.0, true)])),
    ] {
        assert_eq!(
            effect_stack(&document, start),
            effects,
            "clip at {start} ms"
        );
    }
    assert_fixture_tracks(&blurriness_tracks(&document), 80.0, 7500);
}

#[test]
fn edited_keyed_blurs_survive_export_and_reimport() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    premiere_to_tesseract(
        fixture_path(KEYS_FIXTURE),
        root.join("converted"),
        Some(KEYS_SEQUENCE),
        false,
    )
    .unwrap();
    let mut document = TesseractFile::open(first_project(&root.join("converted")))
        .unwrap()
        .project_json()
        .unwrap();
    // Edit: A's first key 80 becomes 90, and D, whose blur hides the
    // timecode, moves 100 ms earlier over the end of C.
    document["composition"]["dynamics"]["entries"][0]["animator"]["keyframes"][0]["value"]
        ["value"] = json!(90.0);
    let layers = document["composition"]["layers"].as_array_mut().unwrap();
    let d = layers
        .iter_mut()
        .find(|layer| {
            layer["type"] == "Video" && (*crate::test_support::layer_range(layer))["start"] == 7500
        })
        .unwrap();
    d["playback"]["inputRange"]["start"] = json!(7400);
    d["playback"]["mapping"]["input"]["start"] = json!(7400);
    let edited = archive(
        root,
        &document,
        &fixture_path("feature_timecoded_source.mp4"),
    );
    let omissions = tesseract_to_premiere(&edited, root.join("native"), false).unwrap();
    // Every blur is written as the current Gaussian Blur, none is reported or lost.
    assert!(omissions.is_empty(), "{omissions:?}");
    let exported = root.join("native/project.prproj");
    assert_eq!(
        read_xml(&exported)
            .matches("<MatchName>AE.Impact_Blur_FX</MatchName>")
            .count(),
        5
    );
    let (_, omissions) = PrProjectFile::load(&exported).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    // A keyed Amount is written with its first key as the StartKeyframe,
    // which is the value AME renders before a later first key.
    let blurriness = native_amount(&exported);
    assert_eq!(blurriness.len(), 5);
    assert_eq!(
        blurriness[0],
        (
            "-91445760000000000,15.789473684210526,0,0,0,0,0,0".to_owned(),
            Some(
                "127008000000,15.789473684210526,0,0,0,0,0,0;381024000000,0,0,0,0,0,0,0;"
                    .to_owned()
            )
        )
    );
    assert_eq!(
        blurriness
            .iter()
            .filter(|(_, keys)| keys.is_none())
            .map(|(start, _)| start.as_str())
            .collect::<Vec<_>>(),
        ["-91445760000000000,1.7543859649122806,0,0,0,0,0,0"]
    );

    // Reimport restores the edited keys on the layer clocks; D's keys moved
    // with it.
    let omissions = premiere_to_tesseract(&exported, root.join("reimported"), None, false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let reimported = TesseractFile::open(first_project(&root.join("reimported")))
        .unwrap()
        .project_json()
        .unwrap();
    assert_eq!(
        effect_stack(&reimported, 7400)[0]["effect"],
        json!({"type": "gaussianBlur", "blurriness": 120.0, "repeatEdgePixels": true})
    );
    assert_eq!(
        effect_stack(&reimported, 0)[0]["effect"]["blurriness"],
        90.0
    );
    assert_fixture_tracks(&blurriness_tracks(&reimported), 90.0, 7400);
}

/// Each video layer's effect stack in a converted document, by layer start,
/// for the layers that have effects.
fn effect_stacks(document: &Value) -> BTreeMap<i64, Value> {
    document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|layer| layer["type"] == "Video" && layer.get("effects").is_some())
        .map(|layer| {
            (
                (*crate::test_support::layer_range(layer))["start"]
                    .as_i64()
                    .unwrap(),
                layer["effects"].clone(),
            )
        })
        .collect()
}

/// An imported, enabled Corner Pin with its corners in native order.
fn corner_pin(id: u64, corners: [[f64; 2]; 4]) -> Value {
    let [[ulx, uly], [urx, ury], [llx, lly], [lrx, lry]] = corners;
    json!({"id": id, "enabled": true, "effect": {"type": "cornerPin",
        "upperLeftX": ulx, "upperLeftY": uly, "upperRightX": urx, "upperRightY": ury,
        "lowerLeftX": llx, "lowerLeftY": lly, "lowerRightX": lrx, "lowerRightY": lry}})
}

/// Corner tracks by the start of the video layer whose Corner Pin they animate
/// and their FX parameter: the layer time, value and easing type of each key.
type CornerTracks = BTreeMap<(i64, String), Vec<(i64, f64, String)>>;

/// Each corner track of a converted document.
fn corner_tracks(document: &Value) -> CornerTracks {
    let owners: BTreeMap<u64, i64> = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|layer| layer["type"] == "Video")
        .flat_map(|layer| {
            let start = (*crate::test_support::layer_range(layer))["start"]
                .as_i64()
                .unwrap();
            layer["effects"]
                .as_array()
                .into_iter()
                .flatten()
                .map(move |effect| (effect["id"].as_u64().unwrap(), start))
        })
        .collect();
    document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            assert_eq!(entry["target"]["kind"], "effectProperty", "{entry}");
            let keys = entry["animator"]["keyframes"]
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
                .collect();
            let owner = owners[&entry["target"]["effectId"].as_u64().unwrap()];
            let param = entry["target"]["paramName"].as_str().unwrap().to_owned();
            ((owner, param), keys)
        })
        .collect()
}

/// The corner tracks of the Corner Pin fixture: C's Upper Left on its clock
/// from source In 1 s, with its third key at `c_third_key` ms, then D's spin
/// of Upper Left and Upper Right, with D starting at `d_start` ms. Each
/// coordinate keeps its corner's key times and easing.
fn assert_corner_fixture_tracks(tracks: &CornerTracks, c_third_key: i64, d_start: i64) {
    let track = |start: i64, param: &str, keys: &[(i64, f64, &str)]| {
        let keys = keys
            .iter()
            .map(|&(millis, value, easing)| (millis, value, easing.to_owned()))
            .collect();
        ((start, param.to_owned()), keys)
    };
    let linear_spin = |values: [f64; 3]| {
        [
            (500, values[0], "linear"),
            (1000, values[1], "linear"),
            (1500, values[2], "linear"),
        ]
    };
    assert_eq!(
        tracks,
        &BTreeMap::from([
            track(
                4000,
                "upperLeftX",
                &[
                    (-500, 0.2, "linear"),
                    (500, 0.0, "linear"),
                    (c_third_key, 0.3, "linear"),
                    (2000, 0.1, "hold")
                ]
            ),
            track(
                4000,
                "upperLeftY",
                &[
                    (-500, 0.2, "linear"),
                    (500, 0.0, "linear"),
                    (c_third_key, 0.2, "linear"),
                    (2000, 0.1, "hold")
                ]
            ),
            track(d_start, "upperLeftX", &linear_spin([0.0, -0.3, 0.0])),
            track(d_start, "upperLeftY", &linear_spin([0.0, 0.0, 0.0])),
            track(d_start, "upperRightX", &linear_spin([1.0, 1.3, 1.0])),
            track(d_start, "upperRightY", &linear_spin([0.0, 0.0, 0.0])),
        ])
    );
}

#[test]
fn adobe_corner_pins_import_as_editable_corner_tracks() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("converted");
    let omissions = premiere_to_tesseract(
        fixture_path(CORNER_PIN_FIXTURE),
        &output,
        Some(CORNER_PIN_SEQUENCE),
        false,
    )
    .unwrap();
    // Only UI nodes and the linked master clip's `DefMappingID` are reported;
    // every Corner Pin converts.
    assert!(
        omissions.iter().all(|omission| {
            omission.reason.contains("ClipTrackItem/TrackItem/Node")
                || omission.reason.contains("DefMappingID")
        }),
        "{omissions:?}"
    );
    let document = TesseractFile::open(first_project(&output))
        .unwrap()
        .project_json()
        .unwrap();
    // A keyed corner starts at its first key, not at its StartKeyframe 0:0:
    // C's Upper Left at 0.2:0.2.
    let identity = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]];
    assert_eq!(
        effect_stacks(&document),
        BTreeMap::from([
            (
                0,
                json!([corner_pin(
                    1,
                    [[0.1, 0.05], [0.95, 0.0], [0.0, 1.0], [0.85, 0.9]]
                )])
            ),
            (
                2000,
                json!([corner_pin(
                    2,
                    [[-0.2, 0.0], [1.3, 0.1], [0.1, 1.0], [0.9, 1.0]]
                )])
            ),
            (
                4000,
                json!([corner_pin(
                    3,
                    [[0.2, 0.2], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]]
                )])
            ),
            (6500, json!([corner_pin(4, identity)])),
            (9000, json!([corner_pin(5, identity)])),
        ])
    );
    assert_corner_fixture_tracks(&corner_tracks(&document), 1000, 6500);
}

/// Every Corner Pin corner record of a native project in document order: its
/// `Name`, `StartKeyframe` and `Keyframes`.
fn native_corners(project: &Path) -> Vec<(String, String, Option<String>)> {
    let xml = read_xml(project);
    let document = roxmltree::Document::parse(&xml).unwrap();
    let text = |node: roxmltree::Node<'_, '_>, tag: &str| {
        node.children()
            .find(|child| child.has_tag_name(tag))
            .and_then(|child| child.text())
            .map(str::to_owned)
    };
    document
        .root_element()
        .children()
        .filter(|node| {
            node.has_tag_name("PointComponentParam")
                && text(*node, "Name").is_some_and(|name| {
                    ["Upper Left", "Upper Right", "Lower Left", "Lower Right"]
                        .contains(&name.as_str())
                })
        })
        .map(|param| {
            (
                text(param, "Name").unwrap(),
                text(param, "StartKeyframe").unwrap(),
                text(param, "Keyframes"),
            )
        })
        .collect()
}

#[test]
fn edited_corner_pins_survive_export_and_reimport() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    premiere_to_tesseract(
        fixture_path(CORNER_PIN_FIXTURE),
        root.join("converted"),
        Some(CORNER_PIN_SEQUENCE),
        false,
    )
    .unwrap();
    let converted = TesseractFile::open(first_project(&root.join("converted"))).unwrap();
    let mut document = converted.project_json().unwrap();
    // Edit: C's third Upper Left key moves from 1 s to 1.2 s on its clock,
    // and D moves 100 ms earlier over the end of C.
    for entry in document["composition"]["dynamics"]["entries"]
        .as_array_mut()
        .unwrap()
    {
        if entry["target"]["effectId"] == 3 {
            entry["animator"]["keyframes"][2]["layerTime"] = json!(1200);
        }
    }
    let layers = document["composition"]["layers"].as_array_mut().unwrap();
    let d = layers
        .iter_mut()
        .find(|layer| {
            layer["type"] == "Video" && (*crate::test_support::layer_range(layer))["start"] == 6500
        })
        .unwrap();
    d["playback"]["inputRange"]["start"] = json!(6400);
    d["playback"]["mapping"]["input"]["start"] = json!(6400);
    let edited = root.join("edited.tsrct");
    let mut builder =
        TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap()).unwrap();
    for (id, asset) in &converted.metadata().assets {
        let name = Path::new(&asset.path).file_name().unwrap();
        builder = builder
            .add_asset(
                id.as_str(),
                fixture_path(name.to_str().unwrap()),
                asset.kind,
            )
            .unwrap();
    }
    builder.write(&edited).unwrap();
    let omissions = tesseract_to_premiere(&edited, root.join("native"), false).unwrap();
    // Every Corner Pin is written; none is reported or lost.
    assert!(omissions.is_empty(), "{omissions:?}");
    let exported = root.join("native/project.prproj");
    assert_eq!(
        read_xml(&exported)
            .matches("<MatchName>AE.ADBE Corner Pin</MatchName>")
            .count(),
        5
    );
    let (_, omissions) = PrProjectFile::load(&exported).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    // Three corners are keyed. C's Upper Left has its first key as
    // `StartKeyframe` and its keys on the source clock from In 1 s, with the
    // moved key at source 2.2 s.
    let corners = native_corners(&exported);
    assert_eq!(corners.len(), 20);
    let keyed: Vec<_> = corners
        .iter()
        .filter(|(_, _, keys)| keys.is_some())
        .collect();
    assert_eq!(keyed.len(), 3, "{keyed:?}");
    let c_upper_left = (
        "Upper Left".to_owned(),
        "-91445760000000000,0.2:0.2,0,0,0,0,0,0,5,4,0,0,0,0".to_owned(),
        Some("127008000000,0.2:0.2,0,0,0,0,0,0,0,0,0,0,0,0;381024000000,0:0,0,0,0,0,0,0,0,0,0,0,0,0;558835200000,0.3:0.2,4,0,0,0,0,0,0,0,0,0,0,0;762048000000,0.1:0.1,0,0,0,0,0,0,0,0,0,0,0,0;".to_owned()),
    );
    assert!(keyed.contains(&&c_upper_left), "{keyed:?}");

    // Reimport restores every corner and key; D's keys moved with it.
    let omissions = premiere_to_tesseract(&exported, root.join("reimported"), None, false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let reimported = TesseractFile::open(first_project(&root.join("reimported")))
        .unwrap()
        .project_json()
        .unwrap();
    // Export gives D its own track, so reimport mints the effect ids in
    // another order; the stacks compare without them.
    let without_ids = |mut stacks: BTreeMap<i64, Value>| {
        for effect in stacks
            .values_mut()
            .flat_map(|stack| stack.as_array_mut().unwrap())
        {
            effect.as_object_mut().unwrap().remove("id");
        }
        stacks
    };
    let identity = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]];
    let expected = BTreeMap::from([
        (
            0,
            json!([corner_pin(
                0,
                [[0.1, 0.05], [0.95, 0.0], [0.0, 1.0], [0.85, 0.9]]
            )]),
        ),
        (
            2000,
            json!([corner_pin(
                0,
                [[-0.2, 0.0], [1.3, 0.1], [0.1, 1.0], [0.9, 1.0]]
            )]),
        ),
        (
            4000,
            json!([corner_pin(
                0,
                [[0.2, 0.2], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]]
            )]),
        ),
        (6400, json!([corner_pin(0, identity)])),
        (9000, json!([corner_pin(0, identity)])),
    ]);
    assert_eq!(
        without_ids(effect_stacks(&reimported)),
        without_ids(expected)
    );
    assert_corner_fixture_tracks(&corner_tracks(&reimported), 1200, 6400);
}

#[test]
fn adobe_directional_blurs_import_in_composition_space() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("converted");
    let omissions = premiere_to_tesseract(
        fixture_path(DIRECTIONAL_BLUR_FIXTURE),
        &output,
        Some(DIRECTIONAL_BLUR_SEQUENCE),
        false,
    )
    .unwrap();
    // Only UI nodes and the linked master clip's `DefMappingID` are reported;
    // every Directional Blur converts, F's on its scaled and rotated clip too.
    assert!(
        omissions.iter().all(|omission| {
            omission.reason.contains("ClipTrackItem/TrackItem/Node")
                || omission.reason.contains("DefMappingID")
        }),
        "{omissions:?}"
    );
    let document = TesseractFile::open(first_project(&output))
        .unwrap()
        .project_json()
        .unwrap();
    // In composition space F's 0/30 on Scale 50 and Rotation 30 is 30/15, and
    // D's keyed Blur Length starts at its first key, 10 at source 0 s.
    let blur = |id: u64, direction: f64, blur_length: f64| {
        json!([{"id": id, "enabled": true, "effect": {"type": "directionalBlur",
            "direction": direction, "blurLength": blur_length}}])
    };
    assert_eq!(
        effect_stacks(&document),
        BTreeMap::from([
            (0, blur(1, 90.0, 10.0)),
            (2000, blur(2, 0.0, 30.0)),
            (4000, blur(3, 45.0, 20.0)),
            (5000, blur(4, 30.0, 15.0)),
            (6000, blur(5, 90.0, 10.0)),
            (8500, blur(6, 30.0, 90.0)),
        ])
    );
    // D's track on its clock from source In 0.5 s: the key before the In,
    // Linear keys and the Hold.
    let linear = || "linear".to_owned();
    assert_eq!(
        corner_tracks(&document),
        BTreeMap::from([(
            (6000, "blurLength".to_owned()),
            vec![
                (-500, 10.0, linear()),
                (500, 0.0, linear()),
                (1000, 30.0, linear()),
                (2000, 0.0, "hold".to_owned()),
            ]
        )])
    );
}

#[test]
fn edited_directional_blurs_survive_export_and_reimport() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    premiere_to_tesseract(
        fixture_path(DIRECTIONAL_BLUR_FIXTURE),
        root.join("converted"),
        Some(DIRECTIONAL_BLUR_SEQUENCE),
        false,
    )
    .unwrap();
    let converted = TesseractFile::open(first_project(&root.join("converted"))).unwrap();
    let mut document = converted.project_json().unwrap();
    // Edit: A's Blur Length 10 becomes 25, F's Direction 30 in composition
    // space becomes 45, and D's second Blur Length key 0 becomes 12.
    let layers = document["composition"]["layers"].as_array_mut().unwrap();
    for (start, field, before, after) in [
        (0, "blurLength", 10.0, 25.0),
        (5000, "direction", 30.0, 45.0),
    ] {
        let layer = layers
            .iter_mut()
            .find(|layer| {
                (*crate::test_support::layer_range(layer))["start"] == start
                    && layer.get("effects").is_some()
            })
            .unwrap();
        let effect = &mut layer["effects"][0]["effect"];
        assert_eq!(effect[field], before, "{start} {field}");
        effect[field] = json!(after);
    }
    let [entry] = document["composition"]["dynamics"]["entries"]
        .as_array_mut()
        .unwrap()
        .as_mut_slice()
    else {
        panic!("expected D's Blur Length track only");
    };
    entry["animator"]["keyframes"][1]["value"]["value"] = json!(12.0);
    let edited = root.join("edited.tsrct");
    let mut builder =
        TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap()).unwrap();
    for (id, asset) in &converted.metadata().assets {
        let name = Path::new(&asset.path).file_name().unwrap();
        builder = builder
            .add_asset(
                id.as_str(),
                fixture_path(name.to_str().unwrap()),
                asset.kind,
            )
            .unwrap();
    }
    builder.write(&edited).unwrap();
    let omissions = tesseract_to_premiere(&edited, root.join("native"), false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let exported = root.join("native/project.prproj");
    let xml = read_xml(&exported);
    let written = |match_name: &str| {
        xml.matches(&format!("<MatchName>{match_name}</MatchName>"))
            .count()
    };
    assert_eq!(written("AE.Impact_Directional_Blur_FX"), 6);
    assert_eq!(written("AE.ADBE Motion Blur"), 0);

    // Reimport restores every edited value and key in composition space, so
    // export inverts the import's map of each clip's frame. Export gives the
    // clips their own tracks, so reimport can mint the effect ids in another
    // order; the stacks compare without them.
    let omissions = premiere_to_tesseract(&exported, root.join("reimported"), None, false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let reimported = TesseractFile::open(first_project(&root.join("reimported")))
        .unwrap()
        .project_json()
        .unwrap();
    let without_ids = |document: &Value| {
        let mut stacks = effect_stacks(document);
        for effect in stacks
            .values_mut()
            .flat_map(|stack| stack.as_array_mut().unwrap())
        {
            effect.as_object_mut().unwrap().remove("id");
        }
        stacks
    };
    assert_eq!(without_ids(&reimported), without_ids(&document));
    assert_eq!(corner_tracks(&reimported), corner_tracks(&document));
}

/// An imported, enabled Levels with its FX `inputBlack`, `inputWhite`,
/// `gamma`, `outputBlack` and `outputWhite`.
fn fx_levels(id: u64, values: [f64; 5]) -> Value {
    let [input_black, input_white, gamma, output_black, output_white] = values;
    json!({"id": id, "enabled": true, "effect": {"type": "levels", "inputBlack": input_black,
        "inputWhite": input_white, "gamma": gamma, "outputBlack": output_black, "outputWhite": output_white}})
}

/// The Levels stacks of the fixture, with A's black input `a_black`, D at
/// `d_start` ms and E's `e_gamma`.
fn levels_fixture_stacks(a_black: f64, d_start: i64, e_gamma: f64) -> BTreeMap<i64, Value> {
    BTreeMap::from([
        (0, json!([fx_levels(1, [a_black, 255.0, 1.0, 0.0, 255.0])])),
        (2000, json!([fx_levels(2, [20.0, 235.0, 1.0, 16.0, 240.0])])),
        (4000, json!([fx_levels(3, [0.0, 255.0, 1.5, 0.0, 255.0])])),
        (
            d_start,
            json!([fx_levels(4, [0.0, 255.0, 1.0, 0.0, 255.0])]),
        ),
        (
            8500,
            json!([fx_levels(5, [0.0, 255.0, e_gamma, 0.0, 255.0])]),
        ),
    ])
}

/// D's White Output track, by the fixture's clip start, with its middle key
/// at `middle` (layer time ms, value): Linear from 255, then a Hold to 200.
fn levels_fixture_tracks(middle: (i64, f64)) -> CornerTracks {
    let key = |millis: i64, value: f64, easing: &str| (millis, value, easing.to_owned());
    BTreeMap::from([(
        (6000, "outputWhite".to_owned()),
        vec![
            key(500, 255.0, "linear"),
            key(middle.0, middle.1, "linear"),
            key(2000, 200.0, "hold"),
        ],
    )])
}

#[test]
fn adobe_levels_import_as_editable_fx_levels() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("converted");
    let omissions = premiere_to_tesseract(
        fixture_path(LEVELS_FIXTURE),
        &output,
        Some(LEVELS_SEQUENCE),
        false,
    )
    .unwrap();
    // Only UI nodes and the linked master clip's `DefMappingID` are reported;
    // every Levels converts, its private data agreeing with its parameters.
    assert!(
        omissions.iter().all(|omission| {
            omission.reason.contains("ClipTrackItem/TrackItem/Node")
                || omission.reason.contains("DefMappingID")
        }),
        "{omissions:?}"
    );
    let document = TesseractFile::open(first_project(&output))
        .unwrap()
        .project_json()
        .unwrap();
    // Native Gamma is FX gamma in hundredths: C's 150 is 1.5 and E's 70 is 0.7.
    assert_eq!(
        effect_stacks(&document),
        levels_fixture_stacks(3.0, 6000, 0.7)
    );
    // `corner_tracks` reads every effect-parameter track: D's keys from source
    // In 0.5 s.
    assert_eq!(
        corner_tracks(&document),
        levels_fixture_tracks((1000, 128.0))
    );
}

/// Each Levels of a native project in document order: its parameters'
/// `StartKeyframe` values, the values that its private data stores, and the
/// `Keyframes` of its keyed parameters by name.
type NativeLevels = (Vec<u16>, Vec<u16>, Vec<(String, String)>);

fn native_levels(project: &Path) -> Vec<NativeLevels> {
    let xml = read_xml(project);
    let document = roxmltree::Document::parse(&xml).unwrap();
    let root = document.root_element();
    let text = |node: roxmltree::Node<'_, '_>, tag: &str| {
        node.children()
            .find(|child| child.has_tag_name(tag))
            .and_then(|child| child.text())
            .map(str::to_owned)
    };
    let record = |id: &str| {
        root.children()
            .find(|node| node.attribute("ObjectID") == Some(id))
            .unwrap()
    };
    root.children()
        .filter(|node| text(*node, "MatchName").as_deref() == Some("PR.ADBE Levels"))
        .map(|filter| {
            let params: Vec<_> = filter
                .descendants()
                .filter(|node| node.has_tag_name("Param"))
                .map(|param| record(param.attribute("ObjectRef").unwrap()))
                .collect();
            let start = params
                .iter()
                .map(|param| {
                    let start = text(*param, "StartKeyframe").unwrap();
                    start.split(',').nth(1).unwrap().parse().unwrap()
                })
                .collect();
            let private = STANDARD
                .decode(text(filter, "PremiereFilterPrivateData").unwrap().trim())
                .unwrap()
                .chunks(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .collect();
            let keys = params
                .iter()
                .filter_map(|param| Some((text(*param, "Name")?, text(*param, "Keyframes")?)))
                .collect();
            (start, private, keys)
        })
        .collect()
}

#[test]
fn edited_levels_survive_export_and_reimport() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    premiere_to_tesseract(
        fixture_path(LEVELS_FIXTURE),
        root.join("converted"),
        Some(LEVELS_SEQUENCE),
        false,
    )
    .unwrap();
    let converted = TesseractFile::open(first_project(&root.join("converted"))).unwrap();
    let mut document = converted.project_json().unwrap();
    // The gate's edit: A's black input 3 to 10, D's middle key from 1 s to
    // 1.2 s on its clock and from 128 to 100, and E's Gamma 0.7 to 0.5.
    for layer in document["composition"]["layers"].as_array_mut().unwrap() {
        let (field, value) = match (*crate::test_support::layer_range(layer))["start"].as_i64() {
            Some(0) => ("inputBlack", 10.0),
            Some(8500) => ("gamma", 0.5),
            _ => continue,
        };
        // The audio layers at the same times have no effects.
        if let Some(effect) = layer.pointer_mut("/effects/0/effect") {
            effect[field] = json!(value);
        }
    }
    let key = &mut document["composition"]["dynamics"]["entries"][0]["animator"]["keyframes"][1];
    key["layerTime"] = json!(1200);
    key["value"]["value"] = json!(100.0);
    let edited = root.join("edited.tsrct");
    let mut builder =
        TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap()).unwrap();
    for (id, asset) in &converted.metadata().assets {
        let name = Path::new(&asset.path).file_name().unwrap();
        builder = builder
            .add_asset(
                id.as_str(),
                fixture_path(name.to_str().unwrap()),
                asset.kind,
            )
            .unwrap();
    }
    builder.write(&edited).unwrap();
    let omissions = tesseract_to_premiere(&edited, root.join("native"), false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let exported = root.join("native/project.prproj");
    let (_, omissions) = PrProjectFile::load(&exported).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    // Each written Levels repeats its StartKeyframes in its private data, with
    // neutral (R), (G) and (B) rows. D's keys are on the source clock from In
    // 0.5 s, the moved key at source 1.7 s.
    let neutral_rows = [0, 255, 0, 255, 100].repeat(3);
    let masters = [
        [10, 255, 0, 255, 100],
        [20, 235, 16, 240, 100],
        [0, 255, 0, 255, 150],
        [0, 255, 0, 255, 100],
        [0, 255, 0, 255, 50],
    ];
    let written = native_levels(&exported);
    assert_eq!(written.len(), masters.len());
    for ((start, private, keys), master) in written.iter().zip(masters) {
        assert_eq!(start, private);
        assert_eq!(start[..5], master);
        assert_eq!(start[5..], neutral_rows);
        let expected_keys = if master == masters[3] {
            vec![(
                "(RGB) White Output Level".to_owned(),
                "254016000000,255,0,0,0,0,0,0;431827200000,100,4,0,0,0,0,0;635040000000,200,0,0,0,0,0,0;".to_owned(),
            )]
        } else {
            Vec::new()
        };
        assert_eq!(keys, &expected_keys);
    }

    // Reimport restores the edit.
    let omissions = premiere_to_tesseract(&exported, root.join("reimported"), None, false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let reimported = TesseractFile::open(first_project(&root.join("reimported")))
        .unwrap()
        .project_json()
        .unwrap();
    assert_eq!(
        effect_stacks(&reimported),
        levels_fixture_stacks(10.0, 6000, 0.5)
    );
    assert_eq!(
        corner_tracks(&reimported),
        levels_fixture_tracks((1200, 100.0))
    );
}

#[test]
fn adobe_brightness_contrast_imports_with_order_bypass_and_keys() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("converted");
    let omissions = premiere_to_tesseract(
        fixture_path(BRIGHTNESS_CONTRAST_FIXTURE),
        &output,
        Some(BRIGHTNESS_CONTRAST_SEQUENCE),
        false,
    )
    .unwrap();
    // Only UI nodes and the linked master clip's `DefMappingID` are reported;
    // every Brightness & Contrast converts, E's bypassed one too.
    assert!(
        omissions.iter().all(|omission| {
            omission.reason.contains("ClipTrackItem/TrackItem/Node")
                || omission.reason.contains("DefMappingID")
        }),
        "{omissions:?}"
    );
    let document = TesseractFile::open(first_project(&output))
        .unwrap()
        .project_json()
        .unwrap();
    let effect = |id: u64, enabled: bool, brightness: f64, contrast: f64| {
        json!({"id": id, "enabled": enabled, "effect": {"type": "brightnessContrast",
            "brightness": brightness, "contrast": contrast}})
    };
    // Premiere applies a chain in descending `Index` (Oracle run C6): B's
    // -10/25 at Index 1 applies first, then its 37/-25 at Index 0. D's keyed
    // Brightness starts at its first key.
    assert_eq!(
        effect_stacks(&document),
        BTreeMap::from([
            (0, json!([effect(1, true, 37.0, -25.0)])),
            (
                2000,
                json!([effect(2, true, -10.0, 25.0), effect(3, true, 37.0, -25.0)])
            ),
            (4000, json!([effect(4, true, -40.0, 85.0)])),
            (6000, json!([effect(5, true, 0.0, 20.0)])),
            (8500, json!([effect(6, false, 37.0, -25.0)])),
        ])
    );
    // D's Brightness on its clock from source In 0.5 s: Linear keys, then the
    // Hold.
    let linear = || "linear".to_owned();
    assert_eq!(
        corner_tracks(&document),
        BTreeMap::from([(
            (6000, "brightness".to_owned()),
            vec![
                (500, 0.0, linear()),
                (1000, 60.0, linear()),
                (2000, 0.0, "hold".to_owned()),
            ]
        )])
    );
}

#[test]
fn edited_brightness_contrast_survives_export_and_reimport() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    premiere_to_tesseract(
        fixture_path(BRIGHTNESS_CONTRAST_FIXTURE),
        root.join("converted"),
        Some(BRIGHTNESS_CONTRAST_SEQUENCE),
        false,
    )
    .unwrap();
    let converted = TesseractFile::open(first_project(&root.join("converted"))).unwrap();
    let mut document = converted.project_json().unwrap();
    // Edit: A's Contrast -25 becomes -10, and D's Brightness key 60 at 1 s on
    // its clock becomes 45 at 1.2 s, still reached Linear and holding.
    let layers = document["composition"]["layers"].as_array_mut().unwrap();
    let a = layers
        .iter_mut()
        .find(|layer| {
            layer["type"] == "Video" && (*crate::test_support::layer_range(layer))["start"] == 0
        })
        .unwrap();
    let contrast = &mut a["effects"][0]["effect"]["contrast"];
    assert_eq!(*contrast, -25.0);
    *contrast = json!(-10.0);
    let [entry] = document["composition"]["dynamics"]["entries"]
        .as_array_mut()
        .unwrap()
        .as_mut_slice()
    else {
        panic!("expected D's Brightness track only");
    };
    let key = &mut entry["animator"]["keyframes"][1];
    assert_eq!(
        (&key["layerTime"], &key["value"]["value"]),
        (&json!(1000), &json!(60.0))
    );
    (key["layerTime"], key["value"]["value"]) = (json!(1200), json!(45.0));
    let edited = root.join("edited.tsrct");
    let mut builder =
        TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap()).unwrap();
    for (id, asset) in &converted.metadata().assets {
        let name = Path::new(&asset.path).file_name().unwrap();
        builder = builder
            .add_asset(
                id.as_str(),
                fixture_path(name.to_str().unwrap()),
                asset.kind,
            )
            .unwrap();
    }
    builder.write(&edited).unwrap();
    let omissions = tesseract_to_premiere(&edited, root.join("native"), false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let exported = root.join("native/project.prproj");

    // Reimport restores every edited value and key, and E stays bypassed.
    let omissions = premiere_to_tesseract(&exported, root.join("reimported"), None, false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let reimported = TesseractFile::open(first_project(&root.join("reimported")))
        .unwrap()
        .project_json()
        .unwrap();
    assert_eq!(effect_stacks(&reimported), effect_stacks(&document));
    assert_eq!(corner_tracks(&reimported), corner_tracks(&document));
}

/// The FX `levels` of an Invert with Blend With Original `blend`: neutral
/// inputs and Gamma, output white 255 * blend / 100 and output black its
/// complement.
fn invert_levels(id: u64, blend: f64) -> Value {
    let output_white = blend * 51.0 / 20.0;
    fx_levels(id, [0.0, 255.0, 1.0, 255.0 - output_white, output_white])
}

/// The Invert stacks of the fixture, with B's Blend `b_blend`.
fn invert_fixture_stacks(b_blend: f64) -> BTreeMap<i64, Value> {
    BTreeMap::from([
        (0, json!([invert_levels(1, 0.0)])),
        (3000, json!([invert_levels(2, b_blend)])),
        (5000, json!([invert_levels(3, 100.0)])),
        (8000, json!([invert_levels(4, 0.0)])),
    ])
}

/// C's complementary output tracks, by its clip start, with the second key
/// at `second` (layer time ms, Blend percent): Linear from a Blend of 100,
/// Linear to 20, then a Hold to 100.
fn invert_fixture_tracks(second: (i64, f64)) -> CornerTracks {
    let (millis, blend) = second;
    let white = blend * 51.0 / 20.0;
    let keys = |second: f64, held: f64, other: f64| {
        vec![
            (500, other, "linear".to_owned()),
            (millis, second, "linear".to_owned()),
            (2000, held, "linear".to_owned()),
            (2500, other, "hold".to_owned()),
        ]
    };
    BTreeMap::from([
        ((5000, "outputWhite".to_owned()), keys(white, 51.0, 255.0)),
        (
            (5000, "outputBlack".to_owned()),
            keys(255.0 - white, 204.0, 0.0),
        ),
    ])
}

#[test]
fn adobe_inverts_import_as_complementary_levels() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("converted");
    let omissions = premiere_to_tesseract(
        fixture_path(INVERT_FIXTURE),
        &output,
        Some(INVERT_SEQUENCE),
        false,
    )
    .unwrap();
    // Only UI nodes and the linked master clip's `DefMappingID` are reported;
    // every Invert converts, its opaque private data ignored.
    assert!(
        omissions.iter().all(|omission| {
            omission.reason.contains("ClipTrackItem/TrackItem/Node")
                || omission.reason.contains("DefMappingID")
        }),
        "{omissions:?}"
    );
    let document = TesseractFile::open(first_project(&output))
        .unwrap()
        .project_json()
        .unwrap();
    // A and D invert fully, B keeps 30 percent of the original, and C's keyed
    // Blend starts at its first key, 100.
    assert_eq!(effect_stacks(&document), invert_fixture_stacks(30.0));
    // C's Blend keys on its clock from source In 1 s animate both outputs.
    assert_eq!(
        corner_tracks(&document),
        invert_fixture_tracks((1000, 20.0))
    );
}

#[test]
fn edited_inverts_survive_export_and_reimport() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    premiere_to_tesseract(
        fixture_path(INVERT_FIXTURE),
        root.join("converted"),
        Some(INVERT_SEQUENCE),
        false,
    )
    .unwrap();
    let converted = TesseractFile::open(first_project(&root.join("converted"))).unwrap();
    let mut document = converted.project_json().unwrap();
    // The gate's edit: B's Blend 30 becomes 60 (output white 76.5 to 153,
    // output black 178.5 to 102), and C's second key, 20 at 1 s on its clock,
    // becomes 40 at 1.2 s (output white 102, output black 153) on both tracks.
    let b = document["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|layer| {
            layer["type"] == "Video" && (*crate::test_support::layer_range(layer))["start"] == 3000
        })
        .unwrap();
    let outputs = &mut b["effects"][0]["effect"];
    assert_eq!(
        (&outputs["outputBlack"], &outputs["outputWhite"]),
        (&json!(178.5), &json!(76.5))
    );
    (outputs["outputBlack"], outputs["outputWhite"]) = (json!(102.0), json!(153.0));
    let entries = document["composition"]["dynamics"]["entries"]
        .as_array_mut()
        .unwrap();
    assert_eq!(entries.len(), 2, "expected C's two output tracks");
    for entry in entries {
        let value = match entry["target"]["paramName"].as_str().unwrap() {
            "outputWhite" => 102.0,
            "outputBlack" => 153.0,
            other => panic!("unexpected track {other}"),
        };
        let key = &mut entry["animator"]["keyframes"][1];
        assert_eq!(key["layerTime"], json!(1000));
        (key["layerTime"], key["value"]["value"]) = (json!(1200), json!(value));
    }
    let edited = root.join("edited.tsrct");
    let mut builder =
        TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap()).unwrap();
    for (id, asset) in &converted.metadata().assets {
        let name = Path::new(&asset.path).file_name().unwrap();
        builder = builder
            .add_asset(
                id.as_str(),
                fixture_path(name.to_str().unwrap()),
                asset.kind,
            )
            .unwrap();
    }
    builder.write(&edited).unwrap();
    let omissions = tesseract_to_premiere(&edited, root.join("native"), false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let exported = root.join("native/project.prproj");
    let xml = read_xml(&exported);
    // Every Levels in Invert's form writes back as an Invert, none as Levels.
    assert_eq!(
        xml.matches("<MatchName>AE.ADBE Invert</MatchName>").count(),
        4
    );
    assert_eq!(
        xml.matches("<MatchName>PR.ADBE Levels</MatchName>").count(),
        0
    );
    // C's Blend keys on the source clock from In 1 s: the moved key at
    // source 2.2 s, the Hold out of the 3 s key.
    assert_eq!(
        xml.matches("<Keyframes>381024000000,100,0,0,0,0,0,0;558835200000,40,0,0,0,0,0,0;762048000000,20,4,0,0,0,0,0;889056000000,100,0,0,0,0,0,0;</Keyframes>").count(),
        1
    );

    // Reimport restores the edit.
    let omissions = premiere_to_tesseract(&exported, root.join("reimported"), None, false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let reimported = TesseractFile::open(first_project(&root.join("reimported")))
        .unwrap()
        .project_json()
        .unwrap();
    assert_eq!(effect_stacks(&reimported), invert_fixture_stacks(60.0));
    assert_eq!(
        corner_tracks(&reimported),
        invert_fixture_tracks((1200, 40.0))
    );
}
/// An 8-bit channel as its FX share of 255, read back as the converted
/// document is: `serde_json` parses a float to within one ulp unless its
/// `float_roundtrip` feature is on, so the expectation takes the same path.
fn share(channel: u8) -> f64 {
    serde_json::from_str(&(f64::from(channel) / 255.0).to_string()).unwrap()
}

/// An imported, enabled `tintTritone` with 8-bit colours as shares of 255.
fn fx_tint(id: u64, (black, white, amount): ([u8; 3], [u8; 3], f64)) -> Value {
    json!({"id": id, "enabled": true, "effect": {"type": "tintTritone",
        "blackR": share(black[0]), "blackG": share(black[1]), "blackB": share(black[2]),
        "whiteR": share(white[0]), "whiteG": share(white[1]), "whiteB": share(white[2]),
        "amount": amount}})
}

/// The Tint stacks of the fixture, with B's Map White To `b_white` and C's
/// Amount `c_amount`. D's keyed Amount starts at its first key, 0, and E's
/// keyed Map White To at its first key, white.
fn tint_fixture_stacks(b_white: [u8; 3], c_amount: f64) -> BTreeMap<i64, Value> {
    let (black, white, orange) = ([0, 0, 0], [255, 255, 255], [255, 128, 0]);
    BTreeMap::from([
        (0, json!([fx_tint(1, (black, white, 100.0))])),
        (2000, json!([fx_tint(2, ([163, 247, 143], b_white, 100.0))])),
        (4000, json!([fx_tint(3, (black, orange, c_amount))])),
        (6000, json!([fx_tint(4, (black, orange, 0.0))])),
        (8500, json!([fx_tint(5, (black, white, 100.0))])),
    ])
}

/// The tracks of the fixture: D's Amount on its clock from source In 0.5 s
/// (Linear from 0, then `d_second` (layer time ms, Amount) held until 50 at
/// 2 s) and E's three Map White To channel tracks (Linear from white to
/// `e_second` (layer time ms, colour)).
fn tint_fixture_tracks(d_second: (i64, f64), e_second: (i64, [u8; 3])) -> CornerTracks {
    let linear = "linear".to_owned();
    let mut tracks = BTreeMap::from([(
        (6000, "amount".to_owned()),
        vec![
            (500, 0.0, linear.clone()),
            (d_second.0, d_second.1, linear.clone()),
            (2000, 50.0, "hold".to_owned()),
        ],
    )]);
    for (channel, name) in ["whiteR", "whiteG", "whiteB"].into_iter().enumerate() {
        tracks.insert(
            (8500, name.to_owned()),
            vec![
                (500, 1.0, linear.clone()),
                (e_second.0, share(e_second.1[channel]), linear.clone()),
            ],
        );
    }
    tracks
}

/// Only UI nodes and the linked master clip's `DefMappingID` are reported.
fn assert_only_node_omissions(omissions: &[premiere_file::Omission]) {
    assert!(
        omissions.iter().all(|omission| {
            omission.reason.contains("ClipTrackItem/TrackItem/Node")
                || omission.reason.contains("DefMappingID")
        }),
        "{omissions:?}"
    );
}

#[test]
fn adobe_tints_import_as_editable_tint_tritones() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("converted");
    let omissions = premiere_to_tesseract(
        fixture_path(TINT_FIXTURE),
        &output,
        Some(TINT_SEQUENCE),
        false,
    )
    .unwrap();
    assert_only_node_omissions(&omissions);
    let document = TesseractFile::open(first_project(&output))
        .unwrap()
        .project_json()
        .unwrap();
    // Every Tint converts with all seven values; the alpha of a colour (0 on
    // the defaults, opaque elsewhere) is ignored.
    assert_eq!(
        effect_stacks(&document),
        tint_fixture_stacks([240, 242, 22], 50.0)
    );
    assert_eq!(
        corner_tracks(&document),
        tint_fixture_tracks((1000, 100.0), (1000, [0, 128, 255]))
    );
}

/// A native Tint record: its Bypass, and per parameter its `StartKeyframe`
/// value and `Keyframes`.
type NativeTint = (String, Vec<(String, Option<String>)>);

/// The Tint records of a native project in chain document order.
fn native_tints(project: &Path) -> Vec<NativeTint> {
    let xml = read_xml(project);
    let document = roxmltree::Document::parse(&xml).unwrap();
    let root = document.root_element();
    let record = |id: &str| {
        root.children()
            .find(|node| node.attribute("ObjectID") == Some(id))
            .unwrap()
    };
    let child_text = |node: roxmltree::Node<'_, '_>, tag: &str| {
        node.children()
            .find(|child| child.has_tag_name(tag))
            .and_then(|child| child.text())
            .map(str::to_owned)
    };
    root.children()
        .filter(|node| {
            node.has_tag_name("VideoFilterComponent")
                && child_text(*node, "MatchName").as_deref() == Some("AE.ADBE Tint")
        })
        .map(|filter| {
            let body = filter.first_element_child().unwrap();
            let params = body
                .children()
                .find(|node| node.has_tag_name("Params"))
                .unwrap()
                .children()
                .filter(|node| node.has_tag_name("Param"))
                .map(|reference| {
                    let param = record(reference.attribute("ObjectRef").unwrap());
                    let start = child_text(param, "StartKeyframe").unwrap();
                    let value = start.split(',').nth(1).unwrap().to_owned();
                    (value, child_text(param, "Keyframes"))
                })
                .collect();
            (child_text(body, "Bypass").unwrap(), params)
        })
        .collect()
}

#[test]
fn edited_tints_survive_export_and_reimport() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    premiere_to_tesseract(
        fixture_path(TINT_FIXTURE),
        root.join("converted"),
        Some(TINT_SEQUENCE),
        false,
    )
    .unwrap();
    let converted = TesseractFile::open(first_project(&root.join("converted"))).unwrap();
    let mut document = converted.project_json().unwrap();
    // The gate's edits: B's Map White To (240, 242, 22) becomes (0, 64, 255);
    // C's Amount 50 becomes 25; D's second Amount key, 100 at 1 s on its
    // clock, becomes 80 at 1.2 s; E's second Map White To key, (0, 128, 255)
    // at 1 s, becomes (255, 0, 128) at 1.2 s on all three channel tracks.
    for layer in document["composition"]["layers"].as_array_mut().unwrap() {
        if layer["type"] != "Video" {
            continue;
        }
        let start = (*crate::test_support::layer_range(layer))["start"]
            .as_i64()
            .unwrap();
        let effect = &mut layer["effects"][0]["effect"];
        match start {
            2000 => {
                assert_eq!(effect["whiteR"], json!(share(240)));
                (effect["whiteR"], effect["whiteG"], effect["whiteB"]) =
                    (json!(0.0), json!(share(64)), json!(1.0));
            }
            4000 => {
                assert_eq!(effect["amount"], json!(50.0));
                effect["amount"] = json!(25.0);
            }
            _ => {}
        }
    }
    let entries = document["composition"]["dynamics"]["entries"]
        .as_array_mut()
        .unwrap();
    assert_eq!(
        entries.len(),
        4,
        "expected D's Amount and E's three channels"
    );
    for entry in entries {
        let value = match entry["target"]["paramName"].as_str().unwrap() {
            "amount" => 80.0,
            "whiteR" => 1.0,
            "whiteG" => 0.0,
            "whiteB" => share(128),
            other => panic!("unexpected track {other}"),
        };
        let key = &mut entry["animator"]["keyframes"][1];
        assert_eq!(key["layerTime"], json!(1000));
        (key["layerTime"], key["value"]["value"]) = (json!(1200), json!(value));
    }
    let edited = root.join("edited.tsrct");
    let mut builder =
        TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap()).unwrap();
    for (id, asset) in &converted.metadata().assets {
        let name = Path::new(&asset.path).file_name().unwrap();
        builder = builder
            .add_asset(
                id.as_str(),
                fixture_path(name.to_str().unwrap()),
                asset.kind,
            )
            .unwrap();
    }
    builder.write(&edited).unwrap();
    let omissions = tesseract_to_premiere(&edited, root.join("native"), false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let exported = root.join("native/project.prproj");
    // Opaque native colours (alpha 0xff00, the 8-bit value in each channel's
    // high byte); keys on the source clock, D's from In 0.5 s: the moved key
    // at source 1.7 s, the Hold out of the 1.5 s key. E's colour keys have
    // zero handles.
    let none: Option<String> = None;
    let opaque =
        |rgb: [u64; 3]| (0xff00_u64 << 48 | rgb[0] << 40 | rgb[1] << 24 | rgb[2] << 8).to_string();
    assert_eq!(
        native_tints(&exported),
        [
            ("false".to_owned(), vec![(opaque([0, 0, 0]), none.clone()), (opaque([255, 255, 255]), none.clone()), ("100.".to_owned(), none.clone())]),
            ("false".to_owned(), vec![(opaque([163, 247, 143]), none.clone()), (opaque([0, 64, 255]), none.clone()), ("100.".to_owned(), none.clone())]),
            ("false".to_owned(), vec![(opaque([0, 0, 0]), none.clone()), (opaque([255, 128, 0]), none.clone()), ("25.".to_owned(), none.clone())]),
            ("false".to_owned(), vec![(opaque([0, 0, 0]), none.clone()), (opaque([255, 128, 0]), none.clone()), ("0.".to_owned(), Some("254016000000,0,0,0,0,0,0,0;431827200000,80,4,0,0,0,0,0;635040000000,50,0,0,0,0,0,0;".to_owned()))]),
            ("false".to_owned(), vec![(opaque([0, 0, 0]), none.clone()), (opaque([255, 255, 255]), Some(format!("127008000000,{},0,0,0,0,0,0;304819200000,{},0,0,0,0,0,0;", opaque([255, 255, 255]), opaque([255, 0, 128])))), ("100.".to_owned(), none)]),
        ]
    );
    // Reimport restores the edits.
    let omissions = premiere_to_tesseract(&exported, root.join("reimported"), None, false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let reimported = TesseractFile::open(first_project(&root.join("reimported")))
        .unwrap()
        .project_json()
        .unwrap();
    assert_eq!(
        effect_stacks(&reimported),
        tint_fixture_stacks([0, 64, 255], 25.0)
    );
    assert_eq!(
        corner_tracks(&reimported),
        tint_fixture_tracks((1200, 80.0), (1200, [255, 0, 128]))
    );
}

/// An imported, enabled `gradientRamp` of a linear ramp: frame-UV points,
/// 8-bit colours as shares of 255, `blend` one minus Blend With Original.
fn fx_ramp(
    id: u64,
    (start, end): ([f64; 2], [f64; 2]),
    (start_colour, end_colour): ([u8; 3], [u8; 3]),
    blend: f64,
) -> Value {
    json!({"id": id, "enabled": true, "effect": {"type": "gradientRamp",
        "startX": start[0], "startY": start[1], "endX": end[0], "endY": end[1],
        "startR": share(start_colour[0]), "startG": share(start_colour[1]), "startB": share(start_colour[2]),
        "endR": share(end_colour[0]), "endG": share(end_colour[1]), "endB": share(end_colour[2]),
        "blend": blend, "shape": 0.0}})
}

/// The Ramp stacks of the run E10 fixture, with B's End Color `b_end` and
/// its FX blend `b_blend`, and E's start `e_start`. C's keyed Blend starts
/// at its first key, Blend With Original 1 (FX 0); D's keyed End of Ramp at
/// its first key, 0.5:1.
fn ramp_fixture_stacks(b_end: [u8; 3], b_blend: f64, e_start: [f64; 2]) -> BTreeMap<i64, Value> {
    let (black, white, vertical) = ([0, 0, 0], [255, 255, 255], ([0.5, 0.0], [0.5, 1.0]));
    BTreeMap::from([
        (0, json!([fx_ramp(1, vertical, (black, white), 1.0)])),
        (
            2000,
            json!([fx_ramp(
                2,
                ([0.2, 0.5], [0.8, 0.5]),
                ([200, 40, 40], b_end),
                b_blend
            )]),
        ),
        (4000, json!([fx_ramp(3, vertical, (black, white), 0.0)])),
        (6500, json!([fx_ramp(4, vertical, (black, white), 1.0)])),
        (
            9000,
            json!([fx_ramp(
                5,
                (e_start, [0.5, 0.1]),
                ([255, 255, 0], [0, 0, 255]),
                1.0
            )]),
        ),
    ])
}

/// The tracks of the fixture: C's blend on its clock from source In 0.5 s
/// (FX 0 at the first key, `c_second` (layer time ms, FX blend) held until
/// 0.5 at 2 s) and D's End of Ramp as an x track that stays 0.5 and a y track
/// Linear from 1 to `d_second` (layer time ms, y).
fn ramp_fixture_tracks(c_second: (i64, f64), d_second: (i64, f64)) -> CornerTracks {
    let linear = "linear".to_owned();
    BTreeMap::from([
        (
            (4000, "blend".to_owned()),
            vec![
                (500, 0.0, linear.clone()),
                (c_second.0, c_second.1, linear.clone()),
                (2000, 0.5, "hold".to_owned()),
            ],
        ),
        (
            (6500, "endX".to_owned()),
            vec![
                (500, 0.5, linear.clone()),
                (d_second.0, 0.5, linear.clone()),
            ],
        ),
        (
            (6500, "endY".to_owned()),
            vec![(500, 1.0, linear.clone()), (d_second.0, d_second.1, linear)],
        ),
    ])
}

#[test]
fn adobe_ramps_import_as_editable_gradient_ramps() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("converted");
    let omissions = premiere_to_tesseract(
        fixture_path(RAMP_FIXTURE),
        &output,
        Some(RAMP_SEQUENCE),
        false,
    )
    .unwrap();
    assert_only_node_omissions(&omissions);
    let document = TesseractFile::open(first_project(&output))
        .unwrap()
        .project_json()
        .unwrap();
    // Every Ramp converts with all twelve values: the fixture's ramps are
    // axis-aligned on canvas-size clips at default Motion. B's Blend With
    // Original 0.3 (a float32 through the MCP) is FX blend 1 − 0.300000011921.
    assert_eq!(
        effect_stacks(&document),
        ramp_fixture_stacks([40, 40, 200], 1.0 - 0.300000011921, [0.5, 0.9])
    );
    assert_eq!(
        corner_tracks(&document),
        ramp_fixture_tracks((1000, 1.0), (1500, 0.6))
    );
}

/// A native Ramp record: its Bypass, and per parameter its `StartKeyframe`
/// value and `Keyframes`.
fn native_ramps(project: &Path) -> Vec<NativeTint> {
    let xml = read_xml(project);
    let document = roxmltree::Document::parse(&xml).unwrap();
    let text = |node: roxmltree::Node<'_, '_>, tag: &str| {
        node.children()
            .find(|child| child.has_tag_name(tag))
            .and_then(|child| child.text())
            .map(str::to_owned)
    };
    let mut ramps = Vec::new();
    for node in document.root_element().children() {
        if text(node, "MatchName").as_deref() != Some("AE.ADBE Ramp") {
            continue;
        }
        let body = node.first_element_child().unwrap();
        let params: Vec<_> = body
            .children()
            .find(|child| child.has_tag_name("Params"))
            .unwrap()
            .children()
            .filter(|param| param.is_element())
            .map(|param| {
                let id = param.attribute("ObjectRef").unwrap();
                let record = document
                    .root_element()
                    .children()
                    .find(|node| node.attribute("ObjectID") == Some(id))
                    .unwrap();
                let start = text(record, "StartKeyframe").unwrap();
                let value = start.split(',').nth(1).unwrap().to_owned();
                (value, text(record, "Keyframes"))
            })
            .collect();
        ramps.push((text(body, "Bypass").unwrap(), params));
    }
    ramps
}

#[test]
fn edited_ramps_survive_export_and_reimport() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    premiere_to_tesseract(
        fixture_path(RAMP_FIXTURE),
        root.join("converted"),
        Some(RAMP_SEQUENCE),
        false,
    )
    .unwrap();
    let converted = TesseractFile::open(first_project(&root.join("converted"))).unwrap();
    let mut document = converted.project_json().unwrap();
    // The gate's edits: B's End Color (40, 40, 200) becomes (0, 200, 0) and
    // its FX blend 0.7 becomes 0.5 (Blend With Original 0.5); C's second Blend
    // key, FX 1 at 1 s on its clock, becomes 0.75 at 1.2 s (Blend With
    // Original 0.25); D's second End of Ramp key, 0.5:0.6 at 1.5 s, becomes
    // 0.5:0.7 at 1.2 s on both coordinate tracks; E's start 0.5:0.9 becomes
    // 0.5:0.8.
    for layer in document["composition"]["layers"].as_array_mut().unwrap() {
        if layer["type"] != "Video" {
            continue;
        }
        let start = (*crate::test_support::layer_range(layer))["start"]
            .as_i64()
            .unwrap();
        let effect = &mut layer["effects"][0]["effect"];
        match start {
            2000 => {
                assert_eq!(effect["endB"], json!(share(200)));
                (effect["endR"], effect["endG"], effect["endB"]) =
                    (json!(0.0), json!(share(200)), json!(0.0));
                effect["blend"] = json!(0.5);
            }
            9000 => {
                assert_eq!(effect["startY"], json!(0.9));
                effect["startY"] = json!(0.8);
            }
            _ => {}
        }
    }
    let entries = document["composition"]["dynamics"]["entries"]
        .as_array_mut()
        .unwrap();
    assert_eq!(
        entries.len(),
        3,
        "expected C's blend and D's two coordinates"
    );
    for entry in entries {
        let value = match entry["target"]["paramName"].as_str().unwrap() {
            "blend" => 0.75,
            "endX" => 0.5,
            "endY" => 0.7,
            other => panic!("unexpected track {other}"),
        };
        let key = &mut entry["animator"]["keyframes"][1];
        (key["layerTime"], key["value"]["value"]) = (json!(1200), json!(value));
    }
    let edited = root.join("edited.tsrct");
    let mut builder =
        TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap()).unwrap();
    for (id, asset) in &converted.metadata().assets {
        let name = Path::new(&asset.path).file_name().unwrap();
        builder = builder
            .add_asset(
                id.as_str(),
                fixture_path(name.to_str().unwrap()),
                asset.kind,
            )
            .unwrap();
    }
    builder.write(&edited).unwrap();
    let omissions = tesseract_to_premiere(&edited, root.join("native"), false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let exported = root.join("native/project.prproj");
    // Opaque native colours; the linear Shape and Scatter 0; Blend With
    // Original 1 − blend; keys on the source clock: C's moved key at source
    // 1.7 s (its In is 0.5 s) with the Hold out of it, D's at 1.2 s (In 0)
    // with straight spatial fields.
    let none: Option<String> = None;
    let opaque =
        |rgb: [u64; 3]| (0xff00_u64 << 48 | rgb[0] << 40 | rgb[1] << 24 | rgb[2] << 8).to_string();
    let statics =
        |start: &str, start_colour: [u64; 3], end: &str, end_colour: [u64; 3], blend: &str| {
            vec![
                (start.to_owned(), none.clone()),
                (opaque(start_colour), none.clone()),
                (end.to_owned(), none.clone()),
                (opaque(end_colour), none.clone()),
                ("0".to_owned(), none.clone()),
                ("0.".to_owned(), none.clone()),
                (blend.to_owned(), none.clone()),
            ]
        };
    let mut c = statics("0.5:0", [0, 0, 0], "0.5:1", [255, 255, 255], "1.");
    c[6].1 = Some(
        "254016000000,1,0,0,0,0,0,0;431827200000,0.25,4,0,0,0,0,0;635040000000,0.5,0,0,0,0,0,0;"
            .to_owned(),
    );
    let mut d = statics("0.5:0", [0, 0, 0], "0.5:1", [255, 255, 255], "0.");
    d[2].1 = Some(
        "127008000000,0.5:1,0,0,0,0,0,0,0,0,0,0,0,0;304819200000,0.5:0.7,0,0,0,0,0,0,0,0,0,0,0,0;"
            .to_owned(),
    );
    assert_eq!(
        native_ramps(&exported),
        [
            (
                "false".to_owned(),
                statics("0.5:0", [0, 0, 0], "0.5:1", [255, 255, 255], "0.")
            ),
            (
                "false".to_owned(),
                statics("0.2:0.5", [200, 40, 40], "0.8:0.5", [0, 200, 0], "0.5")
            ),
            ("false".to_owned(), c),
            ("false".to_owned(), d),
            (
                "false".to_owned(),
                statics("0.5:0.8", [255, 255, 0], "0.5:0.1", [0, 0, 255], "0.")
            ),
        ]
    );
    // Reimport restores the edits.
    let omissions = premiere_to_tesseract(&exported, root.join("reimported"), None, false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let reimported = TesseractFile::open(first_project(&root.join("reimported")))
        .unwrap()
        .project_json()
        .unwrap();
    assert_eq!(
        effect_stacks(&reimported),
        ramp_fixture_stacks([0, 200, 0], 0.5, [0.5, 0.8])
    );
    assert_eq!(
        corner_tracks(&reimported),
        ramp_fixture_tracks((1200, 0.75), (1200, 0.7))
    );
}

/// An FX `mosaic` with Sharp Colors on.
fn fx_mosaic(id: u64, horizontal: u32, vertical: u32) -> Value {
    json!({"id": id, "enabled": true, "effect": {"type": "mosaic",
        "horizontalBlocks": f64::from(horizontal), "verticalBlocks": f64::from(vertical), "sharpColors": true}})
}

/// The Mosaic stacks of the run E8 fixture (`feature_mosaic_strict`): A 16 × 9,
/// B `b`, C the 10 × 10 defaults, D keyed from 10 × 10 (source In 0.5 s),
/// E 96 × 54, each with Sharp Colors on.
fn mosaic_fixture_stacks(b: (u32, u32)) -> BTreeMap<i64, Value> {
    BTreeMap::from([
        (0, json!([fx_mosaic(1, 16, 9)])),
        (2000, json!([fx_mosaic(2, b.0, b.1)])),
        (4000, json!([fx_mosaic(3, 10, 10)])),
        (6000, json!([fx_mosaic(4, 10, 10)])),
        (9000, json!([fx_mosaic(5, 96, 54)])),
    ])
}

/// D's count tracks: 10 at layer 500 ms (source 1 s), the `second` key, 20 at
/// 2000 ms (source 2.5 s), Hold into the second and third keys.
fn mosaic_fixture_tracks(second: (i64, (f64, f64))) -> CornerTracks {
    let track = |value: f64| {
        vec![
            (500, 10.0, "linear".to_owned()),
            (second.0, value, "hold".to_owned()),
            (2000, 20.0, "hold".to_owned()),
        ]
    };
    BTreeMap::from([
        ((6000, "horizontalBlocks".to_owned()), track(second.1 .0)),
        ((6000, "verticalBlocks".to_owned()), track(second.1 .1)),
    ])
}

#[test]
fn adobe_mosaics_import_as_editable_mosaics() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("converted");
    let omissions = premiere_to_tesseract(
        fixture_path(MOSAIC_FIXTURE),
        &output,
        Some(MOSAIC_SEQUENCE),
        false,
    )
    .unwrap();
    assert_only_node_omissions(&omissions);
    let document = TesseractFile::open(first_project(&output))
        .unwrap()
        .project_json()
        .unwrap();
    // Every Mosaic converts: Sharp Colors on, whole counts, Hold keys; the
    // Premiere 26.5.1 checkbox record has no `Name`.
    assert_eq!(effect_stacks(&document), mosaic_fixture_stacks((48, 27)));
    assert_eq!(
        corner_tracks(&document),
        mosaic_fixture_tracks((1000, (40.0, 30.0)))
    );
}

/// The Mosaic records of a native project in chain document order: Bypass,
/// and per parameter its `StartKeyframe` value and `Keyframes`; the checkbox
/// record's `Name` element, when present, is prefixed to its value.
fn native_mosaics(project: &Path) -> Vec<NativeTint> {
    let xml = read_xml(project);
    let document = roxmltree::Document::parse(&xml).unwrap();
    let text = |node: roxmltree::Node<'_, '_>, tag: &str| {
        node.children()
            .find(|child| child.has_tag_name(tag))
            .and_then(|child| child.text())
            .map(str::to_owned)
    };
    let mut mosaics = Vec::new();
    for node in document.root_element().children() {
        if text(node, "MatchName").as_deref() != Some("AE.ADBE Mosaic") {
            continue;
        }
        let body = node.first_element_child().unwrap();
        let params: Vec<_> = body
            .children()
            .find(|child| child.has_tag_name("Params"))
            .unwrap()
            .children()
            .filter(|param| param.is_element())
            .map(|param| {
                let id = param.attribute("ObjectRef").unwrap();
                let record = document
                    .root_element()
                    .children()
                    .find(|node| node.attribute("ObjectID") == Some(id))
                    .unwrap();
                let start = text(record, "StartKeyframe").unwrap();
                let value = start.split(',').nth(1).unwrap().to_owned();
                let value = match text(record, "Name") {
                    Some(name) if text(record, "ParameterID").as_deref() == Some("3") => {
                        format!("{name:?} {value}")
                    }
                    _ => value,
                };
                (value, text(record, "Keyframes"))
            })
            .collect();
        mosaics.push((text(body, "Bypass").unwrap(), params));
    }
    mosaics
}

#[test]
fn edited_mosaics_survive_export_and_reimport() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    premiere_to_tesseract(
        fixture_path(MOSAIC_FIXTURE),
        root.join("converted"),
        Some(MOSAIC_SEQUENCE),
        false,
    )
    .unwrap();
    let converted = TesseractFile::open(first_project(&root.join("converted"))).unwrap();
    let mut document = converted.project_json().unwrap();
    // The gate's edits: B 48 × 27 becomes 32 × 18 and its layer is scaled to
    // 50 % and rotated 30° (the grid is a fraction of the clip frame at any
    // Motion); D's second key, 40 × 30 at 1 s on its clock (source
    // 1.5 s), becomes 36 × 24 at 1.2 s (source 1.7 s), still held.
    for layer in document["composition"]["layers"].as_array_mut().unwrap() {
        if layer["type"] != "Video" {
            continue;
        }
        if (*crate::test_support::layer_range(layer))["start"] == json!(2000) {
            let effect = &mut layer["effects"][0]["effect"];
            assert_eq!(effect["horizontalBlocks"], json!(48.0));
            (effect["horizontalBlocks"], effect["verticalBlocks"]) = (json!(32.0), json!(18.0));
            (layer["transform"]["scale"], layer["transform"]["rotation"]) =
                (json!([50.0, 50.0]), json!(30.0));
        }
    }
    let entries = document["composition"]["dynamics"]["entries"]
        .as_array_mut()
        .unwrap();
    assert_eq!(entries.len(), 2, "expected D's two count tracks");
    for entry in entries {
        let value = match entry["target"]["paramName"].as_str().unwrap() {
            "horizontalBlocks" => 36.0,
            "verticalBlocks" => 24.0,
            other => panic!("unexpected track {other}"),
        };
        let key = &mut entry["animator"]["keyframes"][1];
        assert_eq!(key["easing"]["type"], "hold");
        (key["layerTime"], key["value"]["value"]) = (json!(1200), json!(value));
    }
    let edited = root.join("edited.tsrct");
    let mut builder =
        TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap()).unwrap();
    for (id, asset) in &converted.metadata().assets {
        let name = Path::new(&asset.path).file_name().unwrap();
        builder = builder
            .add_asset(
                id.as_str(),
                fixture_path(name.to_str().unwrap()),
                asset.kind,
            )
            .unwrap();
    }
    builder.write(&edited).unwrap();
    let omissions = tesseract_to_premiere(&edited, root.join("native"), false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let exported = root.join("native/project.prproj");
    // Whole counts; the checkbox written `true` without a `Name`; D's keys on
    // the source clock with mode 4 (Hold) out of the first two keys and the
    // moved key at 1.7 s.
    let none: Option<String> = None;
    let statics = |horizontal: &str, vertical: &str| {
        vec![
            (horizontal.to_owned(), none.clone()),
            (vertical.to_owned(), none.clone()),
            ("true".to_owned(), none.clone()),
        ]
    };
    let keys = |second: &str| {
        Some(format!(
            "254016000000,10,4,0,0,0,0,0;431827200000,{second},4,0,0,0,0,0;635040000000,20,0,0,0,0,0,0;"
        ))
    };
    let mut d = statics("10", "10");
    (d[0].1, d[1].1) = (keys("36"), keys("24"));
    assert_eq!(
        native_mosaics(&exported),
        [
            ("false".to_owned(), statics("16", "9")),
            ("false".to_owned(), statics("32", "18")),
            ("false".to_owned(), statics("10", "10")),
            ("false".to_owned(), d),
            ("false".to_owned(), statics("96", "54")),
        ]
    );
    // Reimport restores the edits.
    let omissions = premiere_to_tesseract(&exported, root.join("reimported"), None, false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let reimported = TesseractFile::open(first_project(&root.join("reimported")))
        .unwrap()
        .project_json()
        .unwrap();
    assert_eq!(effect_stacks(&reimported), mosaic_fixture_stacks((32, 18)));
    assert_eq!(
        corner_tracks(&reimported),
        mosaic_fixture_tracks((1200, (36.0, 24.0)))
    );
    let b = reimported["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| (*crate::test_support::layer_range(layer))["start"] == json!(2000))
        .unwrap();
    assert_eq!(b["transform"]["scale"], json!([50.0, 50.0]));
    assert_eq!(b["transform"]["rotation"], json!(30.0));
}

/// The stage groups of a converted document by start: the group's Motion
/// (scale, rotation) and its video's transform (anchor, position, scale,
/// rotation, skew, skew axis).
type Stages = BTreeMap<
    i64,
    (
        ([f64; 2], f64),
        ([f64; 2], [f64; 2], [f64; 2], f64, f64, f64),
    ),
>;

fn transform_stages(document: &Value) -> Stages {
    let pair = |value: &Value| [value[0].as_f64().unwrap(), value[1].as_f64().unwrap()];
    document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|layer| layer["type"] == "Group")
        .map(|group| {
            assert!(group.get("masks").is_none(), "{group}");
            let [video] = group["layers"].as_array().unwrap().as_slice() else {
                panic!("expected the video alone under {group}");
            };
            assert_eq!(video["type"], "Video");
            let (motion, t) = (&group["transform"], &video["transform"]);
            (
                (*crate::test_support::layer_range(group))["start"]
                    .as_i64()
                    .unwrap(),
                (
                    (pair(&motion["scale"]), motion["rotation"].as_f64().unwrap()),
                    (
                        pair(&t["anchorPoint"]),
                        pair(&t["position"]),
                        pair(&t["scale"]),
                        t["rotation"].as_f64().unwrap(),
                        t["skew"].as_f64().unwrap(),
                        t["skewAxis"].as_f64().unwrap(),
                    ),
                ),
            )
        })
        .collect()
}

/// The stages of the run E11 fixture: A at 0 s, B at 2 s, C at 4 s, F at
/// 5 s and D at 6 s (their statics the first keys') and E at 8.5 s, with B's
/// Rotation `b_rotation`, B's video scale `b_scale`, C's skew axis `c_axis`
/// and E's position `e_position`. A skewed clip's skew axis is its Skew Axis
/// less 90 (the gate-measured convention); the unskewed clips' is 0.
fn transform_fixture_stages(
    b_rotation: f64,
    b_scale: [f64; 2],
    c_axis: f64,
    e_position: [f64; 2],
) -> Stages {
    let centre = [960.0, 540.0];
    let still = ([100.0, 100.0], 0.0);
    BTreeMap::from([
        (
            0,
            (
                still,
                (centre, [1440.0, 540.0], [100.0, 100.0], 0.0, 0.0, 0.0),
            ),
        ),
        (
            5000,
            (
                still,
                (centre, [480.0, 540.0], [100.0, 100.0], 0.0, 0.0, 0.0),
            ),
        ),
        (
            2000,
            (
                still,
                ([1440.0, 540.0], centre, b_scale, b_rotation, 0.0, 0.0),
            ),
        ),
        (
            4000,
            (still, (centre, centre, [100.0, 100.0], 0.0, 30.0, c_axis)),
        ),
        (
            6000,
            (still, (centre, centre, [100.0, 100.0], 0.0, 0.0, 0.0)),
        ),
        (
            8500,
            (
                ([50.0, 50.0], 30.0),
                (centre, e_position, [150.0, 150.0], 0.0, 0.0, 0.0),
            ),
        ),
    ])
}

/// The layer-property tracks of a converted document, by the start of the
/// group that holds the keyed layer and the property.
fn layer_tracks(document: &Value) -> CornerTracks {
    let owners: BTreeMap<u64, i64> = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|layer| layer["type"] == "Group")
        .map(|group| {
            (
                group["layers"][0]["id"].as_u64().unwrap(),
                (*crate::test_support::layer_range(group))["start"]
                    .as_i64()
                    .unwrap(),
            )
        })
        .collect();
    document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            assert_eq!(entry["target"]["kind"], "layer", "{entry}");
            let keys = entry["animator"]["keyframes"]
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
                .collect();
            let owner = owners[&entry["target"]["layerId"].as_u64().unwrap()];
            let property = entry["target"]["propertyType"].as_str().unwrap().to_owned();
            ((owner, property), keys)
        })
        .collect()
}

/// D's tracks: Scale on both axes (Uniform Scale) 100 at layer 500 ms (source
/// 1 s), the `second` key, 50 at 2000 ms (source 2.5 s) held; Rotation 0 to
/// 90 arriving with `rotation_easing`. F's Position: 0.25 to 0.75 of the
/// frame over its first second.
fn transform_fixture_tracks(second: (i64, f64), rotation_easing: &str) -> CornerTracks {
    let scale = vec![
        (500, 100.0, "linear".to_owned()),
        (second.0, second.1, "linear".to_owned()),
        (2000, 50.0, "hold".to_owned()),
    ];
    let linear = |values: [f64; 2]| {
        vec![
            (0, values[0], "linear".to_owned()),
            (1000, values[1], "linear".to_owned()),
        ]
    };
    BTreeMap::from([
        ((5000, "positionX".to_owned()), linear([480.0, 1440.0])),
        ((5000, "positionY".to_owned()), linear([540.0, 540.0])),
        (
            (6000, "rotation".to_owned()),
            vec![
                (500, 0.0, "linear".to_owned()),
                (2000, 90.0, rotation_easing.to_owned()),
            ],
        ),
        ((6000, "scaleX".to_owned()), scale.clone()),
        ((6000, "scaleY".to_owned()), scale),
    ])
}

/// The approximations of the run E11 fixture, one warning each: A's
/// Transform Opacity 50 (E11 T6) and F's Shutter Angle 180 with the
/// composition's shutter angle off (T12), as (clip record, the start of the
/// warning, whose measured error the schema test pins).
const TRANSFORM_FIXTURE_WARNINGS: [(&str, &str); 2] = [
    (
        "VideoClipTrackItem:83",
        "Transform Opacity 50 blends in linear light in Premiere; converted as sRGB opacity",
    ),
    (
        "VideoClipTrackItem:89",
        "Transform motion blur (Shutter Angle 180) approximated by FX motion blur",
    ),
];

/// A's and F's approximated values: A's video at Opacity 50 (its
/// Transform's) in a group at Opacity 50 (the clip's), and F's video blurred
/// by the composition's one shutter at 180° with phase 0.
fn assert_approximated_fixture_stages(document: &Value) {
    let groups: BTreeMap<i64, &Value> = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|layer| layer["type"] == "Group")
        .map(|group| {
            (
                (*crate::test_support::layer_range(group))["start"]
                    .as_i64()
                    .unwrap(),
                group,
            )
        })
        .collect();
    let (a, f) = (groups[&0], groups[&5000]);
    assert_eq!(
        (
            &a["transform"]["opacity"],
            &a["layers"][0]["transform"]["opacity"]
        ),
        (&json!(50.0), &json!(50.0))
    );
    let blurred: Vec<_> = groups
        .iter()
        .filter(|(_, group)| group["layers"][0]["motionBlur"] == json!(true))
        .map(|(start, _)| *start)
        .collect();
    assert_eq!(blurred, [5000], "{f}");
    let settings = &document["composition"]["motionBlur"];
    assert_eq!(
        (
            &settings["enabled"],
            &settings["shutterAngle"],
            &settings["shutterPhase"]
        ),
        (&json!(true), &json!(180.0), &json!(0.0))
    );
}

#[test]
fn adobe_transforms_import_as_staged_video_transforms() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("converted");
    let omissions = premiere_to_tesseract(
        fixture_path(TRANSFORM_FIXTURE),
        &output,
        Some(TRANSFORM_SEQUENCE),
        false,
    )
    .unwrap();
    // The fixture's warnings by record, the reader's reports of UI nodes aside.
    let mut warnings: Vec<_> = omissions
        .iter()
        .filter(|omission| {
            !(omission.reason.contains("ClipTrackItem/TrackItem/Node")
                || omission.reason.contains("DefMappingID"))
        })
        .map(|omission| (omission.record.as_str(), omission.reason.as_str()))
        .collect();
    warnings.sort_unstable();
    assert_eq!(
        warnings.len(),
        TRANSFORM_FIXTURE_WARNINGS.len(),
        "{warnings:?}"
    );
    for ((record, reason), (fixture_record, start)) in
        warnings.into_iter().zip(TRANSFORM_FIXTURE_WARNINGS)
    {
        assert_eq!(record, fixture_record);
        assert!(reason.starts_with(start), "{reason}");
    }
    let document = TesseractFile::open(first_project(&output))
        .unwrap()
        .project_json()
        .unwrap();
    // Every clip stages: the video carries the Transform in source pixels
    // (E11 T7), Scale Height on both axes (T3), C's skew axis 90 less than
    // its Skew Axis 45 (the gate-measured convention; the unskewed clips
    // keep 0), under the clip's Motion (T10); D's and F's keys are the
    // video's tracks (T9); A's Opacity and F's motion blur approximate.
    assert_eq!(
        transform_stages(&document),
        transform_fixture_stages(30.0, [50.0, 50.0], -45.0, [1440.0, 540.0])
    );
    assert_eq!(
        layer_tracks(&document),
        transform_fixture_tracks((1000, 200.0), "linear")
    );
    assert_approximated_fixture_stages(&document);
    assert_eq!(effect_stacks(&document), BTreeMap::new());
    let flat = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|layer| layer["type"] == "Video" && layer["name"] != "Premiere video 1")
        .count();
    assert_eq!(flat, 0);
}

/// The Transform records of a native project in chain document order: per
/// parameter its `Name` (`-` for none), `StartKeyframe` value and `Keyframes`.
fn native_transforms(project: &Path) -> Vec<Vec<(String, String, Option<String>)>> {
    let xml = read_xml(project);
    let document = roxmltree::Document::parse(&xml).unwrap();
    let text = |node: roxmltree::Node<'_, '_>, tag: &str| {
        node.children()
            .find(|child| child.has_tag_name(tag))
            .and_then(|child| child.text())
            .map(str::to_owned)
    };
    let mut transforms = Vec::new();
    for node in document.root_element().children() {
        if text(node, "MatchName").as_deref() != Some("AE.ADBE Geometry") {
            continue;
        }
        let body = node.first_element_child().unwrap();
        assert_eq!(text(body, "Bypass").as_deref(), Some("false"));
        let params: Vec<_> = body
            .children()
            .find(|child| child.has_tag_name("Params"))
            .unwrap()
            .children()
            .filter(|param| param.is_element())
            .map(|param| {
                let id = param.attribute("ObjectRef").unwrap();
                let record = document
                    .root_element()
                    .children()
                    .find(|node| node.attribute("ObjectID") == Some(id))
                    .unwrap();
                let start = text(record, "StartKeyframe").unwrap();
                (
                    text(record, "Name").unwrap_or_else(|| "-".to_owned()),
                    start.split(',').nth(1).unwrap().to_owned(),
                    text(record, "Keyframes"),
                )
            })
            .collect();
        transforms.push(params);
    }
    transforms
}

#[test]
fn edited_transforms_survive_export_and_reimport() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    premiere_to_tesseract(
        fixture_path(TRANSFORM_FIXTURE),
        root.join("converted"),
        Some(TRANSFORM_SEQUENCE),
        false,
    )
    .unwrap();
    let converted = TesseractFile::open(first_project(&root.join("converted"))).unwrap();
    let mut document = converted.project_json().unwrap();
    // The gate's edits on the staged videos: B Rotation 30 to 45 and its
    // scale 50/50 to 70/50 (Uniform Scale off, Scale Width 70); C's skew
    // axis −45 to −120 (Skew Axis −30, the vector the gate measured); E's
    // Position 0.75 to 0.6 of the frame.
    for group in document["composition"]["layers"].as_array_mut().unwrap() {
        if group["type"] != "Group" {
            continue;
        }
        let start = (*crate::test_support::layer_range(group))["start"]
            .as_i64()
            .unwrap();
        let transform = &mut group["layers"][0]["transform"];
        match start {
            2000 => {
                (transform["rotation"], transform["scale"]) = (json!(45.0), json!([70.0, 50.0]));
            }
            4000 => transform["skewAxis"] = json!(-120.0),
            8500 => transform["position"] = json!([1152.0, 540.0]),
            _ => {}
        }
    }
    // D's second Scale key, 200 at 1 s on its clock (source 1.5 s), becomes
    // 180 at 1.2 s (source 1.7 s), still starting the Hold; D's Rotation
    // arrives at 90 on a Bézier. A Bézier into the Scale key is no
    // option: Premiere ignores the in-handle of a key that starts a Hold.
    let d_video = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| {
            layer["type"] == "Group" && (*crate::test_support::layer_range(layer))["start"] == 6000
        })
        .map(|group| group["layers"][0]["id"].clone())
        .unwrap();
    let entries = document["composition"]["dynamics"]["entries"]
        .as_array_mut()
        .unwrap();
    assert_eq!(entries.len(), 5, "expected D's three tracks and F's two");
    for entry in entries
        .iter_mut()
        .filter(|entry| entry["target"]["layerId"] == d_video)
    {
        let rotation = entry["target"]["propertyType"] == "rotation";
        let key = &mut entry["animator"]["keyframes"][1];
        if rotation {
            assert_eq!(key["value"]["value"], json!(90.0));
            key["easing"] =
                json!({"type": "cubicBezier", "x1": 0.4, "y1": 0.0, "x2": 0.6, "y2": 1.0});
            continue;
        }
        assert_eq!(key["value"]["value"], json!(200.0));
        key["layerTime"] = json!(1200);
        key["value"]["value"] = json!(180.0);
    }
    let edited = root.join("edited.tsrct");
    let mut builder =
        TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap()).unwrap();
    for (id, asset) in &converted.metadata().assets {
        let name = Path::new(&asset.path).file_name().unwrap();
        builder = builder
            .add_asset(
                id.as_str(),
                fixture_path(name.to_str().unwrap()),
                asset.kind,
            )
            .unwrap();
    }
    builder.write(&edited).unwrap();
    // Export reports A's and F's approximations as import does.
    let omissions = tesseract_to_premiere(&edited, root.join("native"), false).unwrap();
    let mut reasons: Vec<_> = omissions
        .iter()
        .map(|omission| omission.reason.as_str())
        .collect();
    reasons.sort_unstable();
    assert_eq!(
        reasons.len(),
        TRANSFORM_FIXTURE_WARNINGS.len(),
        "{omissions:?}"
    );
    for (reason, (_, start)) in reasons.into_iter().zip(TRANSFORM_FIXTURE_WARNINGS) {
        assert!(reason.starts_with(start), "{reason}");
    }
    let exported = root.join("native/project.prproj");
    // One Transform per staged clip in the AE-family form: the checkboxes
    // without a `Name`, bilinear sampling, Opacity 100 and the composition's
    // shutter angle but on A (Opacity 50) and F (Shutter Angle 180, its
    // Position keys); B's Uniform Scale off with Scale Width 70; D's keys on
    // the source clock with the moved key and Hold (mode 4) out of it, and
    // the Rotation's Bézier arrival as mode 5 out of the first key.
    let statics = |anchor: &str,
                   position: &str,
                   uniform: &str,
                   height: &str,
                   width: &str,
                   skew: &str,
                   axis: &str,
                   rotation: &str| {
        vec![
            ("Anchor Point".to_owned(), anchor.to_owned(), None),
            ("Position".to_owned(), position.to_owned(), None),
            ("-".to_owned(), uniform.to_owned(), None),
            ("Scale Height".to_owned(), height.to_owned(), None),
            ("Scale Width".to_owned(), width.to_owned(), None),
            ("Skew".to_owned(), skew.to_owned(), None),
            ("Skew Axis".to_owned(), axis.to_owned(), None),
            ("Rotation".to_owned(), rotation.to_owned(), None),
            ("Opacity".to_owned(), "100.".to_owned(), None),
            ("-".to_owned(), "true".to_owned(), None),
            ("Shutter Angle".to_owned(), "0.".to_owned(), None),
            ("Sampling".to_owned(), "0".to_owned(), None),
        ]
    };
    let mut d = statics(
        "0.5:0.5", "0.5:0.5", "true", "100.", "100.", "0.", "0.", "0.",
    );
    d[3].2 = Some(
        "254016000000,100,0,0,0,0,0,0;431827200000,180,4,0,0,0,0,0;635040000000,50,0,0,0,0,0,0;"
            .to_owned(),
    );
    let mut a = statics(
        "0.5:0.5", "0.75:0.5", "true", "100.", "100.", "0.", "0.", "0.",
    );
    a[8].1 = "50.".to_owned();
    let mut f = statics(
        "0.5:0.5", "0.25:0.5", "true", "100.", "100.", "0.", "0.", "0.",
    );
    f[1].2 = Some(
        "0,0.25:0.5,0,0,0,0,0,0,0,0,0,0,0,0;254016000000,0.75:0.5,0,0,0,0,0,0,0,0,0,0,0,0;"
            .to_owned(),
    );
    (f[9].1, f[10].1) = ("false".to_owned(), "180.".to_owned());
    let native = native_transforms(&exported);
    assert_eq!(native.len(), 6, "{native:?}");
    assert_eq!(native[0], a);
    assert_eq!(
        native[1],
        statics("0.75:0.5", "0.5:0.5", "false", "50.", "70.", "0.", "0.", "45.")
    );
    assert_eq!(
        native[2],
        statics("0.5:0.5", "0.5:0.5", "true", "100.", "100.", "30.", "-30.", "0.")
    );
    assert_eq!(native[3], f);
    assert_eq!(native[4][..7], d[..7]);
    assert_eq!(native[4][8..], d[8..]);
    let (name, start, keys) = &native[4][7];
    let modes: Vec<_> = keys
        .as_deref()
        .unwrap()
        .split_terminator(';')
        .map(|key| key.split(',').take(3).collect::<Vec<_>>().join(","))
        .collect();
    assert_eq!(
        (name.as_str(), start.as_str(), modes),
        (
            "Rotation",
            "0.",
            vec![
                "254016000000,0,5".to_owned(),
                "635040000000,90,0".to_owned()
            ]
        )
    );
    assert_eq!(
        native[5],
        statics("0.5:0.5", "0.6:0.5", "true", "150.", "150.", "0.", "0.", "0.")
    );
    // Reimport restores the edits: B non-uniform 70/50, C's axis, E's
    // position, D's moved Bézier key; A's and F's approximations round-trip
    // with their warnings.
    let omissions = premiere_to_tesseract(&exported, root.join("reimported"), None, false).unwrap();
    let mut reasons: Vec<_> = omissions
        .iter()
        .map(|omission| omission.reason.as_str())
        .collect();
    reasons.sort_unstable();
    assert_eq!(
        reasons.len(),
        TRANSFORM_FIXTURE_WARNINGS.len(),
        "{omissions:?}"
    );
    for (reason, (_, start)) in reasons.into_iter().zip(TRANSFORM_FIXTURE_WARNINGS) {
        assert!(reason.starts_with(start), "{reason}");
    }
    let reimported = TesseractFile::open(first_project(&root.join("reimported")))
        .unwrap()
        .project_json()
        .unwrap();
    assert_eq!(
        transform_stages(&reimported),
        transform_fixture_stages(45.0, [70.0, 50.0], -120.0, [1152.0, 540.0])
    );
    assert_eq!(
        layer_tracks(&reimported),
        transform_fixture_tracks((1200, 180.0), "cubicBezier")
    );
    assert_approximated_fixture_stages(&reimported);
}

#[test]
fn adobe_black_whites_import_as_default_tint_tritones_and_export_as_tints() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let omissions = premiere_to_tesseract(
        fixture_path(BLACK_WHITE_FIXTURE),
        root.join("converted"),
        Some(BLACK_WHITE_SEQUENCE),
        false,
    )
    .unwrap();
    assert_only_node_omissions(&omissions);
    let converted = TesseractFile::open(first_project(&root.join("converted"))).unwrap();
    let document = converted.project_json().unwrap();
    // Each Black & White is Tint's defaults, a grayscale (Oracle run E7), with
    // nothing keyed.
    let grayscale = |id| fx_tint(id, ([0, 0, 0], [255, 255, 255], 100.0));
    assert_eq!(
        effect_stacks(&document),
        BTreeMap::from([(0, json!([grayscale(1)])), (5000, json!([grayscale(2)]))])
    );
    assert!(corner_tracks(&document).is_empty());
    // Export writes each as a default Tint: the Black & White identity is not
    // kept, because every `tintTritone` is a Tint.
    let mut builder =
        TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap()).unwrap();
    for (id, asset) in &converted.metadata().assets {
        let name = Path::new(&asset.path).file_name().unwrap();
        builder = builder
            .add_asset(
                id.as_str(),
                fixture_path(name.to_str().unwrap()),
                asset.kind,
            )
            .unwrap();
    }
    let unedited = root.join("unedited.tsrct");
    builder.write(&unedited).unwrap();
    let omissions = tesseract_to_premiere(&unedited, root.join("native"), false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let exported = root.join("native/project.prproj");
    let xml = read_xml(&exported);
    assert_eq!(xml.matches("AE.ADBE Black &amp; White").count(), 0);
    let default_tint = (
        "false".to_owned(),
        vec![
            ((0xff00_u64 << 48).to_string(), None),
            (
                (0xff00_u64 << 48 | 255 << 40 | 255 << 24 | 255 << 8).to_string(),
                None,
            ),
            ("100.".to_owned(), None),
        ],
    );
    assert_eq!(
        native_tints(&exported),
        [default_tint.clone(), default_tint]
    );
    let omissions = premiere_to_tesseract(&exported, root.join("reimported"), None, false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let reimported = TesseractFile::open(first_project(&root.join("reimported")))
        .unwrap()
        .project_json()
        .unwrap();
    assert_eq!(effect_stacks(&reimported), effect_stacks(&document));
}
