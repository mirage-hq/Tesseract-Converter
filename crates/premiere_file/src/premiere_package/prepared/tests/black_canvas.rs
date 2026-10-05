//! Reduced source-pattern regression, not independent Adobe render evidence.
use super::*;
use crate::schema::text::{PrFill, PrGraphicObject, PrRgb};

fn canvas_archive(root: &Path, audio_first: bool) -> TesseractFile {
    let mut value = crate::test_support::editable_document();
    // The failing launch source has a 77.867s document, a 77866ms black Shape
    // (not a legacy Rect), and sound below it. Keep its geometry and endpoint;
    // substitute the short public audio fixture for the private full mix.
    value["duration"] = json!(77.867);
    let canvas = json!({
        "type":"Shape", "id":1, "name":"Authored black background",
        "blendMode":"normal", "activeRange":{"start":0,"duration":77866},
        "transform":{"anchorPoint":[0,0],"position":[0,0],"scale":[100,100],"rotation":0,"opacity":100},
        "shape":{
            "path":{"commands":[
                {"type":"moveTo","x":0,"y":0},
                {"type":"lineTo","x":1920,"y":0},
                {"type":"lineTo","x":1920,"y":1080},
                {"type":"lineTo","x":0,"y":1080},
                {"type":"close"}
            ]},
            "fills":[{"paint":{"type":"solid","color":[0,0,0,1]},"fillRule":"nonZeroWinding","blendMode":"normal","opacity":1}],
            "strokes":[]
        }
    });
    let sound = json!({
        "type":"Audio", "id":2, "name":"Sound below picture",
        "playback":crate::test_support::linear_playback(json!({"start":21,"duration":200}), json!({"start":0,"duration":200})),
        "sourceRange":{"start":0,"duration":200}, "sourceIntrinsicDuration":200,
        "volume":1, "source":{"assetId":"music"}
    });
    value["composition"]["layers"] = if audio_first {
        json!([sound, canvas])
    } else {
        json!([canvas, sound])
    };
    value["composition"]["dynamics"] = json!({"entries":[]});
    TesseractFileBuilder::from_project_json(&serde_json::to_vec(&value).unwrap())
        .unwrap()
        .add_asset(
            "music",
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/audio-mono.wav"),
            AssetKind::Audio,
        )
        .unwrap()
        .write(root.join("input.tsrct"))
        .unwrap()
}

#[test]
fn authored_black_shape_with_audio_remains_editable_and_covers_the_sequence() {
    for audio_first in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let file = canvas_archive(root.path(), audio_first);
        let source = fs::read(root.path().join("input.tsrct")).unwrap();
        let prepared = Premiere
            .prepare_export(&file, file.project(), &Default::default())
            .unwrap();
        // Exercise the same final stage as automatic hybrid export, without a
        // foreign picture hiding a failure to recognize the authored graphic.
        let staged = prepared
            .stage_with_picture_replacements(root.path(), &root.path().join("final"), &[])
            .unwrap();
        assert!(staged.after_effects_paths().is_empty());
        let native = read_native(&staged.directory().join("project.prproj"));
        let sequence = native.single_sequence().unwrap();
        let graphics: Vec<_> = sequence
            .video_items()
            .filter_map(|item| item.graphic())
            .collect();
        assert_eq!(graphics.len(), 1, "keep the authored editable background");
        let graphic = graphics[0];
        assert_eq!(graphic.start_ticks, 0);
        assert_eq!(graphic.end_ticks, 2336 * FrameRate::Fps30.ticks_per_frame());
        assert_eq!(sequence.end_ticks(), graphic.end_ticks);
        let [PrGraphicObject::Shape(shape)] = graphic.objects.as_slice() else {
            panic!("one editable Shape required")
        };
        assert_eq!(shape.appearance.fill, Some(PrFill::Solid(PrRgb([0; 3]))));
        assert_eq!(
            shape
                .path
                .vertices
                .iter()
                .map(|vertex| vertex.point)
                .collect::<Vec<_>>(),
            [[0.0, 0.0], [1920.0, 0.0], [1920.0, 1080.0], [0.0, 1080.0]]
        );
        assert!(shape.path.closed);
        assert_eq!(shape.transform.position, [0.0, 0.0]);
        assert_eq!(shape.transform.anchor, [0.0, 0.0]);
        assert_eq!(shape.transform.scale, 100.0);
        assert_eq!(shape.transform.opacity, 100.0);
        assert_eq!(sequence.audio.len(), 1);
        let audio = &sequence.audio[0];
        assert_eq!(audio.start_ticks, 21 * crate::schema::TICKS_PER_MILLISECOND);
        assert_eq!(audio.end_ticks, 221 * crate::schema::TICKS_PER_MILLISECOND);
        assert_eq!(audio.in_ticks, 0);
        assert_eq!(audio.out_ticks, 200 * crate::schema::TICKS_PER_MILLISECOND);
        assert_eq!(fs::read(root.path().join("input.tsrct")).unwrap(), source);
    }
}

#[test]
fn removed_authored_black_shape_exports_as_a_disabled_replacement() {
    let root = tempfile::tempdir().unwrap();
    let file = canvas_archive(root.path(), false);
    let prepared = Premiere
        .prepare_export(&file, file.project(), &Default::default())
        .unwrap();
    let recipe = prepared.packing_recipe();
    let container = recipe
        .containers()
        .into_iter()
        .find(|c| c.token == recipe.root())
        .unwrap();
    let boundary = recipe
        .boundaries()
        .into_iter()
        .find(|b| b.layer == fx_schema::LayerId::new(1))
        .unwrap();
    let replacement = PictureReplacement {
        packing_id: recipe.id(),
        container: recipe.root(),
        boundaries: vec![boundary.token],
        picture: crate::AfterEffectsPicture {
            composition_guid: "00000001-0000-0000-0000-000000000000".into(),
            relative_path: "media/ae-0001/compositions.aep".into(),
            dimensions: container.dimensions,
            frame_rate: container.frame_rate,
            intrinsic_duration_ticks: container.timeline_end_ticks,
            timeline_ticks: 0..container.timeline_end_ticks,
            source_ticks: 0..container.timeline_end_ticks,
            enabled: false,
        },
    };
    let timeline_end = container.timeline_end_ticks;
    let staged = prepared
        .stage_with_picture_replacements(root.path(), &root.path().join("final"), &[replacement])
        .unwrap();
    let native = read_native(&staged.directory().join("project.prproj"));
    let sequence = native.single_sequence().unwrap();
    assert_eq!(sequence.video_items().count(), 1);
    assert_eq!(
        sequence
            .video_items()
            .filter_map(|item| item.graphic())
            .count(),
        0
    );
    let linked = sequence.video_occurrences().next().unwrap();
    assert!(!linked.enabled);
    assert_eq!(linked.timeline_ticks(), 0..timeline_end);
    assert_eq!(linked.source_ticks(), 0..timeline_end);
    assert_eq!(sequence.end_ticks(), timeline_end);
    assert_eq!(
        sequence.gaps(&native.media),
        Vec::from_iter(Some(0..timeline_end))
    );
    assert_eq!(sequence.audio.len(), 1);
    assert_eq!(
        sequence.audio[0].start_ticks..sequence.audio[0].end_ticks,
        21 * crate::schema::TICKS_PER_MILLISECOND..221 * crate::schema::TICKS_PER_MILLISECOND
    );
    assert_eq!(
        staged.after_effects_paths(),
        [PathBuf::from("media/ae-0001/compositions.aep")]
    );
    assert!(!root.path().join("final").exists());
}
