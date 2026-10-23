//! `source.inputTransform` export of a camera-input LUT: the footage stays
//! editable and the LUT is a diagnosed omission (no native per-footage LUT;
//! only an SDR-display 3D LUT becomes a head-of-stack Apply Color LUT, see
//! `adapter::apply_color_lut::tests`). Own-reader structural evidence only.

use super::*;

const OMITTED: &str = "source.inputTransform omitted";

fn with_lut(mut layer: Value) -> Value {
    layer["source"]["inputTransform"] = json!({"semanticVersion": 1, "transform": {
        "type": "lut3d",
        "assetId": "input-lut",
        "inputEncoding": "camera:apple-log:v1",
        "outputEncoding": "jerboa:sdr-rec709-display:v1",
        "interpolation": "trilinear"
    }});
    layer
}

#[test]
fn input_transform_omission_keeps_image_and_video_footage() {
    let mut clip = with_lut(video(191, "lut-mov"));
    let span = json!({"start": 500, "duration": 1000});
    clip.as_object_mut().unwrap().remove("activeRange");
    clip["sourceRange"] = span.clone();
    clip["playback"] = fixture_linear_playback(json!({"start":1000,"duration":1000}), span);
    let doc = document(vec![with_lut(image(190, "lut-exr")), clip], Vec::new());
    let sources = BTreeMap::from([
        (
            "lut-exr".into(),
            resolved(
                "lut-exr",
                "media/lut.exr",
                NativeSourceFormat::OpenExr,
                [640, 360],
                0,
                NativeFrameRate::integer(0),
                0.0,
            ),
        ),
        (
            "lut-mov".into(),
            resolved(
                "lut-mov",
                "media/lut.mov",
                NativeSourceFormat::QuickTime,
                [1920, 1080],
                4000,
                NativeFrameRate::integer(24),
                0.0,
            ),
        ),
    ]);
    let output = to_aep_with_media(&doc, &sources).unwrap();
    let native = read_project(&output.bytes).unwrap();
    let names: Vec<_> = layers(&native)
        .iter()
        .map(|layer| layer.name.as_ref().to_owned())
        .collect();
    assert_eq!(
        names,
        ["image-190", "video-191"],
        "{:?}",
        output.diagnostics
    );
    assert!(output.omitted_layer_ids.is_empty());
    for id in [190, 191] {
        assert!(
            output.diagnostics.iter().any(|diagnostic| {
                diagnostic.layer_id == Some(LayerId::new(id))
                    && diagnostic.message.contains(OMITTED)
            }),
            "{id}: {:?}",
            output.diagnostics
        );
    }
}

/// In an FX ACES document a `cameraInput` is the footage's OCIO input
/// colourspace: no omission, and the source item declares it (`ocio.rs`).
#[test]
fn aces_camera_input_writes_the_footage_ocio_input_colourspace() {
    let mut clip = video(191, "slog3-mov");
    clip["source"]["inputTransform"] = json!({"semanticVersion": 1, "transform": {
        "type": "cameraInput",
        "inputEncoding": "camera:sony-slog3-sgamut3-cine:v1"
    }});
    let span = json!({"start": 500, "duration": 1000});
    clip.as_object_mut().unwrap().remove("activeRange");
    clip["sourceRange"] = span.clone();
    clip["playback"] = fixture_linear_playback(json!({"start":1000,"duration":1000}), span);
    let mut value = document(vec![clip], Vec::new()).to_json_value().unwrap();
    value["colorManagement"] =
        json!({"workingSpace":"acescct","outputTransform":"aces2.0","compositing":"wholeProgram"});
    let doc = EditableFxCompositionDocument::from_json_value(value).unwrap();
    let sources = BTreeMap::from([(
        "slog3-mov".into(),
        resolved(
            "slog3-mov",
            "media/slog3.mov",
            NativeSourceFormat::QuickTime,
            [1920, 1080],
            4000,
            NativeFrameRate::integer(24),
            0.0,
        ),
    )]);
    let output = to_aep_with_media(&doc, &sources).unwrap();
    assert!(
        !output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains(OMITTED)),
        "{:?}",
        output.diagnostics
    );
    let native = read_project(&output.bytes).unwrap();
    let inputs: Vec<_> = native
        .items
        .iter()
        .filter_map(|item| item.ocio_input.as_deref())
        .collect();
    assert_eq!(inputs, ["S-Log3 S-Gamut3.Cine"]);
    assert_eq!(
        native.color,
        crate::structure::ProjectColorContext::Aces {
            config: "ACES 2.0 Studio v4.0".into()
        }
    );
}
