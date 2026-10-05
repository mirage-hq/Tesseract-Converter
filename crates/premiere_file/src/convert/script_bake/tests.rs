//! Script baking through the public export entry, with native readback.
//!
//! Expected values come from each test's own script formula, written again
//! in Rust; no test evaluates keys with a second runtime.

use super::*;
use crate::test_support::write_archive;
#[cfg(feature = "ffmpeg-library")]
use crate::{
    format::{PrProjectFile, PrSequence},
    schema::{PrEffectParamKeys, PrPropertyAnimation, TICKS_PER_MILLISECOND},
};
use fx_schema::PropType;
use serde_json::{json, Value};
#[cfg(feature = "ffmpeg-library")]
use std::f64::consts::PI;
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};
#[cfg(feature = "ffmpeg-library")]
use tesseract_file::TesseractFile;

/// FX `blurriness` per native Amount of the current Gaussian Blur, and FX
/// `blurLength` per native Amount of the current Directional Blur.
#[cfg(feature = "ffmpeg-library")]
const BLURRINESS_PER_AMOUNT: f64 = 5.7;
#[cfg(feature = "ffmpeg-library")]
const BLUR_LENGTH_PER_AMOUNT: f64 = 1.6;

/// The representative red-video fixture: two scripts on a placed, trimmed clip.
fn video_fixture() -> Value {
    serde_json::from_str(include_str!("../../../tests/fixtures/script-video.json")).unwrap()
}

/// `raw` packaged with the fixture video, which lasts 10 s.
fn archive(directory: &Path, raw: &Value) -> PathBuf {
    let source = directory.join("script.tsrct");
    let media = directory.join("source.mp4");
    std::fs::write(
        &media,
        include_bytes!("../../../tests/fixtures/feature_multi_sequence_red_10s.mp4"),
    )
    .unwrap();
    write_archive(&source, raw, &media);
    source
}

fn layer_target(layer: u64, property: &str) -> Value {
    json!({"kind": "layer", "layerId": layer, "propertyType": property})
}

fn effect_target(effect: u64, param: &str) -> Value {
    json!({"kind": "effectProperty", "effectId": effect, "paramName": param})
}

/// Gives each target of `scripts` its layer-time script, replacing its
/// current animator or adding its entry.
fn set_scripts(document: &mut Value, scripts: &[(Value, &str)]) {
    let dynamics = &mut document["composition"]["dynamics"];
    if dynamics.is_null() {
        *dynamics = json!({"entries": []});
    }
    let entries = dynamics["entries"].as_array_mut().unwrap();
    for (target, code) in scripts {
        let animator = json!({"type": "jsScript", "layerTimeJsCode": code});
        match entries.iter_mut().find(|entry| entry["target"] == *target) {
            Some(entry) => entry["animator"] = animator,
            None => entries.push(json!({"target": target, "animator": animator})),
        }
    }
}

/// Exports `source` in check and write mode through the public entry, which
/// must report the same diagnostics, and loads the written project.
#[cfg(feature = "ffmpeg-library")]
fn export(source: &Path) -> (Vec<Omission>, PrProjectFile) {
    let directory = source.parent().unwrap();
    let check = directory.join("check");
    let diagnostics = crate::tesseract_to_premiere(source, &check, true).unwrap();
    assert!(!check.exists());
    let output = directory.join("export");
    assert_eq!(
        diagnostics,
        crate::tesseract_to_premiere(source, &output, false).unwrap()
    );
    let (project, read_omissions) = PrProjectFile::load(output.join("project.prproj")).unwrap();
    assert!(read_omissions.is_empty(), "{read_omissions:?}");
    (diagnostics, project)
}

fn reasons(diagnostics: &[Omission]) -> Vec<&str> {
    diagnostics
        .iter()
        .map(|item| item.reason.as_str())
        .collect()
}

fn assert_written(diagnostics: &[Omission], count: usize) {
    let summary =
        format!("{count} of {count} baked JS animation tracks were written as native keys");
    assert!(
        reasons(diagnostics).contains(&summary.as_str()),
        "{:#?}",
        reasons(diagnostics)
    );
}

#[cfg(feature = "ffmpeg-library")]
fn sequence(project: &PrProjectFile) -> &PrSequence {
    project.single_sequence().unwrap()
}

/// Native keys as (owner-local ms, value), from the owner's source in-point.
#[cfg(feature = "ffmpeg-library")]
fn local(keys: &[crate::schema::PrScalarKeyframe], source_in: i64) -> Vec<(f64, f64)> {
    keys.iter()
        .map(|key| {
            let ms = (key.source_ticks - source_in) as f64 / TICKS_PER_MILLISECOND as f64;
            (ms, key.value)
        })
        .collect()
}

/// Every key lies on `expected`, the script's formula at owner-local ms,
/// and the keys are not one constant.
#[cfg(feature = "ffmpeg-library")]
fn assert_on_curve(keys: &[(f64, f64)], expected: impl Fn(f64) -> f64, tolerance: f64) {
    assert!(
        keys.windows(2).any(|pair| pair[0].1 != pair[1].1),
        "{keys:?}"
    );
    for &(ms, value) in keys {
        let want = expected(ms);
        assert!(
            (value - want).abs() <= tolerance,
            "key at {ms} ms is {value}, the script gives {want}: {keys:?}"
        );
    }
}

#[cfg(feature = "ffmpeg-library")]
fn scalar(
    animations: &[PrPropertyAnimation],
    select: fn(&PrPropertyAnimation) -> bool,
) -> &[crate::schema::PrScalarKeyframe] {
    animations
        .iter()
        .find(|animation| select(animation))
        .and_then(PrPropertyAnimation::scalar_keys)
        .unwrap_or_else(|| panic!("{animations:?}"))
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn public_export_retains_editable_owner_units_and_local_time_without_changing_source() {
    let directory = tempfile::tempdir().unwrap();
    let source = archive(directory.path(), &video_fixture());
    let before = std::fs::read(&source).unwrap();
    let (diagnostics, project) = export(&source);
    assert_eq!(
        reasons(&diagnostics),
        [
            "JS animation baking: 2 of 2 scripts became editable keys (4 keys, from at most 6130 JavaScript evaluations); each fit stays within its target's tolerance at every integer millisecond of its owner's window, which is not Adobe render-fidelity proof",
            "2 of 2 baked JS animation tracks were written as native keys",
        ]
    );
    let clip = sequence(&project).video_occurrences().next().unwrap();
    assert_eq!(
        (clip.start_ticks, clip.end_ticks),
        (500 * TICKS_PER_MILLISECOND, 3500 * TICKS_PER_MILLISECOND)
    );
    assert_eq!(
        (clip.in_ticks, clip.out_ticks),
        (1000 * TICKS_PER_MILLISECOND, 4000 * TICKS_PER_MILLISECOND)
    );
    assert_eq!(clip.animations.len(), 2);
    for animation in &clip.animations {
        let (keys, expected) = match animation {
            PrPropertyAnimation::Opacity(keys) => (keys, [20.0, 80.0]),
            PrPropertyAnimation::Rotation(keys) => (keys, [-30.0, 30.0]),
            _ => panic!("unexpected native property"),
        };
        assert_eq!(
            local(keys, clip.in_ticks),
            [(0.0, expected[0]), (3000.0, expected[1])]
        );
    }
    assert_eq!(std::fs::read(&source).unwrap(), before);
}

/// Nonlinear Motion scripts become Premiere's paired Position point, one
/// uniform Scale and Bezier-eased scalar keys, each on the owner's clock.
#[cfg(feature = "ffmpeg-library")]
#[test]
fn video_motion_scripts_become_paired_uniform_and_nonlinear_native_keys() {
    let directory = tempfile::tempdir().unwrap();
    let mut raw = video_fixture();
    let spring = "const t = input.time.seconds; return 45 * Math.exp(-3 * t) * Math.sin(10 * t);";
    set_scripts(
        &mut raw,
        &[
            (
                layer_target(1, "opacity"),
                "return 50 + 40 * Math.sin(input.time.seconds * Math.PI);",
            ),
            (layer_target(1, "rotation"), spring),
            (
                layer_target(1, "positionX"),
                "return 960 + 300 * Math.sin(input.time.seconds * 2);",
            ),
            (
                layer_target(1, "positionY"),
                "return 540 + 200 * Math.cos(input.time.seconds * 3);",
            ),
            (
                layer_target(1, "scaleX"),
                "return 45 + 10 * Math.pow(input.time.seconds / 3, 2);",
            ),
            (
                layer_target(1, "scaleY"),
                "return 45 + 10 * Math.pow(input.time.seconds / 3, 2);",
            ),
        ],
    );
    let source = archive(directory.path(), &raw);
    let (diagnostics, project) = export(&source);
    assert_written(&diagnostics, 6);
    let clip = sequence(&project).video_occurrences().next().unwrap();
    let source_in = clip.in_ticks;
    let seconds = |ms: f64| ms / 1000.0;
    let opacity = local(
        scalar(&clip.animations, |a| {
            matches!(a, PrPropertyAnimation::Opacity(_))
        }),
        source_in,
    );
    assert_on_curve(&opacity, |ms| 50.0 + 40.0 * (seconds(ms) * PI).sin(), 1e-9);
    let rotation = local(
        scalar(&clip.animations, |a| {
            matches!(a, PrPropertyAnimation::Rotation(_))
        }),
        source_in,
    );
    assert_on_curve(
        &rotation,
        |ms| 45.0 * (-3.0 * seconds(ms)).exp() * (10.0 * seconds(ms)).sin(),
        1e-9,
    );
    assert!(clip
        .animations
        .iter()
        .any(
            |animation| animation.scalar_keys().is_some_and(|keys| keys.iter().any(
                |key| matches!(
                    key.easing,
                    crate::schema::PrKeyframeEasing::CubicBezier { .. }
                )
            ))
        ));
    let scale = local(
        scalar(&clip.animations, |a| {
            matches!(a, PrPropertyAnimation::UniformScale(_))
        }),
        source_in,
    );
    assert_on_curve(&scale, |ms| 45.0 + 10.0 * (seconds(ms) / 3.0).powi(2), 1e-9);
    // One point track: both axes at every key, normalized to the canvas.
    let position = clip
        .animations
        .iter()
        .find_map(PrPropertyAnimation::point_keys)
        .unwrap();
    assert!(position.len() >= 3);
    for key in position {
        let t = seconds((key.source_ticks - source_in) as f64 / TICKS_PER_MILLISECOND as f64);
        assert!((key.value[0] * 1920.0 - (960.0 + 300.0 * (t * 2.0).sin())).abs() < 1e-6);
        assert!((key.value[1] * 1080.0 - (540.0 + 200.0 * (t * 3.0).cos())).abs() < 1e-6);
    }

    // Reimport recovers the same editable keys on the video's own clock.
    let reimported = directory.path().join("reimported");
    crate::premiere_to_tesseract(
        directory.path().join("export/project.prproj"),
        &reimported,
        None,
        false,
    )
    .unwrap();
    let archive = std::fs::read_dir(&reimported)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().is_some_and(|ext| ext == "tsrct"))
        .unwrap();
    let document = TesseractFile::open(archive)
        .unwrap()
        .project_json()
        .unwrap();
    let times = |property: &str| -> Vec<i64> {
        document["composition"]["dynamics"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["target"]["propertyType"] == property)
            .unwrap_or_else(|| panic!("{property}"))["animator"]["keyframes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|key| key["layerTime"].as_i64().unwrap())
            .collect()
    };
    assert_eq!(times("positionX"), times("positionY"));
    assert_eq!(times("scaleX"), times("scaleY"));
    assert_eq!(times("opacity").len(), opacity.len());
}

/// A Position with one scripted axis keeps the static axis at every key of
/// the one native point.
#[cfg(feature = "ffmpeg-library")]
#[test]
fn a_single_scripted_position_axis_keeps_the_static_axis_on_every_key() {
    let directory = tempfile::tempdir().unwrap();
    let mut raw = video_fixture();
    raw["composition"]["dynamics"] = json!({"entries": []});
    set_scripts(
        &mut raw,
        &[(
            layer_target(1, "positionX"),
            "return 600 + 100 * input.time.seconds * input.time.seconds;",
        )],
    );
    let document = EditableFxCompositionDocument::from_json_value(raw.clone()).unwrap();
    let before = document.to_json_value().unwrap();
    let mut omissions = Vec::new();
    let baked = bake_scripts(&document, &mut omissions).unwrap();
    assert_eq!(document.to_json_value().unwrap(), before);
    let entries = baked.document().composition().dynamics().entries();
    let [x, y] = entries else {
        panic!("the static axis gets its own track: {entries:?}")
    };
    let (x, y) = (
        x.animator.keyframe_track().unwrap(),
        y.animator.keyframe_track().unwrap(),
    );
    assert_eq!(y.keyframes().len(), x.keyframes().len());
    for (x, y) in x.keyframes().iter().zip(y.keyframes()) {
        assert_eq!((x.layer_time(), x.easing()), (y.layer_time(), y.easing()));
        assert_eq!(y.value(), &PropertyValue::Float(540.0));
    }
    let source = archive(directory.path(), &raw);
    let (diagnostics, project) = export(&source);
    assert_written(&diagnostics, 1);
    let clip = sequence(&project).video_occurrences().next().unwrap();
    let points = clip
        .animations
        .iter()
        .find_map(PrPropertyAnimation::point_keys)
        .unwrap();
    assert_eq!(points.len(), x.keyframes().len());
    assert!(points.iter().all(|key| key.value[1] == 0.5));
}

#[test]
fn script_baking_reports_from_the_evaluation_loop() {
    let mut raw = video_fixture();
    set_scripts(
        &mut raw,
        &[
            (layer_target(7, "opacity"), "return input.time.seconds;"),
            (layer_target(7, "rotation"), "return input.time.seconds;"),
        ],
    );
    let document = EditableFxCompositionDocument::from_json_value(raw).unwrap();
    let main_thread = std::thread::current().id();
    let events = Mutex::new(Vec::new());
    let callback = |event| {
        events
            .lock()
            .unwrap()
            .push((event, std::thread::current().id()));
    };

    bake_scripts_with_progress(
        &document,
        &mut Vec::new(),
        fx_conv::Progress::new(&callback),
    )
    .unwrap();

    let events = events.into_inner().unwrap();
    let (started, _) = events
        .iter()
        .find(|(event, _)| event.started && event.phase == "baking Premiere scripts")
        .unwrap();
    assert_eq!(started.total, Some(2));
    assert!(events.iter().any(|(event, thread)| {
        event.phase == started.phase
            && event.completed == Some(1)
            && !event.started
            && *thread != main_thread
    }));
    assert!(events.iter().any(|(event, _)| {
        event.phase == started.phase && event.completed == started.total && !event.started
    }));
}

/// Imports the Adobe-derived `fixture`, which packages its media, and
/// returns its archive and editable document.
#[cfg(feature = "ffmpeg-library")]
fn imported(directory: &Path, fixture: &str) -> (PathBuf, Value) {
    let output = directory.join("imported");
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let source = fixtures.join(fixture);
    let source = if fixture == "feature_adjustment_layer_26_5_strict.prproj" {
        // Premiere saved two hints per source: ./file.mp4 and ../files/file.mp4.
        // Stage both with identical bytes rather than weakening media identity
        // checks or changing the Adobe-authored project.
        let package = directory.join("source");
        let sibling = directory.join("files");
        std::fs::create_dir_all(&package).unwrap();
        std::fs::create_dir_all(&sibling).unwrap();
        for name in [
            "feature_linked_av_source.mp4",
            "feature_timecoded_source.mp4",
        ] {
            for destination in [&package, &sibling] {
                std::fs::copy(fixtures.join(name), destination.join(name)).unwrap();
            }
        }
        let staged = package.join(fixture);
        std::fs::copy(source, &staged).unwrap();
        staged
    } else {
        source
    };
    crate::premiere_to_tesseract(&source, &output, None, false).unwrap();
    let archive = std::fs::read_dir(&output)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().is_some_and(|ext| ext == "tsrct"))
        .unwrap();
    let document = TesseractFile::open(&archive)
        .unwrap()
        .project_json()
        .unwrap();
    (archive, document)
}

/// `archive` with `document` as its project and the same packaged assets.
#[cfg(feature = "ffmpeg-library")]
fn rescripted(archive: &Path, document: &Value) -> PathBuf {
    let mut file = TesseractFile::open(archive).unwrap();
    file.replace_project(EditableFxCompositionDocument::from_json_value(document.clone()).unwrap())
        .unwrap();
    let edited = archive
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("scripted/scripted.tsrct");
    std::fs::create_dir_all(edited.parent().unwrap()).unwrap();
    file.save_as(&edited).unwrap();
    edited
}

/// Owner-local native keys of one binding in an exported project.
#[cfg(feature = "ffmpeg-library")]
type NativeKeys = fn(&PrSequence) -> Vec<(f64, f64)>;

/// One owner case: an Adobe-derived fixture, the scripts that replace or
/// join its animation, the native keys they write, the scripts' curve at
/// owner-local milliseconds, and the number of baked tracks.
#[cfg(feature = "ffmpeg-library")]
type OwnerCase<'a> = (
    &'a str,
    Vec<(Value, &'a str)>,
    NativeKeys,
    &'a dyn Fn(f64) -> f64,
    usize,
);

/// The topmost clip that starts at `ms`.
#[cfg(feature = "ffmpeg-library")]
fn clip_at(sequence: &PrSequence, ms: i64) -> &crate::format::PrVideoOccurrence {
    sequence
        .video_occurrences()
        .rfind(|clip| clip.start_ticks == ms * TICKS_PER_MILLISECOND)
        .unwrap()
}

/// Owner-local keys of the native parameter `name` of `clip`'s first effect.
#[cfg(feature = "ffmpeg-library")]
fn effect_keys(clip: &crate::format::PrVideoOccurrence, name: &str) -> Vec<(f64, f64)> {
    let animation = clip.effects[0]
        .animations
        .iter()
        .find(|animation| animation.param.name == name)
        .unwrap_or_else(|| panic!("{name}: {:?}", clip.effects[0]));
    local(animation.keys.scalar().unwrap(), clip.in_ticks)
}

/// Scripts on the owners of Adobe-derived fixtures become the native keys of
/// their bindings: flat Crop guides that follow their video, root and stage
/// Linear Wipes, a stage group's Motion, graphic Vector Motion, text and
/// clip Opacity, audio and clip sound Volume, and each kind of mapped effect
/// parameter.
#[cfg(feature = "ffmpeg-library")]
#[test]
fn scripts_on_every_owner_kind_reach_their_native_bindings() {
    let ramp = "return 50 + 40 * Math.sin(input.time.seconds * 2);";
    let expect_ramp = |ms: f64| 50.0 + 40.0 * (ms / 1000.0 * 2.0).sin();
    let cases: &[OwnerCase<'_>] = &[
        // Video 3 and its Crop guide 8 share their rotation, so the Crop
        // follows the clip's written Motion.
        (
            "feature_stage_motion_26_5_strict.prproj",
            vec![
                (layer_target(3, "rotation"), ramp),
                (layer_target(8, "rotation"), ramp),
            ],
            |sequence| {
                let clip = clip_at(sequence, 2000);
                local(
                    scalar(&clip.animations, |a| {
                        matches!(a, PrPropertyAnimation::Rotation(_))
                    }),
                    clip.in_ticks,
                )
            },
            &expect_ramp,
            2,
        ),
        // The stage group's Linear Wipe guide: completion is 100 − visible.
        (
            "feature_stage_motion_26_5_strict.prproj",
            vec![(
                layer_target(10, "scaleX"),
                "return 100 - 80 * Math.sin(input.time.seconds * Math.PI / 4);",
            )],
            |sequence| {
                let clip = clip_at(sequence, 4000);
                local(
                    &clip.linear_wipe.as_ref().unwrap().completion,
                    clip.in_ticks,
                )
            },
            &|ms| 80.0 * (ms / 1000.0 * PI / 4.0).sin(),
            1,
        ),
        // The stage group's own Motion is its one clip's, on the group's clock.
        (
            "feature_stage_motion_26_5_strict.prproj",
            vec![(layer_target(12, "rotation"), ramp)],
            |sequence| {
                let clip = clip_at(sequence, 4000);
                local(
                    scalar(&clip.animations, |a| {
                        matches!(a, PrPropertyAnimation::Rotation(_))
                    }),
                    clip.in_ticks,
                )
            },
            &expect_ramp,
            1,
        ),
        // A Transform stage's video: its Transform takes the video's keys.
        (
            "feature_transform_strict.prproj",
            vec![(layer_target(3, "rotation"), ramp)],
            |sequence| effect_keys(clip_at(sequence, 2000), "Rotation"),
            &expect_ramp,
            1,
        ),
        // A root clip's Linear Wipe guide.
        (
            "feature_linear_wipe_strict.prproj",
            vec![(
                layer_target(3, "scaleX"),
                "return 100 - 60 * Math.sin(input.time.seconds * Math.PI / 6);",
            )],
            |sequence| {
                let clip = clip_at(sequence, 2000);
                local(
                    &clip.linear_wipe.as_ref().unwrap().completion,
                    clip.in_ticks,
                )
            },
            &|ms| 60.0 * (ms / 1000.0 * PI / 6.0).sin(),
            1,
        ),
        // Vector Motion rotation and the text's Opacity on a graphic's clock.
        (
            "feature_graphic_transform_keys_26_5_strict.prproj",
            vec![(layer_target(3, "rotation"), ramp)],
            |sequence| {
                let crate::format::PrVideoItem::Graphic(graphic) = sequence
                    .video_items()
                    .find(|item| matches!(item, crate::format::PrVideoItem::Graphic(_)))
                    .unwrap()
                else {
                    unreachable!()
                };
                local(
                    scalar(&graphic.vector_motion.as_ref().unwrap().animations, |a| {
                        matches!(a, PrPropertyAnimation::Rotation(_))
                    }),
                    graphic.in_ticks,
                )
            },
            &expect_ramp,
            1,
        ),
        (
            "feature_graphic_transform_keys_26_5_strict.prproj",
            vec![(layer_target(2, "opacity"), ramp)],
            |sequence| {
                let crate::format::PrVideoItem::Graphic(graphic) = sequence
                    .video_items()
                    .find(|item| matches!(item, crate::format::PrVideoItem::Graphic(_)))
                    .unwrap()
                else {
                    unreachable!()
                };
                let crate::schema::text::PrGraphicObject::Text(text) = &graphic.objects[0] else {
                    unreachable!()
                };
                local(
                    scalar(&text.animations, |a| {
                        matches!(a, PrPropertyAnimation::Opacity(_))
                    }),
                    graphic.in_ticks,
                )
            },
            &expect_ramp,
            1,
        ),
        (
            "feature_graphic_clip_opacity_keys_26_5_strict.prproj",
            vec![(layer_target(3, "opacity"), ramp)],
            |sequence| {
                let crate::format::PrVideoItem::Graphic(graphic) = sequence
                    .video_items()
                    .find(|item| matches!(item, crate::format::PrVideoItem::Graphic(_)))
                    .unwrap()
                else {
                    unreachable!()
                };
                local(
                    scalar(&graphic.animations, |a| {
                        matches!(a, PrPropertyAnimation::Opacity(_))
                    }),
                    graphic.in_ticks,
                )
            },
            &expect_ramp,
            1,
        ),
        // The current Gaussian Blur's Amount is FX blurriness per 5.7.
        (
            "feature_gaussian_blur_keys_26_5_strict.prproj",
            vec![(effect_target(1, "blurriness"), ramp)],
            |sequence| effect_keys(clip_at(sequence, 0), "Amount"),
            &|ms| expect_ramp(ms) / BLURRINESS_PER_AMOUNT,
            1,
        ),
        (
            "feature_brightness_contrast_strict.prproj",
            vec![(effect_target(5, "brightness"), ramp)],
            |sequence| {
                let clip = clip_at(sequence, 6000);
                let keys = clip.effects[0].animations[0].keys.scalar().unwrap();
                local(keys, clip.in_ticks)
            },
            &expect_ramp,
            1,
        ),
        // Beside the fixture's authored Brightness keys.
        (
            "feature_brightness_contrast_strict.prproj",
            vec![(
                effect_target(5, "contrast"),
                "return 20 * Math.sin(input.time.seconds * 3);",
            )],
            |sequence| effect_keys(clip_at(sequence, 6000), "Contrast"),
            &|ms| 20.0 * (ms / 1000.0 * 3.0).sin(),
            1,
        ),
        // On an unscaled, unrotated clip, the current Directional Blur's
        // Amount is FX blurLength per 1.6, and its Angle the FX direction.
        (
            "feature_directional_blur_strict.prproj",
            vec![(effect_target(5, "blurLength"), ramp)],
            |sequence| effect_keys(clip_at(sequence, 6000), "Amount"),
            &|ms| expect_ramp(ms) / BLUR_LENGTH_PER_AMOUNT,
            1,
        ),
        (
            "feature_directional_blur_strict.prproj",
            vec![(
                effect_target(5, "direction"),
                "return 90 + 30 * Math.sin(input.time.seconds * 2);",
            )],
            |sequence| effect_keys(clip_at(sequence, 6000), "Angle"),
            &|ms| 90.0 + 30.0 * (ms / 1000.0 * 2.0).sin(),
            1,
        ),
        // Levels writes whole native levels.
        (
            "feature_levels_strict.prproj",
            vec![(
                effect_target(4, "outputWhite"),
                "return 200 + 50 * Math.sin(input.time.seconds * Math.PI / 2);",
            )],
            |sequence| {
                let clip = clip_at(sequence, 6000);
                let keys = clip.effects[0].animations[0].keys.scalar().unwrap();
                local(keys, clip.in_ticks)
            },
            &|ms| (200.0 + 50.0 * (ms / 1000.0 * PI / 2.0).sin()).round(),
            1,
        ),
        // Gamma writes whole native hundredths.
        (
            "feature_levels_strict.prproj",
            vec![(
                effect_target(4, "gamma"),
                "return 1 + 0.3 * Math.sin(input.time.seconds * 2);",
            )],
            |sequence| effect_keys(clip_at(sequence, 6000), "(RGB) Gamma"),
            &|ms| (100.0 * (1.0 + 0.3 * (ms / 1000.0 * 2.0).sin())).round(),
            1,
        ),
        // Both coordinates of one corner: one native point track.
        (
            "feature_corner_pin_strict.prproj",
            vec![
                (
                    effect_target(3, "upperLeftX"),
                    "return 0.25 * Math.sin(input.time.seconds * Math.PI / 2);",
                ),
                (
                    effect_target(3, "upperLeftY"),
                    "return 0.2 * input.time.seconds / 2.5;",
                ),
            ],
            |sequence| {
                let clip = clip_at(sequence, 4000);
                let upper_left = clip.effects[0]
                    .animations
                    .iter()
                    .find(|animation| animation.param.id == 1)
                    .unwrap();
                let PrEffectParamKeys::Point(keys) = &upper_left.keys else {
                    unreachable!()
                };
                keys.iter()
                    .map(|key| {
                        let ms = (key.source_ticks - clip.in_ticks) as f64
                            / TICKS_PER_MILLISECOND as f64;
                        assert!((key.value[1] - 0.2 * ms / 1000.0 / 2.5).abs() < 1e-9);
                        (ms, key.value[0])
                    })
                    .collect()
            },
            &|ms| 0.25 * (ms / 1000.0 * PI / 2.0).sin(),
            2,
        ),
        // An adjustment layer's Opacity and its current Gaussian Blur.
        (
            "feature_adjustment_layer_26_5_strict.prproj",
            vec![(layer_target(5, "opacity"), ramp)],
            |sequence| {
                let clip = clip_at(sequence, 3000);
                local(
                    scalar(&clip.animations, |a| {
                        matches!(a, PrPropertyAnimation::Opacity(_))
                    }),
                    clip.in_ticks,
                )
            },
            &expect_ramp,
            1,
        ),
        (
            "feature_adjustment_layer_26_5_strict.prproj",
            vec![(effect_target(3, "blurriness"), ramp)],
            |sequence| effect_keys(clip_at(sequence, 3000), "Amount"),
            &|ms| expect_ramp(ms) / BLURRINESS_PER_AMOUNT,
            1,
        ),
        // A root sound's Volume: Premiere Linear pieces within 0.1 dB.
        (
            "feature_audio_volume_keys_strict.prproj",
            vec![(
                layer_target(4, "volume"),
                "return 0.2 + 0.8 * Math.sin(input.time.seconds * Math.PI / 5);",
            )],
            |sequence| {
                let sound = sequence
                    .audio
                    .iter()
                    .find(|sound| {
                        sound.start_ticks == 5000 * TICKS_PER_MILLISECOND
                            && sound.volume_keys.is_some()
                    })
                    .unwrap();
                let volume = sound.volume_keys.as_ref().unwrap();
                local(&volume.keys, sound.in_ticks)
                    .into_iter()
                    .map(|(ms, gain)| (ms, gain * volume.gain))
                    .collect()
            },
            &|ms| 0.2 + 0.8 * (ms / 1000.0 * PI / 5.0).sin(),
            1,
        ),
        // A root clip's own sound, which its video's Volume script makes
        // audible, beside the linked audio clip of the same source.
        (
            "feature_linked_av_strict.prproj",
            vec![(
                layer_target(1, "volume"),
                "return 0.2 + 0.8 * Math.sin(input.time.seconds * Math.PI / 5);",
            )],
            |sequence| {
                let sound = sequence
                    .audio
                    .iter()
                    .find(|sound| sound.volume_keys.is_some())
                    .unwrap();
                let volume = sound.volume_keys.as_ref().unwrap();
                local(&volume.keys, sound.in_ticks)
                    .into_iter()
                    .map(|(ms, gain)| (ms, gain * volume.gain))
                    .collect()
            },
            &|ms| 0.2 + 0.8 * (ms / 1000.0 * PI / 5.0).sin(),
            1,
        ),
    ];
    for (fixture, scripts, native, expected, baked) in cases {
        let directory = tempfile::tempdir().unwrap();
        let (archive, mut document) = imported(directory.path(), fixture);
        set_scripts(&mut document, scripts);
        let source = rescripted(&archive, &document);
        let (diagnostics, project) = export(&source);
        assert_written(&diagnostics, *baked);
        // Volume is written as Linear pieces within 0.1 dB.
        let sound = scripts
            .iter()
            .any(|(target, _)| target["propertyType"] == "volume");
        let tolerance = if sound { 0.02 } else { 1e-6 };
        assert_on_curve(&native(sequence(&project)), expected, tolerance);
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn tint_amount_bakes_but_range_errors_and_colour_channels_keep_their_scripts() {
    for (parameter, code, baked) in [
        ("amount", "return 20 + 20 * input.time.seconds;", true),
        ("amount", "return 101;", false),
        ("blackR", "return 0.25;", false),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut document = video_fixture();
        document["composition"]["dynamics"] = json!({"entries": []});
        document["composition"]["layers"][0]["effects"] = json!([{
            "id": 17, "enabled": true, "effect": {
                "type": "tintTritone", "blackR": 0, "blackG": 0, "blackB": 0,
                "whiteR": 1, "whiteG": 1, "whiteB": 1, "amount": 100
            }
        }]);
        set_scripts(&mut document, &[(effect_target(17, parameter), code)]);
        let source = archive(directory.path(), &document);
        let original = std::fs::read(&source).unwrap();
        let (diagnostics, project) = export(&source);
        assert_eq!(std::fs::read(&source).unwrap(), original);
        if baked {
            assert_written(&diagnostics, 1);
            let keys = effect_keys(clip_at(sequence(&project), 500), "Amount to Tint");
            assert_on_curve(&keys, |ms| 20.0 + 20.0 * ms / 1000.0, 1e-6);
        } else {
            assert!(reasons(&diagnostics)
                .iter()
                .any(|reason| reason.contains("0 of 1 scripts")));
            assert!(clip_at(sequence(&project), 500).effects.is_empty());
            if parameter == "blackR" {
                assert!(reasons(&diagnostics)
                    .iter()
                    .any(|reason| reason.contains("native colour keys couple three channels")));
            }
        }
    }
}

/// A script on one tile field of a Replicate's `motionTile` stays a script:
/// a Count keys all four tile fields, and the unbaked effect is omitted.
#[cfg(feature = "ffmpeg-library")]
#[test]
fn replicate_tile_scripts_keep_their_scripts() {
    let directory = tempfile::tempdir().unwrap();
    let mut document = video_fixture();
    document["composition"]["dynamics"] = json!({"entries": []});
    document["composition"]["layers"][0]["effects"] = json!([{
        "id": 17, "enabled": true, "effect": {
            "type": "motionTile", "tileCenterX": 0.25, "tileCenterY": 0.25,
            "tileWidth": 50, "tileHeight": 50, "outputWidth": 100, "outputHeight": 100,
            "mirrorEdges": false, "phase": 0
        }
    }]);
    set_scripts(
        &mut document,
        &[(effect_target(17, "tileWidth"), "return 25;")],
    );
    let source = archive(directory.path(), &document);
    let (diagnostics, project) = export(&source);
    let found = reasons(&diagnostics);
    assert!(
        found.iter().any(|reason| reason.contains("0 of 1 scripts")),
        "{found:?}"
    );
    assert!(
        found
            .iter()
            .any(|reason| reason.contains("native Replicate Count keys couple four tile fields")),
        "{found:?}"
    );
    assert!(clip_at(sequence(&project), 500).effects.is_empty());
}

/// Scripts on the two outputs of a Levels in Invert's form key one Invert
/// Blend With Original when the output black is the output white's
/// complement, as the writer exports such keys; other output scripts key
/// the Levels outputs.
#[cfg(feature = "ffmpeg-library")]
#[test]
fn complementary_invert_outputs_key_blend_with_original() {
    let white = "return 255 * (0.3 + 0.5 * Math.sin(input.time.seconds * Math.PI / 3));";
    let white_at = |ms: f64| 255.0 * (0.3 + 0.5 * (ms / 1000.0 * PI / 3.0).sin());
    for (black, invert) in [
        (
            "return 255 - 255 * (0.3 + 0.5 * Math.sin(input.time.seconds * Math.PI / 3));",
            true,
        ),
        ("return 40 + 20 * Math.sin(input.time.seconds * 2);", false),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let (archive, mut document) = imported(directory.path(), "feature_invert_strict.prproj");
        // Effect 3, on the clip at 5 s, is the fixture's keyed Invert.
        set_scripts(
            &mut document,
            &[
                (effect_target(3, "outputWhite"), white),
                (effect_target(3, "outputBlack"), black),
            ],
        );
        let source = rescripted(&archive, &document);
        let (diagnostics, project) = export(&source);
        assert_written(&diagnostics, 2);
        let clip = clip_at(sequence(&project), 5000);
        let params = &clip.effects[0].params;
        if invert {
            assert!(
                matches!(params, crate::schema::PrEffectParams::Invert(_)),
                "{params:?}"
            );
            // The output white's share of 255 levels, in percent.
            assert_on_curve(
                &effect_keys(clip, "Blend With Original"),
                |ms| white_at(ms) * 20.0 / 51.0,
                1e-9,
            );
        } else {
            assert!(
                matches!(params, crate::schema::PrEffectParams::Levels(_)),
                "{params:?}"
            );
            assert_on_curve(
                &effect_keys(clip, "(RGB) White Output Level"),
                |ms| white_at(ms).round(),
                1e-9,
            );
            assert_on_curve(
                &effect_keys(clip, "(RGB) Black Output Level"),
                |ms| (40.0 + 20.0 * (ms / 1000.0 * 2.0).sin()).round(),
                1e-9,
            );
        }
    }
}

/// The fixture's video inside a nest group that starts at 1 s, itself
/// starting 500 ms into the group.
fn nested_fixture() -> Value {
    let mut raw = video_fixture();
    raw["composition"]["dynamics"] = json!({"entries": []});
    let mut video = raw["composition"]["layers"][0].take();
    video["playback"] = crate::test_support::linear_playback(
        json!({"start": 500, "duration": 2000}),
        json!({"start": 1000, "duration": 2000}),
    );
    video["sourceRange"] = json!({"start": 1000, "duration": 2000});
    video["parent"] = json!(10);
    raw["composition"]["layers"][0] = json!({
        "type": "Group", "id": 10, "name": "Nest", "blendMode": "normal",
        "playback": crate::test_support::linear_playback(json!({"start": 1000, "duration": 2500}), json!({"start": 0, "duration": 2500})),
        "transform": {
            "anchorPoint": [960, 540], "position": [960, 540], "scale": [100, 100],
            "rotation": 0, "opacity": 100
        },
        "layers": [video]
    });
    raw
}

/// The project that the public entry writes for `source`, before its XML:
/// the same preparation, inspection and writer, of the fixture's one video.
#[cfg(feature = "ffmpeg-library")]
fn written_model(source: &Path) -> PrProjectFile {
    let file = TesseractFile::open(source).unwrap();
    let asset = file.asset("premiere-video-1").unwrap();
    let media = BTreeMap::from([(
        "premiere-video-1".to_owned(),
        crate::media::MediaFacts::Video(
            crate::media::inspect_video_media(
                asset.open().unwrap(),
                asset.open().unwrap(),
                asset.descriptor().byte_length,
            )
            .unwrap(),
        ),
    )]);
    let mut omissions = Vec::new();
    let baked = bake_scripts(file.project(), &mut omissions).unwrap();
    let exported = crate::convert::export_document(
        baked.document(),
        &media,
        &BTreeMap::new(),
        &BTreeMap::new(),
        crate::FrameRate::Fps30,
        &mut omissions,
    )
    .unwrap();
    exported.project
}

/// A nest placement and a clip inside it each take keys on their own clock:
/// the group's from its start, the clip's from its source in-point.
#[cfg(feature = "ffmpeg-library")]
#[test]
fn nested_owners_keep_owner_local_clocks_through_their_offsets() {
    let rotation_script = "return 30 * Math.sin(input.time.seconds * 3);";
    let clip_rotation = |nest: &crate::schema::PrNestOccurrence| {
        let clip = nest.sequence.video_occurrences().next().unwrap();
        assert_eq!(
            (clip.start_ticks, clip.in_ticks),
            (500 * TICKS_PER_MILLISECOND, 1000 * TICKS_PER_MILLISECOND)
        );
        let rotation = local(
            scalar(&clip.animations, |a| {
                matches!(a, PrPropertyAnimation::Rotation(_))
            }),
            clip.in_ticks,
        );
        assert_eq!(rotation.last().unwrap().0, 2000.0);
        assert_on_curve(&rotation, |ms| 30.0 * (ms / 1000.0 * 3.0).sin(), 1e-9);
    };

    // Keys of a clip inside a nest read back.
    let directory = tempfile::tempdir().unwrap();
    let mut raw = nested_fixture();
    set_scripts(&mut raw, &[(layer_target(1, "rotation"), rotation_script)]);
    let (diagnostics, project) = export(&archive(directory.path(), &raw));
    assert_written(&diagnostics, 1);
    let nest = sequence(&project).nest_occurrences().next().unwrap();
    assert!(nest.animations.is_empty());
    clip_rotation(nest);

    // The group's own Opacity and effect keys count from its start.
    let directory = tempfile::tempdir().unwrap();
    raw["composition"]["layers"][0]["effects"] = json!([
        {"id": 30, "enabled": true, "effect": {"type": "gaussianBlur", "blurriness": 5.0}}
    ]);
    set_scripts(
        &mut raw,
        &[
            (
                layer_target(10, "opacity"),
                "return 50 + 40 * Math.cos(input.time.seconds * 2);",
            ),
            (
                effect_target(30, "blurriness"),
                "return 10 + 8 * Math.sin(input.time.seconds * 2.5);",
            ),
        ],
    );
    let source = archive(directory.path(), &raw);
    // The crate's reader omits nest placements with effects, so the written
    // project cannot be read back; check the model that the public entry writes.
    let check = directory.path().join("check");
    let diagnostics = crate::tesseract_to_premiere(&source, &check, true).unwrap();
    assert_written(&diagnostics, 3);
    let project = written_model(&source);
    let nest = sequence(&project).nest_occurrences().next().unwrap();
    assert_eq!(
        (nest.start_ticks, nest.in_ticks),
        (1000 * TICKS_PER_MILLISECOND, 0)
    );
    let opacity = local(
        scalar(&nest.animations, |a| {
            matches!(a, PrPropertyAnimation::Opacity(_))
        }),
        0,
    );
    assert_eq!(opacity.last().unwrap().0, 2500.0);
    assert_on_curve(&opacity, |ms| 50.0 + 40.0 * (ms / 1000.0 * 2.0).cos(), 1e-9);
    let blur = local(
        nest.effects[0].animations[0].keys.scalar().unwrap(),
        nest.in_ticks,
    );
    assert_on_curve(
        &blur,
        |ms| (10.0 + 8.0 * (ms / 1000.0 * 2.5).sin()) / BLURRINESS_PER_AMOUNT,
        1e-9,
    );
    clip_rotation(nest);
}

/// A nest whose only clip is an adjustment layer exports, so the group's
/// scripts bake and are written.
#[test]
fn a_nest_of_one_adjustment_layer_takes_its_group_keys() {
    let directory = tempfile::tempdir().unwrap();
    let mut raw = nested_fixture();
    raw["composition"]["layers"][0]["layers"] = json!([{
        "type": "Adjustment", "id": 12, "name": "Adjust", "parent": 10, "blendMode": "normal",
        "activeRange": {"start": 0, "duration": 2500},
        "transform": {"anchorPoint": [0, 0], "position": [0, 0], "scale": [100, 100], "rotation": 0, "opacity": 100},
        "effects": [{"id": 31, "enabled": true, "effect": {"type": "gaussianBlur", "blurriness": 20.0}}]
    }]);
    set_scripts(
        &mut raw,
        &[
            (
                layer_target(10, "opacity"),
                "return 50 + 40 * Math.cos(input.time.seconds * 2);",
            ),
            (
                layer_target(12, "opacity"),
                "return 50 + 30 * Math.sin(input.time.seconds * 2);",
            ),
            (
                effect_target(31, "blurriness"),
                "return 20 + 10 * Math.sin(input.time.seconds);",
            ),
        ],
    );
    let source = archive(directory.path(), &raw);
    let diagnostics =
        crate::tesseract_to_premiere(&source, directory.path().join("check"), true).unwrap();
    assert_written(&diagnostics, 3);
}

/// A layer that export visits fails the export as it does without scripts,
/// whatever the blend mode of the group that holds it.
#[test]
fn a_visited_invalid_layer_fails_the_export_alike_with_or_without_scripts() {
    let export = |blend_mode: &str, with_scripts: bool| {
        let mut raw = video_fixture();
        if !with_scripts {
            raw["composition"]["dynamics"] = json!({"entries": []});
        }
        // A legacy video without its source range, which the writer rejects
        // when it reaches it.
        raw["composition"]["layers"].as_array_mut().unwrap().insert(0, json!({
            "type": "Group", "id": 20, "name": "Group", "blendMode": blend_mode,
            "playback": crate::test_support::linear_playback(json!({"start": 0, "duration": 1000}), json!({"start": 0, "duration": 1000})),
            "transform": {"anchorPoint": [960, 540], "position": [960, 540], "scale": [100, 100], "rotation": 0, "opacity": 100},
            "layers": [{
                "type": "Media", "id": 21, "name": "Legacy", "parent": 20,
                "activeRange": {"start": 0, "duration": 1000},
                "source": {"assetId": "premiere-video-1", "kind": "video"},
                "transform": {"anchorPoint": [0, 0], "position": [0, 0], "scale": [100, 100], "rotation": 0, "opacity": 100}
            }]
        }));
        let directory = tempfile::tempdir().unwrap();
        let source = archive(directory.path(), &raw);
        crate::tesseract_to_premiere(&source, directory.path().join("check"), true)
            .map_err(|error| error.to_string())
    };
    // A Multiply group exports as a nest with its blend, as a Normal one
    // does, so both reach the legacy video.
    for blend_mode in ["normal", "multiply"] {
        let error = export(blend_mode, false).unwrap_err();
        assert!(error.contains("sourceRange"), "{blend_mode}: {error}");
        assert_eq!(export(blend_mode, true).unwrap_err(), error, "{blend_mode}");
    }
}

/// A failed axis keeps both of its control's scripts, and only the failing
/// one reports its error; Scale X leads a uniform Scale whatever the entry
/// order.
#[test]
fn paired_scripts_report_the_failing_axis_and_follow_axis_order() {
    let mut raw = video_fixture();
    raw["composition"]["dynamics"] = json!({"entries": []});
    set_scripts(
        &mut raw,
        &[
            (layer_target(1, "positionX"), "throw new Error('x failed');"),
            (
                layer_target(1, "positionY"),
                "return 540 + 10 * input.time.seconds;",
            ),
            (
                layer_target(1, "scaleY"),
                "return 40 + 10 * input.time.seconds;",
            ),
            (
                layer_target(1, "scaleX"),
                "return 40 + 10 * input.time.seconds + 0.001 * Math.sin(input.time.seconds * 7);",
            ),
        ],
    );
    let document = EditableFxCompositionDocument::from_json_value(raw).unwrap();
    let mut omissions = Vec::new();
    let baked = bake_scripts(&document, &mut omissions).unwrap();
    let reasons = reasons_of(&omissions);
    assert!(reasons.iter().any(|reason| reason.starts_with("1 JS animation script(s) kept their animator, which export does not convert: the script failed: layer 1 positionX (the script failed at layer time 0 ms: Error: x failed")), "{reasons:#?}");
    assert!(reasons.contains(&"1 JS animation script(s) kept their animator, which export does not convert: Premiere keys Position as one point, and the other axis's script is not baked: layer 1 positionY".to_owned()), "{reasons:#?}");
    let keys = |property: PropType| -> Vec<(f64, f64)> {
        let target = PropertyTarget::layer(fx_schema::LayerId::new(1), property);
        baked
            .document()
            .composition()
            .dynamics()
            .entries()
            .iter()
            .find(|entry| entry.target == target)
            .and_then(|entry| entry.animator.keyframe_track())
            .unwrap_or_else(|| panic!("{property}"))
            .keyframes()
            .iter()
            .map(|key| {
                let PropertyValue::Float(value) = key.value() else {
                    panic!("{key:?}")
                };
                (key.layer_time().as_millis() as f64 / 1000.0, *value)
            })
            .collect()
    };
    // Both axes take Scale X's samples, the leading axis's.
    let scale = keys(PropType::ScaleX);
    assert_eq!(keys(PropType::ScaleY), scale);
    for &(t, value) in &scale {
        assert!((value - (40.0 + 10.0 * t + 0.001 * (7.0 * t).sin())).abs() < 1e-9);
    }
    assert!(scale
        .iter()
        .any(|&(t, value)| (value - (40.0 + 10.0 * t)).abs() > 1e-6));
}

/// An evaluation thread's native stack follows the longest script source,
/// from the floor up to 1 GiB at the source bound.
#[test]
fn evaluation_stacks_follow_the_longest_source() {
    assert_eq!(sample::stack_bytes(0), Some(16 << 20));
    assert_eq!(sample::stack_bytes(512), Some(16 << 20));
    assert_eq!(sample::stack_bytes(4096), Some(128 << 20));
    assert_eq!(sample::stack_bytes(32 * 1024), Some(1 << 30));
    assert_eq!(
        sample::stack_bytes(32 * 1024 + 1),
        Some((1 << 30) + (32 << 10))
    );
    assert_eq!(sample::stack_bytes(usize::MAX), None);
}

/// Graphic Position and text Rotation have no verified Bezier speeds, so
/// their scripts become Linear and Hold keys; text Opacity and the uniform
/// Scales of text and Vector Motion keep Bezier.
#[cfg(feature = "ffmpeg-library")]
#[test]
fn graphic_parameters_without_verified_bezier_take_linear_keys() {
    let directory = tempfile::tempdir().unwrap();
    let (archive, mut document) = imported(
        directory.path(),
        "feature_graphic_transform_keys_26_5_strict.prproj",
    );
    let smooth = "return 480 + 200 * Math.sin(input.time.seconds * 1.5);";
    let text_scale = "return 100 + 30 * Math.sin(input.time.seconds * 2);";
    let group_scale = "const t = input.time.seconds; return 80 + 40 * t * t / 9;";
    set_scripts(
        &mut document,
        &[
            (layer_target(2, "positionX"), smooth),
            (
                layer_target(2, "positionY"),
                "return 540 + 80 * Math.cos(input.time.seconds * 1.5);",
            ),
            (
                layer_target(2, "rotation"),
                "return 20 * Math.sin(input.time.seconds * 2);",
            ),
            (
                layer_target(2, "opacity"),
                "return 50 + 40 * Math.sin(input.time.seconds * 2);",
            ),
            (layer_target(3, "positionX"), smooth),
            (
                layer_target(3, "positionY"),
                "return 540 + 50 * input.time.seconds * input.time.seconds;",
            ),
            (layer_target(2, "scaleX"), text_scale),
            (layer_target(2, "scaleY"), text_scale),
            (layer_target(3, "scaleX"), group_scale),
            (layer_target(3, "scaleY"), group_scale),
        ],
    );
    let source = rescripted(&archive, &document);
    let (diagnostics, project) = export(&source);
    assert_written(&diagnostics, 10);
    let crate::format::PrVideoItem::Graphic(graphic) = sequence(&project)
        .video_items()
        .find(|item| matches!(item, crate::format::PrVideoItem::Graphic(_)))
        .unwrap()
    else {
        unreachable!()
    };
    let crate::schema::text::PrGraphicObject::Text(text) = &graphic.objects[0] else {
        unreachable!()
    };
    let cubic = |animations: &[PrPropertyAnimation], select: fn(&PrPropertyAnimation) -> bool| {
        let animation = animations
            .iter()
            .find(|animation| select(animation))
            .unwrap();
        match animation {
            PrPropertyAnimation::Position(keys) => keys.iter().any(|key| {
                matches!(
                    key.easing,
                    crate::schema::PrKeyframeEasing::CubicBezier { .. }
                )
            }),
            _ => animation.scalar_keys().unwrap().iter().any(|key| {
                matches!(
                    key.easing,
                    crate::schema::PrKeyframeEasing::CubicBezier { .. }
                )
            }),
        }
    };
    let motion = &graphic.vector_motion.as_ref().unwrap().animations;
    assert!(!cubic(&text.animations, |a| matches!(
        a,
        PrPropertyAnimation::Position(_)
    )));
    assert!(!cubic(&text.animations, |a| matches!(
        a,
        PrPropertyAnimation::Rotation(_)
    )));
    assert!(cubic(&text.animations, |a| matches!(
        a,
        PrPropertyAnimation::Opacity(_)
    )));
    assert!(!cubic(motion, |a| matches!(
        a,
        PrPropertyAnimation::Position(_)
    )));
    for (animations, expected) in [
        (
            &text.animations,
            (&|ms: f64| 100.0 + 30.0 * (ms / 1000.0 * 2.0).sin()) as &dyn Fn(f64) -> f64,
        ),
        (motion, &|ms: f64| 80.0 + 40.0 * (ms / 1000.0).powi(2) / 9.0),
    ] {
        assert!(cubic(animations, |a| matches!(
            a,
            PrPropertyAnimation::UniformScale(_)
        )));
        let scale = scalar(animations, |a| {
            matches!(a, PrPropertyAnimation::UniformScale(_))
        });
        assert_on_curve(&local(scale, graphic.in_ticks), expected, 1e-9);
    }
}

/// Authored keys keep their export; a pair that one native track cannot
/// hold keeps its script, reported with the reason.
#[cfg(feature = "ffmpeg-library")]
#[test]
fn authored_keys_are_unchanged_and_unrepresentable_pairs_keep_their_scripts() {
    let directory = tempfile::tempdir().unwrap();
    let mut raw = video_fixture();
    let authored = |values: [f64; 2]| {
        json!({"type": "keyframes", "enabled": true, "keyframes": [
            {"id": format!("a{}", values[0]), "layerTime": 0, "value": {"type": "float", "value": values[0]}, "easing": {"type": "linear"}},
            {"id": format!("b{}", values[1]), "layerTime": 2000, "value": {"type": "float", "value": values[1]}, "easing": {"type": "linear"}}
        ]})
    };
    raw["composition"]["dynamics"] = json!({"entries": [
        {"target": layer_target(1, "opacity"), "animator": authored([30.0, 90.0])},
        {"target": layer_target(1, "positionY"), "animator": authored([400.0, 600.0])},
    ]});
    set_scripts(
        &mut raw,
        &[
            (
                layer_target(1, "rotation"),
                "return 10 * input.time.seconds;",
            ),
            (
                layer_target(1, "positionX"),
                "return 900 + 10 * input.time.seconds;",
            ),
            (layer_target(1, "scaleX"), "return 40 + input.time.seconds;"),
        ],
    );
    let source = archive(directory.path(), &raw);
    let (diagnostics, project) = export(&source);
    let reasons = reasons(&diagnostics);
    assert!(reasons.contains(&"1 JS animation script(s) kept their animator, which export does not convert: Premiere keys Position as one point, and the other axis has authored animation: layer 1 positionX"), "{reasons:#?}");
    assert!(reasons.contains(&"1 JS animation script(s) kept their animator, which export does not convert: Premiere Scale is uniform, and the other axis is static: layer 1 scaleX"), "{reasons:#?}");
    assert_written(&diagnostics, 1);
    let clip = sequence(&project).video_occurrences().next().unwrap();
    let opacity = local(
        scalar(&clip.animations, |a| {
            matches!(a, PrPropertyAnimation::Opacity(_))
        }),
        clip.in_ticks,
    );
    assert_eq!(opacity, [(0.0, 30.0), (2000.0, 90.0)]);
    assert!(clip
        .animations
        .iter()
        .any(|a| matches!(a, PrPropertyAnimation::Rotation(_))));

    // Two Scale scripts that differ are not one uniform Scale.
    let mut raw = video_fixture();
    raw["composition"]["dynamics"] = json!({"entries": []});
    set_scripts(
        &mut raw,
        &[
            (layer_target(1, "scaleX"), "return 40 + input.time.seconds;"),
            (
                layer_target(1, "scaleY"),
                "return 40 + 2 * input.time.seconds;",
            ),
        ],
    );
    let document = EditableFxCompositionDocument::from_json_value(raw).unwrap();
    let mut omissions = Vec::new();
    let baked = bake_scripts(&document, &mut omissions).unwrap();
    assert!(matches!(baked.document, Cow::Borrowed(_)));
    let reasons = reasons_of(&omissions);
    assert!(reasons.iter().any(|reason| reason.starts_with(
        "1 JS animation script(s) kept their animator, which export does not convert: Premiere Scale is uniform, but the Scale axes differ: layer 1 scaleY (Premiere Scale is uniform, but Scale Y leaves the Scale X curve at layer time"
    )), "{reasons:#?}");
    assert!(reasons.contains(&"1 JS animation script(s) kept their animator, which export does not convert: Premiere Scale is uniform, and the other axis's script is not baked: layer 1 scaleX".to_owned()), "{reasons:#?}");
}

fn reasons_of(omissions: &[Omission]) -> Vec<String> {
    omissions.iter().map(|item| item.reason.clone()).collect()
}

/// Bakes the one-video fixture with `code` as its only (Opacity) script.
fn bake_opacity(code: &str) -> (Vec<Omission>, bool) {
    let mut raw = video_fixture();
    raw["composition"]["dynamics"] = json!({"entries": []});
    set_scripts(&mut raw, &[(layer_target(1, "opacity"), code)]);
    let document = EditableFxCompositionDocument::from_json_value(raw).unwrap();
    let mut omissions = Vec::new();
    let baked = bake_scripts(&document, &mut omissions).unwrap();
    let kept = baked.document().composition().dynamics().entries()[0]
        .animator
        .is_js_script();
    (omissions, kept)
}

/// Failed, non-finite, non-scalar and history-dependent scripts keep their
/// animator: nothing becomes a static or truncated track.
#[test]
fn failing_and_history_dependent_scripts_keep_their_animator() {
    for (code, reason) in [
        ("return NaN;", "the script did not return a finite number: layer 1 opacity (the script did not return a finite number at layer time 0 ms)"),
        ("return [1, 2];", "the script did not return a finite number"),
        ("return input.time.seconds > 1 ? Infinity : 50;", "the script did not return a finite number at layer time 1001 ms"),
        ("throw new Error('failed');", "the script failed: layer 1 opacity (the script failed at layer time 0 ms: Error: failed (unknown at :9:7))"),
        // Loop-iteration policy was removed; nontermination now requires
        // caller-owned process termination, not a recoverable VM error.
        // Keep loop-body failure coverage without relying on that old cap.
        ("for (let i = 0; i < 10; i++) { if (i === 9) throw new Error('loop failed'); }", "the script failed"),
        ("return input.time + 1;", "the script failed"),
        // Ascending samples are 1, 2, 3, ...; a fresh realm's descending
        // probe returns 1 at the window end.
        ("globalThis.n = (globalThis.n || 0) + 1; return Math.min(globalThis.n, 100);", "the script depends on evaluation history: layer 1 opacity (the script depends on evaluation history: a fresh reverse-order evaluation returned 1 instead of 100 at layer time 3000 ms)"),
        // Remembers the previous call's time.
        ("const t = input.time.milliseconds; const up = globalThis.last === undefined || t > globalThis.last; globalThis.last = t; return up ? 60 : 40;", "the script depends on evaluation history"),
        ("return 50 + Math.random();", "the script depends on evaluation history"),
        ("const t = input.time.seconds; return 50 + 60 * Math.exp(-2 * t) * Math.cos(9 * t);", "the script leaves the value range that export writes"),
    ] {
        let (omissions, kept) = bake_opacity(code);
        assert!(kept, "{code}");
        let reasons = reasons_of(&omissions);
        assert!(reasons.iter().any(|item| item.contains(reason)), "{code}: {reasons:#?}");
        assert!(reasons.iter().any(|item| item.starts_with("JS animation baking: 0 of 1 scripts")));
    }
}

/// Scripts that export cannot write as keys are reported by reason, without
/// a single evaluation.
#[test]
fn unsupported_scripts_are_reported_by_reason_without_evaluation() {
    type Edit = fn(&mut Value);
    let video = |edit: fn(&mut Value)| edit;
    let cases: &[(&str, Edit)] = &[
        (
            "legacy or mixed script clocks need the runtime's migration",
            video(|raw| {
                raw["composition"]["dynamics"]["entries"][0]["animator"] =
                    json!({"type": "jsScript", "code": "return 1;"});
            }),
        ),
        (
            "unknown script fields may change what the script means",
            |raw| {
                raw["composition"]["dynamics"]["entries"][0]["animator"]["futureMode"] =
                    json!(false);
            },
        ),
        (
            "script dependencies need graph evaluation, which no public crate provides",
            |raw| {
                raw["composition"]["dynamics"]["entries"][0]["dependencies"] =
                    json!([layer_target(1, "rotation")]);
                raw["composition"]["dynamics"]["entries"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({
                        "target": layer_target(1, "rotation"),
                        "animator": {"type": "constant", "value": {"type": "float", "value": 1.0}}
                    }));
            },
        ),
        (
            "script layer references need live resource context",
            |raw| {
                raw["composition"]["dynamics"]["entries"][0]["layerRefs"] =
                    json!({"other": {"layerId": 1}});
            },
        ),
        (
            "randomSeedTarget names a target outside the native key bindings",
            |raw| {
                raw["composition"]["dynamics"]["entries"][0]["randomSeedTarget"] =
                    layer_target(1, "rotationY");
            },
        ),
        ("rotationY has no native key binding on this owner", |raw| {
            raw["composition"]["dynamics"]["entries"][0]["target"] = layer_target(1, "rotationY");
        }),
        ("positionZ has no native key binding on this owner", |raw| {
            raw["composition"]["dynamics"]["entries"][0]["target"] = layer_target(1, "positionZ");
        }),
        (
            "the owner or an enclosing group remaps its clock with playback",
            |raw| {
                raw["composition"]["layers"][0]["playback"] =
                    crate::test_support::remapped_playback(
                        crate::test_support::layer_range(&raw["composition"]["layers"][0]).clone(),
                        json!({
                            "keyframes": [
                                {"id": "start", "time": 500, "value": 1000, "easing": {"type": "linear"}},
                                {"id": "end", "time": 3500, "value": 4000, "easing": {"type": "linear"}}
                            ], "before": "inactive", "after": "inactive"
                        }),
                    );
            },
        ),
        (
            "Posterize Time on the owner or an enclosing group holds its clock",
            |raw| {
                raw["composition"]["layers"][0]["effects"] = json!([{"id": 7, "enabled": true, "effect": {"type": "posterizeTime", "frameRate": 12}}]);
            },
        ),
        ("vignette has no Premiere effect mapping", |raw| {
            raw["composition"]["layers"][0]["effects"] =
                json!([{"id": 7, "enabled": true, "effect": {"type": "vignette", "amount": 0.5}}]);
            raw["composition"]["dynamics"]["entries"][0]["target"] = effect_target(7, "amount");
        }),
        (
            "graphic shapes export without keys; a keyed shape omits its graphic",
            |raw| {
                raw["composition"]["layers"].as_array_mut().unwrap().insert(0, json!({
                "type": "Shape", "id": 5, "name": "Shape", "activeRange": {"start": 0, "duration": 1000},
                "transform": {"anchorPoint": [0, 0], "position": [0, 0], "scale": [100, 100], "rotation": 0, "opacity": 100},
                "shape": {"path": {"commands": [{"type": "moveTo", "x": 0, "y": 0}, {"type": "lineTo", "x": 10, "y": 0}, {"type": "close"}]}}
            }));
                raw["composition"]["dynamics"]["entries"][0]["target"] = layer_target(5, "opacity");
            },
        ),
        (
            "a group without a video or adjustment layer exports no nested sequence",
            |raw| {
                raw["composition"]["layers"].as_array_mut().unwrap().insert(0, json!({
                "type": "Group", "id": 6, "name": "Empty", "blendMode": "normal",
                "playback": crate::test_support::linear_playback(json!({"start": 0, "duration": 1000}), json!({"start": 0, "duration": 1000})),
                "transform": {"anchorPoint": [0, 0], "position": [0, 0], "scale": [100, 100], "rotation": 0, "opacity": 100},
                "layers": [{
                    "type": "Rect", "id": 7, "name": "Solid", "parent": 6, "activeRange": {"start": 0, "duration": 1000},
                    "transform": {"anchorPoint": [0, 0], "position": [0, 0], "scale": [100, 100], "rotation": 0, "opacity": 100},
                    "rect": {"size": [10, 10], "fillColor": [1, 0, 0, 1]}
                }]
            }));
                raw["composition"]["dynamics"]["entries"][0]["target"] = layer_target(6, "opacity");
            },
        ),
        (
            "a Color Matte exports without keys; an animated one is omitted",
            |raw| {
                raw["composition"]["dynamics"]["entries"][0]["target"] = layer_target(2, "opacity");
            },
        ),
        (
            "mask and layer-style properties have no native keys",
            |raw| {
                raw["composition"]["dynamics"]["entries"][0]["target"] =
                    json!({"kind": "fxItemProperty", "itemId": 9, "propertyName": "opacity"});
            },
        ),
    ];
    for (reason, edit) in cases {
        let mut raw = video_fixture();
        raw["composition"]["dynamics"]["entries"]
            .as_array_mut()
            .unwrap()
            .truncate(1);
        edit(&mut raw);
        let Ok(document) = EditableFxCompositionDocument::from_json_value(raw.clone()) else {
            panic!("{reason}: the edited fixture is not a valid document: {raw}");
        };
        let mut omissions = Vec::new();
        let baked = bake_scripts(&document, &mut omissions).unwrap();
        assert!(matches!(baked.document, Cow::Borrowed(_)), "{reason}");
        let reasons = reasons_of(&omissions);
        assert!(
            reasons.iter().any(|item| item.contains(reason)),
            "{reason}: {reasons:#?}"
        );
        assert!(
            reasons.iter().any(|item| item.contains("0 of 1 scripts became editable keys (0 keys, from at most 0 JavaScript evaluations)")),
            "{reason}: {reasons:#?}"
        );
    }
}

/// Aggregate budgets stop the export before publication; a track over the
/// native key limit keeps its script. Nothing is truncated.
#[cfg(feature = "ffmpeg-library")]
#[test]
fn budgets_stop_the_export_without_the_former_track_key_quota() {
    let document = EditableFxCompositionDocument::from_json_value(video_fixture()).unwrap();
    for (limits, reason) in [
        (
            Limits {
                evaluations: 6129,
                ..LIMITS
            },
            "need at least 6130 JavaScript evaluations, more than the 6129",
        ),
        (Limits { keys: 3, ..LIMITS }, "more than the 3 keys"),
        (
            Limits {
                elapsed: Duration::ZERO,
                ..LIMITS
            },
            "evaluation reached the elapsed-time bound of 0 s",
        ),
    ] {
        let error = bake(&document, &mut Vec::new(), &limits)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("script bake budget exceeded") && error.contains(reason),
            "{error}"
        );
        assert!(error.contains("nothing was published"), "{error}");
    }
    // Extreme windows overflow the evaluation count of one track, its
    // probes, a pair or the batch total: admission fails without wrapping,
    // before any worker starts.
    for (duration, properties) in [
        (u64::MAX, &["opacity"][..]),
        (u64::MAX - 1, &["opacity"]),
        (u64::MAX / 2, &["positionX", "positionY"]),
        (u64::MAX / 2, &["opacity", "rotation"]),
    ] {
        let mut raw = video_fixture();
        // Text retains an unrestricted authored activeRange, so these
        // arithmetic probes reach budget admission before any worker runs.
        raw["composition"]["layers"][0] = json!({
            "type":"Text", "id":1, "name":"Budget owner",
            "activeRange":{"start":0,"duration":duration},
            "transform":raw["composition"]["layers"][0]["transform"],
            "sourceText":{"text":"Budget", "fontFamily":"Inter", "fontStyle":"Bold", "fontSize":80, "fillColor":[1,1,1,1]}
        });
        raw["composition"]["dynamics"] = json!({"entries": []});
        let scripts: Vec<_> = properties
            .iter()
            .map(|property| (layer_target(1, property), "return 50;"))
            .collect();
        set_scripts(&mut raw, &scripts);
        let document = EditableFxCompositionDocument::from_json_value(raw).unwrap();
        let error = bake_scripts(&document, &mut Vec::new())
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("script bake budget exceeded"),
            "{duration} {properties:?}: {error}"
        );
    }
    // A key every millisecond crosses the former 4,096-key policy.
    let mut raw = video_fixture();
    raw["composition"]["layers"][0]["playback"] = crate::test_support::linear_playback(
        json!({"start": raw["composition"]["layers"][0]["playback"]["inputRange"]["start"], "duration": 4100}),
        json!({"start": raw["composition"]["layers"][0]["sourceRange"]["start"], "duration": 4100}),
    );
    raw["composition"]["layers"][0]["sourceRange"]["duration"] = json!(4100);
    raw["duration"] = json!(5.0);
    raw["composition"]["layers"][1]["activeRange"]["duration"] = json!(5000);
    raw["composition"]["dynamics"]["entries"]
        .as_array_mut()
        .unwrap()
        .truncate(1);
    raw["composition"]["dynamics"]["entries"][0]["animator"]["layerTimeJsCode"] =
        json!("return input.time.milliseconds % 2 ? 100 : 0;");
    let directory = tempfile::tempdir().unwrap();
    let source = archive(directory.path(), &raw);
    let (diagnostics, project) = export(&source);
    let animations = &sequence(&project)
        .video_occurrences()
        .next()
        .unwrap()
        .animations;
    assert!(
        animations.iter().any(|animation| matches!(animation,
            PrPropertyAnimation::Opacity(keys) if keys.len() == 4101
        )),
        "{diagnostics:#?}"
    );
}

#[test]
fn script_source_above_former_byte_quota_becomes_editable_keys() {
    let long = format!("return 1; //{}", " ".repeat(32 * 1024));
    let (omissions, kept) = bake_opacity(&long);
    assert!(!kept, "{omissions:?}");
    assert!(!reasons_of(&omissions)
        .iter()
        .any(|item| item.contains("script source exceeds")));
}

/// A baked track whose owner export omits is reported under that owner and
/// not counted as written.
#[cfg(feature = "ffmpeg-library")]
#[test]
fn baked_tracks_that_export_discards_are_reported_under_their_owner() {
    let directory = tempfile::tempdir().unwrap();
    let mut raw = video_fixture();
    // A subtracted mask has no Premiere Crop or Linear Wipe: export omits the clip.
    raw["composition"]["layers"][0]["masks"] = json!([{"id": 20, "mode": "subtract", "layer": 3}]);
    let mut other = raw["composition"]["layers"][0].clone();
    other["id"] = json!(4);
    other["name"] = json!("Other video");
    other.as_object_mut().unwrap().remove("masks");
    let layers = raw["composition"]["layers"].as_array_mut().unwrap();
    layers.insert(1, json!({
        "type": "Rect", "id": 3, "name": "Guide", "activeRange": {"start": 500, "duration": 3000},
        "transform": {"anchorPoint": [0, 0], "position": [0, 0], "scale": [100, 100], "rotation": 0, "opacity": 100},
        "rect": {"size": [1920, 1080], "fillColor": [0, 0, 0, 1]}
    }));
    layers.insert(2, other);
    let source = archive(directory.path(), &raw);
    let diagnostics =
        crate::tesseract_to_premiere(&source, directory.path().join("check"), true).unwrap();
    let discarded = diagnostics
        .iter()
        .find(|item| item.reason.starts_with("baked JS animation of"))
        .unwrap();
    assert_eq!(discarded.record, "layer 1 (\"Animated red video\")");
    assert_eq!(
        discarded.reason,
        "baked JS animation of layer 1 opacity, layer 1 rotation was not written: export did not write this animation; see this layer's other diagnostics"
    );
    assert!(reasons(&diagnostics)
        .contains(&"0 of 2 baked JS animation tracks were written as native keys"));
}

/// Without a script, export neither copies the document nor reports
/// anything, and writes what the writer alone writes.
#[cfg(feature = "ffmpeg-library")]
#[test]
fn documents_without_scripts_export_unchanged_without_a_copy() {
    let mut raw = video_fixture();
    raw["composition"]["dynamics"] = json!({"entries": [
        {"target": layer_target(1, "opacity"), "animator": {"type": "keyframes", "enabled": true, "keyframes": [
            {"id": "a", "layerTime": 0, "value": {"type": "float", "value": 30.0}, "easing": {"type": "linear"}},
            {"id": "b", "layerTime": 2000, "value": {"type": "float", "value": 90.0}, "easing": {"type": "linear"}}
        ]}}
    ]});
    let document = EditableFxCompositionDocument::from_json_value(raw.clone()).unwrap();
    let mut omissions = Vec::new();
    let baked = bake_scripts(&document, &mut omissions).unwrap();
    assert!(matches!(baked.document, Cow::Borrowed(borrowed) if std::ptr::eq(borrowed, &document)));
    assert!(omissions.is_empty());
    let directory = tempfile::tempdir().unwrap();
    let source = archive(directory.path(), &raw);
    let output = directory.path().join("export");
    assert!(crate::tesseract_to_premiere(&source, &output, false)
        .unwrap()
        .is_empty());
    let file = TesseractFile::open(&source).unwrap();
    let media = BTreeMap::from([(
        "premiere-video-1".to_owned(),
        crate::media::MediaFacts::Video(
            crate::media::inspect_video_media(
                file.asset("premiere-video-1").unwrap().open().unwrap(),
                file.asset("premiere-video-1").unwrap().open().unwrap(),
                file.asset("premiere-video-1")
                    .unwrap()
                    .descriptor()
                    .byte_length,
            )
            .unwrap(),
        ),
    )]);
    let unbaked = crate::convert::tesseract_to_premiere(
        file.project(),
        &media,
        &BTreeMap::new(),
        &BTreeMap::new(),
        crate::FrameRate::Fps30,
        &mut Vec::new(),
    )
    .unwrap();
    let (reloaded, _) = PrProjectFile::load(output.join("project.prproj")).unwrap();
    let native = |project: &PrProjectFile| {
        let clip = sequence(project).video_occurrences().next().unwrap();
        (
            clip.animations.clone(),
            clip.start_ticks,
            clip.in_ticks,
            clip.opacity,
        )
    };
    assert_eq!(native(&reloaded), native(&unbaked));
}

/// `input.randomSeed` is the runtime's seed of the target, or of its
/// `randomSeedTarget`, at the owner-local time (pinned runtime values).
#[test]
fn random_seeds_follow_the_runtime_seed_identity() {
    let seed_at = |target: PropertyTarget, time_ms| {
        fx_keyframe_bake::identity::random_seed(sample::seed_prefix(&target).unwrap(), time_ms)
    };
    let layer = fx_schema::LayerId::new(7);
    // fx_composition::script's `random_seed_is_stable_for_property_and_time`.
    assert_eq!(
        seed_at(PropertyTarget::layer(layer, PropType::PositionX), 0),
        3_792_147_118
    );
    assert_eq!(
        seed_at(PropertyTarget::layer(layer, PropType::PositionX), 100),
        3_792_097_914
    );
    assert_eq!(
        seed_at(
            PropertyTarget::effect_param(fx_schema::EffectId::new(101), "blurriness"),
            0
        ),
        3_639_593_753
    );
    for (property, at_zero, at_hundred) in [
        (PropType::Opacity, 3_793_316_757, 3_793_344_313),
        (PropType::Rotation, 3_792_219_658, 3_792_237_502),
    ] {
        assert_eq!(seed_at(PropertyTarget::layer(layer, property), 0), at_zero);
        assert_eq!(
            seed_at(PropertyTarget::layer(layer, property), 100),
            at_hundred
        );
    }
    // The runtime's append-only property seed ids of the other bound targets.
    let prefix = |property, id| {
        let mut hash = 0xcbf2_9ce4_8422_2325;
        fx_keyframe_bake::identity::hash_random_seed_part(&mut hash, 7);
        fx_keyframe_bake::identity::hash_random_seed_part(&mut hash, id);
        assert_eq!(
            sample::seed_prefix(&PropertyTarget::layer(layer, property)),
            Some(hash)
        );
    };
    prefix(PropType::PositionY, 1);
    prefix(PropType::ScaleX, 5);
    prefix(PropType::ScaleY, 6);
    prefix(PropType::AudioVolume, 30);
    assert_eq!(
        sample::seed_prefix(&PropertyTarget::layer(layer, PropType::PositionZ)),
        None
    );

    let mut raw = video_fixture();
    raw["composition"]["layers"][0]["id"] = json!(7);
    raw["composition"]["dynamics"] = json!({"entries": []});
    set_scripts(
        &mut raw,
        &[(layer_target(7, "rotation"), "return input.randomSeed;")],
    );
    raw["composition"]["dynamics"]["entries"][0]["randomSeedTarget"] = layer_target(7, "opacity");
    let document = EditableFxCompositionDocument::from_json_value(raw).unwrap();
    let baked = bake_scripts(&document, &mut Vec::new()).unwrap();
    let keys = baked.document().composition().dynamics().entries()[0]
        .animator
        .keyframe_track()
        .unwrap()
        .keyframes();
    assert_eq!(keys[0].value(), &PropertyValue::Float(3_793_316_757.0));
    assert!(keys.len() > 1000, "a seed changes every millisecond");
}

/// Sampling reaches every owner-local millisecond: a one-millisecond event
/// becomes keys, and a nonlinear body stays within tolerance at every sample.
#[test]
fn sampling_reaches_every_owner_millisecond_and_fits_nonlinear_bodies() {
    let deadline = Deadline::new(Duration::from_secs(60));
    let spike = sample::sample(
        "return input.time.milliseconds === 1234 ? 90 : 10;",
        0,
        3000,
        &deadline,
    )
    .unwrap();
    assert_eq!(spike.len(), 3001);
    let keys = fit::fit(
        &[Axis {
            values: &spike,
            cap: 0.1,
        }],
        &Rules {
            tolerance_cap: 0.1,
            cubic: true,
            scalar_keys: true,
            range: None,
        },
    )
    .unwrap();
    assert!(keys.iter().any(|key| key.offset_ms == 1234), "{keys:?}");
    fit::follows(&keys, &spike, &spike, 1e-9).unwrap();

    // A damped spring and an exponential ease, as motion-graphics scripts use.
    let body = "const t = input.time.seconds; const e = 1 - Math.exp(-5 * t); return 960 + 300 * e * Math.cos(7 * t) + 40 * Math.sin(23 * t);";
    let values = sample::sample(body, 0, 2410, &deadline).unwrap();
    for (ms, value) in (0_u32..).zip(&values) {
        let t = f64::from(ms) / 1000.0;
        let expected =
            960.0 + 300.0 * (1.0 - (-5.0 * t).exp()) * (7.0 * t).cos() + 40.0 * (23.0 * t).sin();
        assert!((value - expected).abs() < 1e-9);
    }
    let rules = Rules {
        tolerance_cap: 0.5,
        cubic: false,
        scalar_keys: true,
        range: None,
    };
    let keys = fit::fit(
        &[Axis {
            values: &values,
            cap: 0.5,
        }],
        &rules,
    )
    .unwrap();
    assert!(keys
        .iter()
        .all(|key| !matches!(key.easing, FittedEasing::Cubic { .. })));
    fit::follows(&keys, &values, &values, 0.5).unwrap();
    assert!(keys.len() < 400, "{}", keys.len());
    assert_eq!(sample::evaluations(2410), 2411 + 64);
}

/// Two different curves share one point track's key times and easing, each
/// within its own tolerance at every millisecond.
#[test]
fn paired_axes_share_keys_within_each_tolerance() {
    let deadline = Deadline::new(Duration::from_secs(60));
    let x = sample::sample(
        "return 960 + 300 * Math.sin(input.time.seconds * 2);",
        0,
        3000,
        &deadline,
    )
    .unwrap();
    let y = sample::sample(
        "const t = input.time.seconds; return t < 1.2 ? 540 : 540 + 200 * Math.pow(t - 1.2, 3);",
        0,
        3000,
        &deadline,
    )
    .unwrap();
    let rules = Rules {
        tolerance_cap: 0.5,
        cubic: true,
        scalar_keys: false,
        range: None,
    };
    let axes = [
        Axis {
            values: &x,
            cap: 0.5,
        },
        Axis {
            values: &y,
            cap: 0.5,
        },
    ];
    let keys = fit::fit(&axes, &rules).unwrap();
    fit::follows(&keys, &x, &x, 0.5).unwrap();
    fit::follows(&keys, &y, &y, 0.5).unwrap();
    assert!(keys.len() < 200, "{}", keys.len());
}

/// A cubic ramp into a jump becomes keys that Premiere's scalar keys hold:
/// no cubic arrival into a key that starts a Hold.
#[cfg(feature = "ffmpeg-library")]
#[test]
fn cubic_arrivals_into_holds_are_refitted_for_native_scalar_keys() {
    let code =
        "const t = input.time.seconds; return t < 1.5 ? 20 + 40 * Math.pow(t / 1.5, 2) : 90;";
    let directory = tempfile::tempdir().unwrap();
    let mut raw = video_fixture();
    raw["composition"]["dynamics"]["entries"][0]["animator"]["layerTimeJsCode"] = json!(code);
    let source = archive(directory.path(), &raw);
    let (diagnostics, project) = export(&source);
    assert_written(&diagnostics, 2);
    let clip = sequence(&project).video_occurrences().next().unwrap();
    let opacity = scalar(&clip.animations, |a| {
        matches!(a, PrPropertyAnimation::Opacity(_))
    });
    let jump = opacity
        .iter()
        .position(|key| key.easing == crate::schema::PrKeyframeEasing::Hold)
        .unwrap();
    assert!(!matches!(
        opacity[jump - 1].easing,
        crate::schema::PrKeyframeEasing::CubicBezier { .. }
    ));
    assert_on_curve(
        &local(opacity, clip.in_ticks),
        |ms| {
            let t = ms / 1000.0;
            if t < 1.5 {
                20.0 + 40.0 * (t / 1.5).powi(2)
            } else {
                90.0
            }
        },
        1e-9,
    );
}

/// The fixture's video, whose `sourceRect` is `source` pixels, on a
/// `canvas`-pixel document without animation, and an identity Corner Pin,
/// effect 40, on `owner`: the video (layer 1), a nest group around it (10)
/// or an adjustment layer above it (12).
fn corner_pin_document(owner: u64, canvas: [u32; 2], source: [u32; 2]) -> Value {
    let mut raw = if owner == 10 {
        nested_fixture()
    } else {
        video_fixture()
    };
    raw["dimensions"] = json!({"width": canvas[0], "height": canvas[1]});
    raw["composition"]["dynamics"] = json!({"entries": []});
    let layers = &mut raw["composition"]["layers"];
    let video = if owner == 10 {
        &mut layers[0]["layers"][0]
    } else {
        &mut layers[0]
    };
    video["source"]["sourceRect"] =
        json!({"x": 0, "y": 0, "width": source[0], "height": source[1]});
    layers[1]["rect"]["size"] = json!(canvas);
    let pin = json!([{"id": 40, "enabled": true, "effect": {
        "type": "cornerPin",
        "upperLeftX": 0, "upperLeftY": 0, "upperRightX": 1, "upperRightY": 0,
        "lowerLeftX": 0, "lowerLeftY": 1, "lowerRightX": 1, "lowerRightY": 1
    }}]);
    if owner == 12 {
        layers.as_array_mut().unwrap().insert(0, json!({
            "type": "Adjustment", "id": 12, "name": "Adjust", "blendMode": "normal",
            "activeRange": {"start": 0, "duration": 3000},
            "transform": {"anchorPoint": [0, 0], "position": [0, 0], "scale": [100, 100], "rotation": 0, "opacity": 100},
            "effects": pin
        }));
    } else {
        layers[0]["effects"] = pin;
    }
    raw
}

/// The fit tolerance cap of `target` on the owners of `raw`, on the
/// document's canvas, or why export writes no keys for it.
fn tolerance_cap(raw: &Value, target: &Value) -> std::result::Result<f64, String> {
    let document = EditableFxCompositionDocument::from_json_value(raw.clone()).unwrap();
    let composition = document.composition();
    let dimensions = document.dimensions();
    let owners = Owners::new(
        composition.layers(),
        composition.dynamics(),
        [dimensions.width, dimensions.height],
    );
    let target: PropertyTarget = serde_json::from_value(target.clone()).unwrap();
    owners
        .bind(&target)
        .map(|bound| bound.binding.rules().tolerance_cap)
}

/// A Corner Pin coordinate fits within half a pixel of the longer side of
/// its clip's frame, one cap for all coordinates: a video's own source,
/// whatever the canvas, and the canvas of a nest or adjustment layer.
/// Position keeps its pixel cap, and an invalid `sourceRect` keeps the
/// writer's error rather than taking another frame.
#[test]
fn corner_pin_caps_are_half_a_pixel_of_the_longer_side_of_the_clip_frame() {
    let cases = [
        // A 4K source on an HD canvas, and an HD source on a 4K canvas.
        (1, [1920, 1080], [3840, 2160], 3840),
        (1, [3840, 2160], [1920, 1080], 1920),
        // A portrait frame's longer side is its height.
        (1, [1920, 1080], [1080, 1920], 1920),
        (12, [1080, 1920], [1280, 720], 1920),
        (12, [4320, 7680], [1280, 720], 7680),
        // A nest's frame is its canvas, not its video's source.
        (10, [7680, 4320], [1280, 720], 7680),
    ];
    let params = ["upperLeftX", "upperLeftY", "lowerRightX", "lowerRightY"];
    let caps: Vec<_> = cases
        .iter()
        .map(|&(owner, canvas, source, _)| {
            let raw = corner_pin_document(owner, canvas, source);
            params.map(|param| tolerance_cap(&raw, &effect_target(40, param)))
        })
        .collect();
    let expected: Vec<[std::result::Result<f64, String>; 4]> = cases
        .iter()
        .map(|&(.., side)| params.map(|_| Ok(0.5 / f64::from(side))))
        .collect();
    assert_eq!(caps, expected, "{cases:?}");

    let raw = corner_pin_document(1, [1920, 1080], [3840, 2160]);
    assert_eq!(tolerance_cap(&raw, &layer_target(1, "positionX")), Ok(0.5));
    for (rect, error) in [
        (None, "sourceRect must cover the exact canvas"),
        (
            Some(json!({"x": 8, "y": 0, "width": 1912, "height": 1080})),
            "sourceRect must start at the source origin",
        ),
        (
            Some(json!({"x": 0, "y": 0, "width": 1920.5, "height": 1080})),
            "sourceRect width must be a positive integer",
        ),
    ] {
        let mut raw = corner_pin_document(1, [1920, 1080], [1920, 1080]);
        let source = raw["composition"]["layers"][0]["source"]
            .as_object_mut()
            .unwrap();
        match rect {
            Some(rect) => source.insert("sourceRect".to_owned(), rect),
            None => source.remove("sourceRect"),
        };
        assert_eq!(
            tolerance_cap(&raw, &effect_target(40, "upperLeftX")),
            Err(format!("unsupported conversion: {error}"))
        );
        // Bindings that do not divide the frame keep their caps.
        assert_eq!(tolerance_cap(&raw, &layer_target(1, "opacity")), Ok(0.1));
    }
}

/// One baked track: each key's owner-local milliseconds, value and easing
/// from the key before it.
type BakedKeys = Vec<(f64, f64, FittedEasing)>;

/// Bakes `scripts`, each a parameter of Corner Pin 40 of `raw` and its code,
/// and returns each parameter's keys. Every script bakes, and no other track
/// is added: export keeps a static partner.
fn baked_corner_keys<const N: usize>(raw: &Value, scripts: [(&str, &str); N]) -> [BakedKeys; N] {
    let mut raw = raw.clone();
    set_scripts(
        &mut raw,
        &scripts.map(|(param, code)| (effect_target(40, param), code)),
    );
    let document = EditableFxCompositionDocument::from_json_value(raw).unwrap();
    let mut omissions = Vec::new();
    let baked = bake_scripts(&document, &mut omissions).unwrap();
    let entries = baked.document().composition().dynamics().entries();
    assert_eq!(entries.len(), N, "{:#?}", reasons_of(&omissions));
    scripts.map(|(param, _)| {
        let target = PropertyTarget::effect_param(fx_schema::EffectId::new(40), param);
        entries
            .iter()
            .find(|entry| entry.target == target)
            .and_then(|entry| entry.animator.keyframe_track())
            .unwrap_or_else(|| panic!("{param} was not baked: {:#?}", reasons_of(&omissions)))
            .keyframes()
            .iter()
            .map(|key| {
                let PropertyValue::Float(value) = key.value() else {
                    panic!("{key:?}")
                };
                let easing = match key.easing() {
                    PropertyKeyframeEasing::Hold => FittedEasing::Hold,
                    PropertyKeyframeEasing::Linear => FittedEasing::Linear,
                    PropertyKeyframeEasing::CubicBezier { x1, y1, x2, y2 } => {
                        assert_eq!([x1, x2], [1.0 / 3.0, 2.0 / 3.0]);
                        FittedEasing::Cubic { y1, y2 }
                    }
                };
                (key.layer_time().as_millis() as f64, *value, easing)
            })
            .collect()
    })
}

/// The largest distance, in pixels of a `side`-pixel frame axis, between the
/// curve through `keys` and `formula` at every millisecond from 0 through
/// `window_ms`: the curve eases into each key and holds after the last.
fn pixel_error(
    keys: &[(f64, f64, FittedEasing)],
    formula: fn(f64) -> f64,
    window_ms: u32,
    side: u32,
) -> f64 {
    (0..=window_ms)
        .map(|ms| {
            let ms = f64::from(ms);
            let next = keys.partition_point(|&(time, _, _)| time <= ms);
            let (start, from, _) = keys[next - 1];
            let value = keys.get(next).map_or(from, |&(end, to, easing)| {
                from + (to - from) * easing.progress((ms - start) / (end - start))
            });
            (value - formula(ms)).abs() * f64::from(side)
        })
        .fold(0.0, f64::max)
}

/// Corner Pin scripts on frames whose longer side exceeds 1920 pixels stay
/// within half a pixel of each side at every owner-local millisecond: a 4K
/// source on an HD canvas, whichever coordinate's entry comes first; one
/// scripted coordinate, beside its static partner, of an adjustment layer on
/// a portrait 4K canvas; and a nest on a 7680-pixel-wide canvas.
#[test]
fn corner_pin_scripts_fit_within_half_a_pixel_of_frames_beyond_1920() {
    const X: &str = "return 0.1 + 0.3 * Math.sin(input.time.seconds * 2);";
    const Y: &str = "return 0.2 * input.time.seconds + 0.05 * Math.cos(input.time.seconds * 5);";
    let x_at: fn(f64) -> f64 = |ms| 0.1 + 0.3 * (ms / 1000.0 * 2.0).sin();
    let y_at: fn(f64) -> f64 = |ms| 0.2 * ms / 1000.0 + 0.05 * (ms / 1000.0 * 5.0).cos();

    let video = corner_pin_document(1, [1920, 1080], [3840, 2160]);
    let [x, y] = baked_corner_keys(&video, [("upperLeftX", X), ("upperLeftY", Y)]);
    let [reversed_y, reversed_x] =
        baked_corner_keys(&video, [("upperLeftY", Y), ("upperLeftX", X)]);
    assert_eq!((&reversed_x, &reversed_y), (&x, &y));
    // One native point: both coordinates share their key times and easing.
    assert!(
        x.len() == y.len() && x.iter().zip(&y).all(|(a, b)| (a.0, a.2) == (b.0, b.2)),
        "{x:?} {y:?}"
    );
    let [adjustment_y] = baked_corner_keys(
        &corner_pin_document(12, [2160, 3840], [1920, 1080]),
        [("upperLeftY", Y)],
    );
    let [nest_x, nest_y] = baked_corner_keys(
        &corner_pin_document(10, [7680, 4320], [1920, 1080]),
        [("upperLeftX", X), ("upperLeftY", Y)],
    );
    let errors = [
        ("4K source X", pixel_error(&x, x_at, 3000, 3840)),
        ("4K source Y", pixel_error(&y, y_at, 3000, 2160)),
        (
            "portrait adjustment Y",
            pixel_error(&adjustment_y, y_at, 3000, 3840),
        ),
        ("7680-pixel nest X", pixel_error(&nest_x, x_at, 2500, 7680)),
        ("7680-pixel nest Y", pixel_error(&nest_y, y_at, 2500, 4320)),
    ];
    assert!(
        errors.iter().all(|(_, pixels)| *pixels <= 0.5 + 1e-9),
        "{errors:?}"
    );
}
