//! Explicit edited-FX non-audio media export cases with packaged primary source bytes.
//!
//! This crate's native reader supplies supplementary structural assertions only.
//! The separate native-specs.json requests independent Adobe authoring; neither
//! native acceptance nor 30fps reference/render fidelity has run for these cases.

use std::{fs, path::Path};

use fx_conv::{ConversionMode, ExportFromTesseract};
use fx_schema::EditableFxCompositionDocument;
use serde_json::Value;
use tesseract_file::{AssetKind, TesseractFileBuilder};

use super::*;
use crate::{AfterEffects, properties};

const FIXTURE_ROOT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/media_native_panel"
);

#[test]
fn native_movie_clock_input_edit_retention() {
    let bytes = include_bytes!(
        "../../../tests/fixtures/media_native_panel/native/fx-export-media-video-stretch.aep"
    );
    use sha2::{Digest, Sha256};
    assert_eq!(
        format!("{:x}", Sha256::digest(bytes)),
        "3efbed60e483041379013d5d2df9c7b328a835bb7d91151813a108baebff0879"
    );
    let native = read_project(bytes).unwrap();
    let ItemKind::Composition(comp) = &native.item(1).unwrap().kind else {
        panic!("pinned independent composition");
    };
    let native_layer = &comp.layers[0];
    assert_eq!(native_layer.name.as_ref(), "Double Speed Movie");
    assert_eq!(native_layer.record.start_time(), Some(-0.125));
    assert_eq!(native_layer.record.in_point(), Some(0.25));
    assert_eq!(native_layer.record.out_point(), Some(4.25));
    assert_eq!(native_layer.record.stretch(), Some(0.5));
    let imported = crate::structure_document::to_structural_fx_document_with_assets(
        &native,
        Some(1),
        &mut |_| true,
    )
    .unwrap();
    let mut value = imported.document.to_json_value().unwrap();
    let occurrence = &mut value["composition"]["layers"][0]["layers"][0];
    let source_clock = occurrence["layers"][0]["playback"].clone();
    occurrence["playback"]["inputRange"] = serde_json::json!({"start": 250, "duration": 1000});
    occurrence["playback"]["mapping"]["input"] =
        serde_json::json!({"start": 250, "duration": 1000});
    occurrence["playback"]["mapping"]["output"] =
        serde_json::json!({"start": 500, "duration": 1000});
    assert_eq!(occurrence["layers"][0]["playback"], source_clock);
    let document = EditableFxCompositionDocument::from_json_value(value).unwrap();
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("edited.tsrct");
    drop(
        TesseractFileBuilder::try_new(document)
            .unwrap()
            .add_asset(
                "aep-local-item-13",
                Path::new(FIXTURE_ROOT).join("media/movie.mov"),
                AssetKind::Video,
            )
            .unwrap()
            .write(&input)
            .unwrap(),
    );
    let output = root.path().join("fresh");
    let report = AfterEffects
        .export_from_tesseract(&input, &output, &Default::default(), ConversionMode::Write)
        .unwrap();
    let fresh = read_project(&fs::read(output.join("project.aep")).unwrap()).unwrap();
    let mut fields = Vec::new();
    for item in &fresh.items {
        if let ItemKind::Composition(comp) = &item.kind {
            for layer in &comp.layers {
                fields.push((
                    layer.name.to_string(),
                    layer.record.start_time(),
                    layer.record.in_point(),
                    layer.record.out_point(),
                    layer.record.stretch(),
                ));
            }
        }
    }
    assert!(
        fields.iter().any(
            |(name, start, input, output, stretch)| name == "Source content clock"
                && *start == Some(-0.125)
                && *input == Some(0.25)
                && *output == Some(4.25)
                && *stretch == Some(0.5)
        ),
        "source clock changed: {fields:?}; {:?}",
        report.diagnostics
    );
    assert!(
        fields.iter().any(
            |(name, start, input, output, stretch)| name == "Double Speed Movie"
                && *start == Some(-0.25)
                && *input == Some(0.5)
                && *output == Some(1.5)
                && *stretch == Some(1.0)
        ),
        "occurrence edit lost: {fields:?}"
    );
}

#[test]
fn native_movie_remap_input_edit_retention() {
    use sha2::{Digest, Sha256};
    let bytes = include_bytes!(
        "../../../tests/fixtures/media_native_panel/native/fx-export-media-static-remap.aep"
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(bytes)),
        "e795d41e2265565f5da6104dfc52893270cb619fac571576eb46c8317c9cb706"
    );
    let native = read_project(bytes).unwrap();
    let ItemKind::Composition(comp) = &native.item(1).unwrap().kind else {
        panic!("pinned remap composition");
    };
    let remap = |layer: &crate::structure::Layer| {
        properties::root_runs(&layer.content)
            .unwrap()
            .into_iter()
            .find(|(name, _)| *name == "ADBE Time Remapping")
            .map(|(_, chunks)| {
                properties::read_numeric(properties::unique_list(chunks, *b"tdbs").unwrap())
                    .unwrap()
            })
    };
    let original = remap(&comp.layers[0]).unwrap();
    let curve = |property: &properties::NumericProperty| {
        property
            .keyframes
            .iter()
            .map(|key| {
                (
                    key.time_secs,
                    key.values.clone(),
                    key.in_interpolation,
                    key.out_interpolation,
                )
            })
            .collect::<Vec<_>>()
    };
    let original_curve = curve(&original);
    assert_eq!(
        original_curve,
        vec![
            (0.0, vec![0.75], 1, 1),
            (2.0, vec![0.75], 1, 1),
            (8.0, vec![8.0], 1, 1),
        ]
    );
    let imported = crate::structure_document::to_structural_fx_document_with_assets(
        &native,
        Some(1),
        &mut |_| true,
    )
    .unwrap();
    let mut value = imported.document.to_json_value().unwrap();
    let occurrence = &mut value["composition"]["layers"][0]["layers"][0];
    let source_clock = occurrence["layers"][0]["playback"].clone();
    occurrence["playback"]["inputRange"] = serde_json::json!({"start": 250, "duration": 1000});
    occurrence["playback"]["mapping"]["input"] =
        serde_json::json!({"start": 250, "duration": 1000});
    occurrence["playback"]["mapping"]["output"] =
        serde_json::json!({"start": 500, "duration": 1000});
    assert_eq!(occurrence["layers"][0]["playback"], source_clock);
    let document = EditableFxCompositionDocument::from_json_value(value).unwrap();
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("edited.tsrct");
    drop(
        TesseractFileBuilder::try_new(document)
            .unwrap()
            .add_asset(
                "aep-local-item-13",
                Path::new(FIXTURE_ROOT).join("media/movie.mov"),
                AssetKind::Video,
            )
            .unwrap()
            .write(&input)
            .unwrap(),
    );
    let output = root.path().join("fresh");
    let report = AfterEffects
        .export_from_tesseract(&input, &output, &Default::default(), ConversionMode::Write)
        .unwrap();
    let fresh = read_project(&fs::read(output.join("project.aep")).unwrap()).unwrap();
    assert!(
        fresh.items.iter().any(|item| {
            let ItemKind::Composition(comp) = &item.kind else {
                return false;
            };
            comp.layers.iter().any(|layer| {
                layer.name.as_ref() == "Held Blue Frame"
                    && layer.record.start_time() == Some(-0.25)
                    && layer.record.in_point() == Some(0.5)
                    && layer.record.out_point() == Some(1.5)
                    && layer.record.stretch() == Some(1.0)
            })
        }),
        "edited outer occurrence clock lost"
    );

    let remaps = fresh
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            ItemKind::Composition(comp) => Some(&comp.layers),
            _ => None,
        })
        .flatten()
        .filter_map(remap)
        .map(|remap| curve(&remap))
        .collect::<Vec<_>>();
    assert!(
        remaps.contains(&original_curve),
        "unmodified native Time Remap changed: {original_curve:?} vs {remaps:?}; {:?}",
        report.diagnostics
    );
}

#[test]
fn native_movie_remap_finite_domain_certificate_guards() {
    let native = read_project(include_bytes!(
        "../../../tests/fixtures/media_native_panel/native/fx-export-media-static-remap.aep"
    ))
    .unwrap();
    let imported = crate::structure_document::to_structural_fx_document_with_assets(
        &native,
        Some(1),
        &mut |_| true,
    )
    .unwrap();
    let value = imported.document.to_json_value().unwrap();
    let carrier = value["composition"]["layers"][0]["layers"][0]["layers"][0]["layers"][0].clone();
    assert_eq!(carrier["name"], "Authored source remap");
    let group: GroupLayer = serde_json::from_value(carrier.clone()).unwrap();
    let (view, domains) =
        hierarchy_clock::finite_media_remap_view(&group, Time::from_millis(2000)).unwrap();
    assert_eq!(view.playback.input_range().duration.as_millis(), 2000);
    assert_eq!(view.playback.mapping(), group.playback.mapping());
    assert_eq!(view.layers, group.layers);
    assert_eq!(domains.default_domain.duration.as_millis(), 8000);
    let (_, long_domains) =
        hierarchy_clock::finite_media_remap_view(&group, Time::from_millis(12000)).unwrap();
    assert_eq!(
        long_domains, domains,
        "enclosing lifetime is not source duration"
    );
    assert!(hierarchy_clock::finite_media_remap_view(&group, Time::ZERO).is_none());
    let mut invalid_duration = carrier.clone();
    invalid_duration["layers"][0]["sourceIntrinsicDuration"] = serde_json::json!(0);
    assert!(serde_json::from_value::<GroupLayer>(invalid_duration).is_err());
    for (pointer, replacement) in [
        ("/layers/0/sourceIntrinsicDuration", serde_json::json!(9000)),
        ("/layers/0", carrier.clone()),
        ("/layers/0/sourceRange/start", serde_json::json!(1)),
        ("/layers/0/playback/inputOffsetMs", serde_json::json!(1)),
        (
            "/layers/0/playback/mapping",
            serde_json::json!({"type": "linear", "input": {"start": 1, "duration": 8000}, "output": {"start": 1, "duration": 8000}}),
        ),
        (
            "/layers/0/playback/mapping/output/start",
            serde_json::json!(1),
        ),
        ("/layers/0/parent", serde_json::json!(999)),
        (
            "/layers/1/source/assetId",
            serde_json::json!("different-source"),
        ),
        ("/playback/inputOffsetMs", serde_json::json!(1)),
        ("/layers", serde_json::json!([])),
    ] {
        let mut edited = carrier.clone();
        *edited.pointer_mut(pointer).unwrap() = replacement;
        let edited: GroupLayer = serde_json::from_value(edited).unwrap();
        assert!(
            hierarchy_clock::finite_media_remap_view(&edited, Time::from_millis(2000)).is_none(),
            "{pointer}"
        );
    }
}

fn close(actual: f64, expected: f64, label: &str) {
    assert!(
        (actual - expected).abs() < 1e-6,
        "{label}: expected {expected}, got {actual}"
    );
}

fn property(layer: &crate::structure::Layer, match_name: &str) -> properties::NumericProperty {
    properties::read_transform(&layer.content)
        .expect("native Transform")
        .into_iter()
        .find(|property| property.match_name == match_name)
        .unwrap_or_else(|| panic!("missing editable {match_name} on {}", layer.name))
        .numeric
        .expect("numeric Transform control")
}

fn assert_values(actual: &[f64], expected: &Value, label: &str) {
    let values = expected.as_array().expect("manual numeric oracle");
    assert_eq!(actual.len(), values.len(), "{label} component count");
    for (index, (actual, oracle)) in actual.iter().zip(values).enumerate() {
        close(
            *actual,
            oracle.as_f64().expect("numeric component"),
            &format!("{label}[{index}]"),
        );
    }
}

fn source_name<'a>(
    project: &'a crate::structure::StructuralProject,
    layer: &crate::structure::Layer,
) -> &'a str {
    let source = project
        .item(layer.record.source_id())
        .expect("editable footage identity");
    let media = source.media.as_ref().expect("footage source item");
    let descriptor = media.as_ref().expect("typed native media descriptor");
    let path = descriptor.authored_path.as_str();
    assert!(
        path.starts_with("media/"),
        "package-relative media path: {path}"
    );
    path.rsplit_once('-')
        .expect("staged asset suffix")
        .1
        .split_once('.')
        .expect("staged source extension")
        .0
}

fn media_file(id: &str) -> (&'static str, AssetKind) {
    match id {
        "red" => ("red.exr", AssetKind::Image),
        "green" => ("green.exr", AssetKind::Image),
        "movie" => ("movie.mov", AssetKind::Video),
        "unsupported-png" => ("unsupported.png", AssetKind::Image),
        _ => panic!("undeclared media asset: {id}"),
    }
}

fn check_case(name: &str, input_json: &str, expected_json: &str) {
    let input: Value = serde_json::from_str(input_json).expect("committed edited FX input");
    let expected: Value = serde_json::from_str(expected_json).expect("manual native contract");
    assert_eq!(input["composition"]["name"], name);
    assert_eq!(expected["caseId"], format!("fx-export-{name}"));
    assert_eq!(expected["status"], "AUTHORING_REQUEST_UNRUN_UNMEASURED");
    assert!(
        !input_json.contains("jsScript"),
        "native editable media only"
    );

    let root = tempfile::tempdir().expect("panel scratch");
    let document = EditableFxCompositionDocument::from_json_value(input)
        .expect("explicit FX media composition");
    let mut builder = TesseractFileBuilder::try_new(document).expect("editable FX archive");
    for asset in expected["assets"]
        .as_array()
        .expect("declared packaged assets")
    {
        let id = asset.as_str().expect("media asset ID");
        let (filename, kind) = media_file(id);
        let path = Path::new(FIXTURE_ROOT).join("media").join(filename);
        builder = builder
            .add_asset(id, &path, kind)
            .expect("package actual primary media");
    }
    let archive_path = root.path().join("edited.tsrct");
    drop(
        builder
            .write(&archive_path)
            .expect("fresh edited FX archive"),
    );
    let output_path = root.path().join("exported");
    let report = AfterEffects
        .export_from_tesseract(
            &archive_path,
            &output_path,
            &Default::default(),
            ConversionMode::Write,
        )
        .expect("fresh FX-to-AEP export with actual media descriptors");
    if let Some(reason) = expected["diagnostic"].as_str() {
        assert!(
            report
                .diagnostics
                .iter()
                .any(|entry| entry.message.contains(reason)),
            "missing contextual unsupported-format diagnostic: {:?}",
            report.diagnostics
        );
    } else {
        assert!(
            report.diagnostics.is_empty(),
            "unexpected omission: {:?}",
            report.diagnostics
        );
    }

    let bytes = fs::read(output_path.join("project.aep")).expect("fresh native project");
    {
        let dir = crate::adobe_test_support::artifact_directory();
        fs::create_dir_all(&dir).expect("panel artifact directory");
        let base = Path::new(&dir).join(name);
        fs::write(base.with_extension("fx.json"), input_json).expect("explicit FX input artifact");
        fs::write(base.with_extension("aep"), &bytes)
            .expect("FX-exported AEP artifact (not Adobe oracle)");
        fs::write(base.with_extension("expected.json"), expected_json)
            .expect("manual native contract artifact");
        // Open <case>/project.aep with its neighboring media/ folder for Adobe
        // inspection; the flat <case>.aep is only an artifact for the runner.
        let package_dir = Path::new(&dir).join(name);
        fs::create_dir_all(&package_dir).expect("standalone native package");
        fs::write(package_dir.join("project.aep"), &bytes).expect("native package project");
        let media_dir = package_dir.join("media");
        if output_path.join("media").is_dir() {
            fs::create_dir_all(&media_dir).expect("panel packaged media directory");
            for entry in fs::read_dir(output_path.join("media")).expect("published media") {
                let entry = entry.expect("published source");
                fs::copy(entry.path(), media_dir.join(entry.file_name()))
                    .expect("copy byte-identical native media for Adobe inspection");
            }
        }
    }

    let project = read_project(&bytes).expect("supplementary structural readback");
    let ItemKind::Composition(comp) = &project.item(1).expect("root item").kind else {
        panic!("{name}: root is not a composition");
    };
    assert_eq!((comp.width, comp.height), (320, 180));
    close(
        comp.duration_secs,
        expected["duration"].as_f64().unwrap_or(2.0),
        "composition duration",
    );
    close(
        comp.frame_rate,
        24.0,
        "source composition fps (not Adobe MP4 output fps)",
    );
    let names = expected["layers"]
        .as_array()
        .expect("named editable layers");
    assert_eq!(comp.layers.len(), names.len(), "{name}: native layer count");
    for (layer, oracle) in comp.layers.iter().zip(names) {
        assert_eq!(
            layer.name.as_ref(),
            oracle.as_str().expect("native layer name")
        );
        assert_ne!(
            layer.record.source_id(),
            0,
            "media must reference editable footage"
        );
    }

    let published = expected["published"].as_array().unwrap_or_else(|| {
        expected["assets"]
            .as_array()
            .expect("declared media assets")
    });
    let descriptors = project
        .items
        .iter()
        .filter_map(|item| item.media.as_ref())
        .map(|media| media.as_ref().expect("typed footage descriptor"))
        .collect::<Vec<_>>();
    assert_eq!(
        descriptors.len(),
        published.len(),
        "only emitted media may be staged"
    );
    assert_eq!(
        report.artifacts.len(),
        descriptors.len() + 1,
        "report must contain only the project and emitted media"
    );
    for artifact in &report.artifacts {
        assert!(
            output_path.join(&artifact.path).is_file(),
            "reported artifact does not exist: {}",
            artifact.path.display()
        );
    }
    for descriptor in &descriptors {
        assert!(
            report.artifacts.iter().any(|artifact| {
                let authored = Path::new(descriptor.authored_path.as_str());
                let canonical_output = output_path.canonicalize().unwrap();
                let reported = authored
                    .strip_prefix(&output_path)
                    .or_else(|_| authored.strip_prefix(&canonical_output))
                    .unwrap_or(authored);
                artifact.path == reported
            }),
            "emitted media is missing from the report: {}",
            descriptor.authored_path
        );
    }
    for asset in published {
        let id = asset.as_str().expect("expected published asset");
        let (filename, _) = media_file(id);
        let descriptor = descriptors
            .iter()
            .find(|descriptor| {
                descriptor.authored_path.ends_with(&format!(
                    "-{id}.{}",
                    filename.rsplit_once('.').expect("fixture extension").1
                ))
            })
            .unwrap_or_else(|| panic!("missing typed native descriptor for {id}"));
        assert_eq!(
            descriptor.source_format,
            if id == "movie" { *b"MOoV" } else { *b"oEXR" }
        );
        assert_eq!(
            (descriptor.width, descriptor.height),
            if id == "movie" { (320, 180) } else { (64, 48) }
        );
        if id == "movie" {
            close(descriptor.duration.seconds(), 8.0, "original MOV duration");
            assert_eq!(descriptor.native_frame_rate.integer, 24);
        } else {
            close(descriptor.duration.seconds(), 0.0, "EXR still duration");
        }
        let published_bytes = fs::read(output_path.join(&descriptor.authored_path))
            .expect("published package media bytes");
        let original_bytes = fs::read(Path::new(FIXTURE_ROOT).join("media").join(filename))
            .expect("primary authoring media bytes");
        assert_eq!(
            published_bytes, original_bytes,
            "asset {id} was not rewritten"
        );
    }

    if let Some(order) = expected["sourceOrder"].as_array() {
        for (layer, asset) in comp.layers.iter().zip(order) {
            assert_eq!(
                source_name(&project, layer),
                asset.as_str().expect("source identity")
            );
        }
    }
    if expected["sharedFootage"] == true {
        assert_eq!(
            comp.layers[0].record.source_id(),
            comp.layers[1].record.source_id(),
            "two occurrences must reference one reusable original source"
        );
    }
    if let Some(ranges) = expected["ranges"].as_array() {
        for (layer, range) in comp.layers.iter().zip(ranges) {
            close(
                layer.record.in_point().expect("variant in-point"),
                range[0].as_f64().expect("manual in-point"),
                "variant in-point",
            );
            close(
                layer.record.out_point().expect("variant out-point"),
                range[1].as_f64().expect("manual out-point"),
                "variant out-point",
            );
        }
    }
    let layer = &comp.layers[0];
    if let Some(clock) = expected["clock"].as_object() {
        for (name, actual) in [
            ("in", layer.record.in_point().expect("editable in-point")),
            ("out", layer.record.out_point().expect("editable out-point")),
            (
                "start",
                layer.record.start_time().expect("editable source start"),
            ),
            (
                "stretch",
                layer.record.stretch().expect("editable playback stretch"),
            ),
        ] {
            close(
                actual,
                clock[name].as_f64().expect("manual clock oracle"),
                name,
            );
        }
    }
    if let Some(oracle) = expected.get("position") {
        assert_values(
            &property(layer, "ADBE Position").values,
            oracle,
            "native position",
        );
    }
    if let Some(oracle) = expected.get("orientation") {
        assert_values(
            &property(layer, "ADBE Orientation").values,
            oracle,
            "native orientation",
        );
    }
    if let Some(oracle) = expected.get("opacity") {
        assert_values(
            &property(layer, "ADBE Opacity").values,
            oracle,
            "native opacity",
        );
    }
    if let Some(oracle) = expected["opacityKeys"].as_array() {
        let native = property(layer, "ADBE Opacity");
        assert!(native.animated, "animated native media opacity");
        assert_eq!(native.keyframes.len(), oracle.len());
        for (key, expected) in native.keyframes.iter().zip(oracle) {
            close(
                key.time_secs,
                expected[0].as_f64().expect("key time"),
                "opacity key time",
            );
            close(
                key.values[0],
                expected[1].as_f64().expect("key value"),
                "opacity key value",
            );
            assert_eq!((key.in_interpolation, key.out_interpolation), (1, 1));
        }
    }
    if let Some(count) = expected["maskCount"].as_u64() {
        let parade = properties::root_runs(&layer.content)
            .expect("native root")
            .into_iter()
            .find(|(name, _)| *name == "ADBE Mask Parade")
            .expect("editable media crop mask");
        let masks = properties::runs(parade.1)
            .expect("mask children")
            .into_iter()
            .filter(|(name, _)| *name == "ADBE Mask Atom")
            .count();
        assert_eq!(
            masks,
            usize::try_from(count).expect("small manual mask count"),
            "media frame crop must remain editable"
        );
    }
    if expected["threeD"] == true {
        assert!(
            layer.record.flags().three_d_layer,
            "media 3D transform retained"
        );
    }
    if expected["timeRemap"] == true {
        assert!(
            properties::root_runs(&layer.content)
                .expect("native root")
                .into_iter()
                .any(|(name, _)| name == "ADBE Time Remapping"),
            "static source sample must retain editable native time remap"
        );
    }
    if expected["frameBlending"] == "frameMix" {
        assert!(
            layer.record.flags().frame_blending,
            "native layer frame mix"
        );
        assert!(
            !layer.record.flags().frame_blending_mode,
            "not optical flow"
        );
        assert_ne!(
            comp.record.flags()[1] & 16,
            0,
            "composition blending master"
        );
    }
}

#[test]
fn media_video_fractional_affine_window_full_export_and_input_edit() {
    for (name, start) in [
        ("media-video-fractional-affine", 1_000),
        ("media-video-fractional-affine-edited", 1_250),
    ] {
        let mut input: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/media_native_panel/media-video-stretch.fx.json"
        ))
        .unwrap();
        input["composition"]["name"] = json!(name);
        input["duration"] = json!(3);
        let layer = &mut input["composition"]["layers"][0];
        layer.as_object_mut().unwrap().remove("activeRange");
        layer["playback"] = json!({
            "type":"windowed",
            "inputRange":{"start":start,"duration":1000},
            "mapping":{"type":"linear", "input":{"start":0,"duration":3000},
                "output":{"start":250,"duration":4000}},
            "inputOffsetMs":250
        });
        let expected = json!({
            "status":"AUTHORING_REQUEST_UNRUN_UNMEASURED",
            "caseId":format!("fx-export-{name}"),
            "assets":["movie"], "layers":["Double Speed Movie"], "duration":3,
            "diagnostic":"rounded-millisecond visibility uses a half-millisecond parent-boundary correction",
            "clock":{
                "stretch":0.75, "start":-0.4375,
                "in":(start as f64 / 1000.0 + 0.4375 - 0.0005) / 0.75,
                "out":(start as f64 / 1000.0 + 1.4375 - 0.0005) / 0.75
            }
        });
        check_case(name, &input.to_string(), &expected.to_string());
    }
}

macro_rules! panel_case {
    ($symbol:ident, $name:literal, $input:expr, $expected:expr) => {
        #[test]
        #[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
        fn $symbol() {
            crate::adobe_test_support::export_case($name, || {
                check_case($name, $input, $expected);
            });
        }
    };
}

panel_case!(
    media_image_fit,
    "media-image-fit",
    include_str!("../../../tests/fixtures/media_native_panel/media-image-fit.fx.json"),
    include_str!("../../../tests/fixtures/media_native_panel/media-image-fit.expected.json")
);
panel_case!(
    media_video_clock,
    "media-video-clock",
    include_str!("../../../tests/fixtures/media_native_panel/media-video-clock.fx.json"),
    include_str!("../../../tests/fixtures/media_native_panel/media-video-clock.expected.json")
);
panel_case!(
    media_video_stretch,
    "media-video-stretch",
    include_str!("../../../tests/fixtures/media_native_panel/media-video-stretch.fx.json"),
    include_str!("../../../tests/fixtures/media_native_panel/media-video-stretch.expected.json")
);
panel_case!(
    media_frame_blending,
    "media-frame-blending",
    include_str!("../../../tests/fixtures/media_native_panel/media-frame-blending.fx.json"),
    include_str!("../../../tests/fixtures/media_native_panel/media-frame-blending.expected.json")
);
panel_case!(
    media_shared_source,
    "media-shared-source",
    include_str!("../../../tests/fixtures/media_native_panel/media-shared-source.fx.json"),
    include_str!("../../../tests/fixtures/media_native_panel/media-shared-source.expected.json")
);
panel_case!(
    media_source_switch,
    "media-source-switch",
    include_str!("../../../tests/fixtures/media_native_panel/media-source-switch.fx.json"),
    include_str!("../../../tests/fixtures/media_native_panel/media-source-switch.expected.json")
);
panel_case!(
    media_unsupported_sibling,
    "media-unsupported-sibling",
    include_str!("../../../tests/fixtures/media_native_panel/media-unsupported-sibling.fx.json"),
    include_str!(
        "../../../tests/fixtures/media_native_panel/media-unsupported-sibling.expected.json"
    )
);
panel_case!(
    media_static_remap,
    "media-static-remap",
    include_str!("../../../tests/fixtures/media_native_panel/media-static-remap.fx.json"),
    include_str!("../../../tests/fixtures/media_native_panel/media-static-remap.expected.json")
);
panel_case!(
    media_image_3d,
    "media-image-3d",
    include_str!("../../../tests/fixtures/media_native_panel/media-image-3d.fx.json"),
    include_str!("../../../tests/fixtures/media_native_panel/media-image-3d.expected.json")
);
panel_case!(
    media_constant_source,
    "media-constant-source",
    include_str!("../../../tests/fixtures/media_native_panel/media-constant-source.fx.json"),
    include_str!("../../../tests/fixtures/media_native_panel/media-constant-source.expected.json")
);
panel_case!(
    media_legacy_image,
    "media-legacy-image",
    include_str!("../../../tests/fixtures/media_native_panel/media-legacy-image.fx.json"),
    include_str!("../../../tests/fixtures/media_native_panel/media-legacy-image.expected.json")
);
