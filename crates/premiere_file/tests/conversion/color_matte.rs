//! Authored Color Mattes: import as editable solid rectangles, export as
//! generator media, and stay distinct from gaps and the implicit canvas.
//! Other rectangles export as editable graphic Shapes.

use super::support::*;

#[cfg(feature = "ffmpeg-library")]
mod mask;
#[cfg(feature = "ffmpeg-library")]
mod motion;
use premiere_file::PrProjectFile;
#[cfg(feature = "ffmpeg-library")]
use premiere_file::{PrSequence, PrVideoItem};
use serde_json::json;
#[cfg(feature = "ffmpeg-library")]
use serde_json::Value;
use std::path::Path;
#[cfg(feature = "ffmpeg-library")]
use tesseract_file::AssetKind;
use tesseract_file::{TesseractFile, TesseractFileBuilder};

#[cfg(feature = "ffmpeg-library")]
const FIXTURE: &str = "feature_color_matte_strict.prproj";
#[cfg(feature = "ffmpeg-library")]
const SEQUENCE_UID: &str = "c8acf9c1-34b2-4086-9f55-d528950a7059";
#[cfg(feature = "ffmpeg-library")]
const TICKS: i64 = 254_016_000_000;

/// `(track, start s, end s, media name)` for every occurrence, with
/// "graphic" for a graphic.
#[cfg(feature = "ffmpeg-library")]
fn placements(project: &PrProjectFile, sequence: &PrSequence) -> Vec<(usize, i64, i64, String)> {
    sequence
        .video_tracks()
        .enumerate()
        .flat_map(|(track, items)| {
            items.iter().map(move |item| {
                let name = match item {
                    PrVideoItem::Media(clip) => project.media(clip).unwrap().name().to_owned(),
                    PrVideoItem::Graphic(_) | PrVideoItem::Capsule(_) => "graphic".to_owned(),
                };
                let range = item.timeline_ticks();
                (track, range.start / TICKS, range.end / TICKS, name)
            })
        })
        .collect()
}

#[cfg(feature = "ffmpeg-library")]
fn layer_names_at(document: &Value, millis: i64) -> Vec<String> {
    document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|layer| {
            let start = (*crate::test_support::layer_range(layer))["start"]
                .as_i64()
                .unwrap();
            let end = start
                + (*crate::test_support::layer_range(layer))["duration"]
                    .as_i64()
                    .unwrap();
            (start..end).contains(&millis)
        })
        .map(|layer| layer["name"].as_str().unwrap().to_owned())
        .collect()
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn isolated_color_mattes_round_trip_as_editable_solid_fills() {
    let dir = tempfile::tempdir().unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(FIXTURE);
    let native = PrProjectFile::load(&source).unwrap().0;
    let sequence = native.sequences().next().unwrap();
    let expected = vec![
        (0, 0, 6, "video-30fps-10s.mp4".to_owned()),
        (0, 7, 9, "Color Matte".to_owned()),
        (1, 2, 5, "Color Matte".to_owned()),
    ];
    assert_eq!(placements(&native, sequence), expected);

    let output = dir.path().join("converted");
    let omissions = premiere_to_tesseract(&source, &output, Some(SEQUENCE_UID), false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let archive = first_project(&output);
    let file = TesseractFile::open(&archive).unwrap();
    // Mattes are generated, so only the video is packaged.
    assert_eq!(file.metadata().assets.len(), 1);
    let document = file.project_json().unwrap();
    assert_eq!(document["duration"], 9.0);
    let layers = document["composition"]["layers"].as_array().unwrap();
    let red = layers
        .iter()
        .find(|layer| layer["rect"]["fillColor"] == json!([1.0, 0.0, 0.0, 1.0]))
        .unwrap();
    let blue = layers
        .iter()
        .find(|layer| layer["rect"]["fillColor"] == json!([0.0, 0.0, 1.0, 1.0]))
        .unwrap();
    assert_eq!(
        (*crate::test_support::layer_range(red)),
        json!({"start": 2000, "duration": 3000})
    );
    assert_eq!(
        (*crate::test_support::layer_range(blue)),
        json!({"start": 7000, "duration": 2000})
    );
    for matte in [red, blue] {
        assert_eq!(matte["type"], "Rect");
        assert_eq!(matte["rect"]["size"], json!([1920.0, 1080.0]));
        assert_eq!(matte["rect"]["strokeEnabled"], false);
        assert_eq!(matte["transform"]["opacity"], 100.0);
    }
    // The red matte stacks above the video; the canvas stays below everything.
    let names: Vec<_> = layers
        .iter()
        .map(|layer| layer["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "Premiere color matte 3",
            "Premiere video 1",
            "Premiere color matte 2",
            "Premiere black canvas"
        ]
    );
    // 3.5 s: red matte over video; 6.5 s: the gap shows only the canvas;
    // 8 s: the blue matte alone is opaque content above the canvas.
    assert_eq!(
        layer_names_at(&document, 3500),
        [
            "Premiere color matte 3",
            "Premiere video 1",
            "Premiere black canvas"
        ]
    );
    assert_eq!(layer_names_at(&document, 6500), ["Premiere black canvas"]);
    assert_eq!(
        layer_names_at(&document, 8000),
        ["Premiere color matte 2", "Premiere black canvas"]
    );

    let package = dir.path().join("premiere");
    tesseract_to_premiere(&archive, &package, false).unwrap();
    let rebuilt = PrProjectFile::load(package.join("project.prproj"))
        .unwrap()
        .0;
    let rebuilt_sequence = rebuilt.sequences().next().unwrap();
    assert_eq!(placements(&rebuilt, rebuilt_sequence), expected);
    let media_files: Vec<_> = std::fs::read_dir(package.join("media"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(media_files, ["video-30fps-10s.mp4"]);
    let xml = read_xml(&package.join("project.prproj"));
    assert!(xml.contains(">/wAAAAEAAAA=</ImporterPrefs>"), "red prefs");
    assert!(xml.contains(">AAD/AAEAAAA=</ImporterPrefs>"), "blue prefs");
    // Both matte streams are still streams, as on every corpus matte.
    assert_eq!(xml.matches("<IsStill>true</IsStill>").count(), 2);
    // As in Adobe's mattes, the four generator clips (two templates, two
    // placements) carry the drop-frame preference and own no markers: only the
    // video's template and placement have a MarkerOwner and a Markers record.
    assert_eq!(
        xml.matches(
            "<BE.Prefs.SyntheticMedia.DefaultIsDropFrame>false</BE.Prefs.SyntheticMedia.DefaultIsDropFrame>"
        )
        .count(),
        4
    );
    assert_eq!(xml.matches("<MarkerOwner ").count(), 2);
    assert_eq!(xml.matches("<Markers ObjectID=").count(), 1);
}

#[test]
fn native_black_video_imports_editably_and_exports_current_content_as_color_matte() {
    use fx_conv::{ConversionMode, ExportFromTesseract};
    use premiere_file::{FrameRate, Premiere, PremiereExportOptions};

    let dir = tempfile::tempdir().unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/black-video/occurrence-1172.xml");
    let output = dir.path().join("imported");
    let omissions = premiere_to_tesseract(
        &source,
        &output,
        Some("d40c25d5-0359-473e-974e-24f423007763"),
        false,
    )
    .unwrap();
    assert!(omissions
        .iter()
        .all(|note| note.scope == premiere_file::OmissionScope::Feature));
    assert_eq!(
        omissions
            .iter()
            .map(|note| note.reason.as_str())
            .collect::<Vec<_>>(),
        [
            "IsCreatedWithNewColorManagement not converted",
            "nondefault tone mapping not converted"
        ]
    );
    let file = TesseractFile::open(first_project(&output)).unwrap();
    assert!(file.metadata().assets.is_empty(), "generator has no file");
    let mut document = file.project_json().unwrap();
    assert_eq!(
        document["dimensions"],
        json!({"width": 2160, "height": 3840})
    );
    let layers = document["composition"]["layers"].as_array().unwrap();
    assert_eq!(layers.len(), 2, "authored solid plus implicit canvas");
    let solid = &layers[0];
    assert_eq!(solid["type"], "Rect");
    assert_eq!(solid["rect"]["size"], json!([2160.0, 3840.0]));
    assert_eq!(solid["rect"]["fillColor"], json!([0.0, 0.0, 0.0, 1.0]));
    assert_eq!(solid["transform"]["position"], json!([0.0, 0.0]));
    assert_eq!(solid["transform"]["anchorPoint"], json!([0.0, 0.0]));
    assert_eq!(solid["transform"]["scale"], json!([100.0, 100.0]));
    assert_eq!(solid["transform"]["rotation"], 0.0);
    assert_eq!(solid["transform"]["opacity"], 100.0);
    assert_eq!(
        crate::test_support::layer_range(solid),
        &json!({"start": 30163, "duration": 1702})
    );
    // Export a current-document two-second placement on the same 29.97 grid,
    // without the unrelated leading gap of the source. Keep the separate
    // implicit canvas so the authored black fill remains ordinary clip content.
    document["duration"] = json!(2.002);
    let layers = document["composition"]["layers"].as_array_mut().unwrap();
    for layer in layers.iter_mut() {
        layer["activeRange"] = json!({"start": 0, "duration": 2002});
    }
    layers[0]["name"] = json!("Edited solid");
    for (index, color, prefs) in [
        (0, [0.0, 0.0, 0.0, 1.0], "AAAAAAEAAAA="),
        (1, [0.2, 0.4, 0.6, 1.0], "M2aZAAEAAAA="),
    ] {
        document["composition"]["layers"][0]["rect"]["fillColor"] = json!(color);
        let archive = dir.path().join(format!("edited-{index}.tsrct"));
        TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap())
            .unwrap()
            .write(&archive)
            .unwrap();
        let package = dir.path().join(format!("export-{index}"));
        let omissions = Premiere
            .export_from_tesseract(
                &archive,
                &package,
                &PremiereExportOptions {
                    frame_rate: Some(FrameRate::Fps30000Over1001),
                },
                ConversionMode::Write,
            )
            .unwrap()
            .diagnostics;
        assert!(omissions.is_empty(), "{omissions:?}");
        let xml = read_xml(&package.join("project.prproj"));
        assert!(xml.contains("<FilePath>1129270354</FilePath>"));
        assert!(!xml.contains("<FilePath>1112293707</FilePath>"));
        assert!(xml.contains(&format!(">{prefs}</ImporterPrefs>")));
        assert_eq!(xml.matches("<IsStill>true</IsStill>").count(), 1);
        let native = PrProjectFile::load(package.join("project.prproj"))
            .unwrap()
            .0;
        let sequence = native.sequences().next().unwrap();
        assert_eq!(sequence.video_occurrences().count(), 1);
        let clip = sequence.video_occurrences().next().unwrap();
        assert_eq!(clip.timeline_ticks(), 0..508_540_032_000);
        let reimported = dir.path().join(format!("reimport-{index}"));
        let omissions =
            premiere_to_tesseract(package.join("project.prproj"), &reimported, None, false)
                .unwrap();
        assert!(omissions.is_empty(), "{omissions:?}");
        let rebuilt = TesseractFile::open(first_project(&reimported))
            .unwrap()
            .project_json()
            .unwrap();
        let solid = &rebuilt["composition"]["layers"][0];
        assert_eq!(solid["type"], "Rect");
        assert_eq!(solid["rect"]["fillColor"], json!(color));
        assert_eq!(
            crate::test_support::layer_range(solid),
            &json!({"start": 0, "duration": 2002})
        );
    }
}

/// A reimported shape layer's name, range, transform, path, fills and strokes.
#[cfg(feature = "ffmpeg-library")]
fn shape_summary(layer: &Value) -> Value {
    let transform = &layer["transform"];
    json!({
        "name": layer["name"],
        "range": (*crate::test_support::layer_range(layer)),
        "transform": [
            &transform["position"],
            &transform["anchorPoint"],
            &transform["scale"],
            &transform["rotation"],
            &transform["opacity"]
        ],
        "path": layer["shape"]["path"]["commands"],
        "fills": layer["shape"]["fills"],
        "strokes": layer["shape"]["strokes"],
    })
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn rectangles_beside_the_mattes_export_as_editable_shapes_and_reimport_as_shape_layers() {
    let dir = tempfile::tempdir().unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let converted = dir.path().join("converted");
    premiere_to_tesseract(
        fixtures.join(FIXTURE),
        &converted,
        Some(SEQUENCE_UID),
        false,
    )
    .unwrap();
    let mut document = TesseractFile::open(first_project(&converted))
        .unwrap()
        .project_json()
        .unwrap();
    // Five bars over the gap and the blue matte (6-9 s): filled; outlined
    // around a centred origin; filled, outlined, turned and scaled; filled
    // with a red-to-green gradient along its top edge over a white fill
    // colour; filled with corners rounded by 20 px.
    let bar = |id: u64, name: &str, transform: Value, rect: Value| {
        json!({
            "type": "Rect", "id": id, "name": name,
            "activeRange": {"start": 6000, "duration": 3000},
            "transform": transform, "rect": rect
        })
    };
    let bars = [
        bar(
            10,
            "Filled bar",
            json!({"anchorPoint": [200, 50], "position": [960, 900], "scale": [100, 100], "rotation": 0, "opacity": 100}),
            json!({"size": [400, 100], "fillColor": [1, 0, 0, 1]}),
        ),
        bar(
            11,
            "Stroked bar",
            json!({"anchorPoint": [0, 0], "position": [960, 200], "scale": [100, 100], "rotation": 0, "opacity": 100}),
            json!({
                "size": [400, 100], "position": [-200, -50], "fillEnabled": false,
                "fillColor": [1, 1, 1, 1], "strokeEnabled": true, "strokeColor": [0, 1, 0, 1],
                "strokeWidth": 6
            }),
        ),
        bar(
            12,
            "Turned bar",
            json!({"anchorPoint": [200, 50], "position": [960, 560], "scale": [80, 80], "rotation": 30, "opacity": 100}),
            json!({
                "size": [400, 100], "fillColor": [1, 1, 0, 1], "strokeEnabled": true,
                "strokeColor": [1, 0, 1, 1], "strokeWidth": 10
            }),
        ),
        bar(
            13,
            "Gradient bar",
            json!({"anchorPoint": [200, 50], "position": [300, 560], "scale": [100, 100], "rotation": 0, "opacity": 100}),
            json!({
                "size": [400, 100], "fillColor": [1, 1, 1, 1],
                "fillPaint": {
                    "type": "gradient", "gradientType": "linear", "start": [0, 0], "end": [400, 0],
                    "stops": [{"offset": 0, "color": [1, 0, 0, 1]}, {"offset": 1, "color": [0, 1, 0, 1]}]
                }
            }),
        ),
        bar(
            14,
            "Rounded bar",
            json!({"anchorPoint": [200, 50], "position": [1620, 560], "scale": [100, 100], "rotation": 0, "opacity": 100}),
            json!({"size": [400, 100], "roundness": 20, "fillColor": [0, 1, 1, 1]}),
        ),
    ];
    let layers = document["composition"]["layers"].as_array_mut().unwrap();
    let asset_id = layers
        .iter()
        .find(|layer| layer["type"] == "Video")
        .unwrap()["source"]["assetId"]
        .as_str()
        .unwrap()
        .to_owned();
    layers.splice(0..0, bars);
    let edited = dir.path().join("edited.tsrct");
    TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap())
        .unwrap()
        .add_asset(
            &asset_id,
            fixtures.join("video-30fps-10s.mp4"),
            AssetKind::Video,
        )
        .unwrap()
        .write(&edited)
        .unwrap();

    // Each bar is a graphic above the mattes and the video, which keep their
    // placements; the first bar listed paints in front.
    let package = dir.path().join("premiere");
    let omissions = tesseract_to_premiere(&edited, &package, false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let native = package.join("project.prproj");
    let rebuilt = PrProjectFile::load(&native).unwrap().0;
    let graphic = |track| (track, 6, 9, "graphic".to_owned());
    assert_eq!(
        placements(&rebuilt, rebuilt.sequences().next().unwrap()),
        [
            (0, 0, 6, "video-30fps-10s.mp4".to_owned()),
            (0, 7, 9, "Color Matte".to_owned()),
            (1, 2, 5, "Color Matte".to_owned()),
            graphic(1),
            graphic(2),
            graphic(3),
            graphic(4),
            graphic(5),
        ]
    );

    // The reimport holds editable shape layers, no longer parametric
    // rectangles, and the mattes stay solid rectangles.
    let reimported = dir.path().join("reimported");
    let omissions = premiere_to_tesseract(&native, &reimported, None, false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let document = TesseractFile::open(first_project(&reimported))
        .unwrap()
        .project_json()
        .unwrap();
    let layers = document["composition"]["layers"].as_array().unwrap();
    let rectangles: Vec<_> = layers
        .iter()
        .filter(|layer| layer["type"] == "Rect")
        .map(|layer| {
            (
                &layer["rect"]["fillColor"],
                crate::test_support::layer_range(layer),
            )
        })
        .collect();
    assert_eq!(
        rectangles,
        [
            (
                &json!([1.0, 0.0, 0.0, 1.0]),
                &json!({"start": 2000, "duration": 3000})
            ),
            (
                &json!([0.0, 0.0, 1.0, 1.0]),
                &json!({"start": 7000, "duration": 2000})
            ),
            (
                &json!([0.0, 0.0, 0.0, 1.0]),
                &json!({"start": 0, "duration": 9000})
            ),
        ]
    );
    let corners = |[x, y]: [f64; 2]| {
        json!([
            {"type": "moveTo", "x": x, "y": y},
            {"type": "lineTo", "x": x + 400.0, "y": y},
            {"type": "lineTo", "x": x + 400.0, "y": y + 100.0},
            {"type": "lineTo", "x": x, "y": y + 100.0},
            {"type": "close"}
        ])
    };
    // FX's rounded outline of the 400x100 bar, corner radius 20 px: each
    // quarter circle's handles are 20 * 0.5522848 px long, in f32. Its eight
    // vertices were written smooth, and return in FX's `straight` mode.
    let k = 20.0 * 0.552_284_8_f32;
    let at = |value: f32| f64::from(value);
    let rounded = json!([
        {"type": "moveTo", "x": 20.0, "y": 0.0, "mirror": "straight"},
        {"type": "lineTo", "x": 380.0, "y": 0.0, "mirror": "straight"},
        {"type": "cubicTo", "c1x": at(380.0 + k), "c1y": 0.0, "c2x": 400.0, "c2y": at(20.0 - k), "x": 400.0, "y": 20.0, "mirror": "straight"},
        {"type": "lineTo", "x": 400.0, "y": 80.0, "mirror": "straight"},
        {"type": "cubicTo", "c1x": 400.0, "c1y": at(80.0 + k), "c2x": at(380.0 + k), "c2y": 100.0, "x": 380.0, "y": 100.0, "mirror": "straight"},
        {"type": "lineTo", "x": 20.0, "y": 100.0, "mirror": "straight"},
        {"type": "cubicTo", "c1x": at(20.0 - k), "c1y": 100.0, "c2x": 0.0, "c2y": at(80.0 + k), "x": 0.0, "y": 80.0, "mirror": "straight"},
        {"type": "lineTo", "x": 0.0, "y": 20.0, "mirror": "straight"},
        {"type": "cubicTo", "c1x": 0.0, "c1y": at(20.0 - k), "c2x": at(20.0 - k), "c2y": 0.0, "x": 20.0, "y": 0.0},
        {"type": "close"}
    ]);
    let fill = |paint: Value| json!([{"blendMode": "normal", "fillRule": "nonZeroWinding", "opacity": 1.0, "paint": paint}]);
    let solid = |color: [f64; 4]| fill(json!({"type": "solid", "color": color}));
    let stroke = |color: [f64; 4], width: f64| {
        json!([{"blendMode": "normal", "cap": "butt", "dashOffset": 0.0, "join": "miter",
                "miterLimit": 4.0, "opacity": 1.0, "paint": {"type": "solid", "color": color},
                "width": width}])
    };
    let range = json!({"start": 6000, "duration": 3000});
    assert_eq!(
        layers
            .iter()
            .filter(|layer| layer["type"] == "Shape")
            .map(shape_summary)
            .collect::<Vec<_>>(),
        [
            json!({
                "name": "Filled bar", "range": range,
                "transform": [[960.0, 900.0], [200.0, 50.0], [100.0, 100.0], 0.0, 100.0],
                "path": corners([0.0, 0.0]), "fills": solid([1.0, 0.0, 0.0, 1.0]),
                "strokes": null
            }),
            json!({
                "name": "Stroked bar", "range": range,
                "transform": [[960.0, 200.0], [0.0, 0.0], [100.0, 100.0], 0.0, 100.0],
                "path": corners([-200.0, -50.0]), "fills": null,
                "strokes": stroke([0.0, 1.0, 0.0, 1.0], 6.0)
            }),
            json!({
                "name": "Turned bar", "range": range,
                "transform": [[960.0, 560.0], [200.0, 50.0], [80.0, 80.0], 30.0, 100.0],
                "path": corners([0.0, 0.0]), "fills": solid([1.0, 1.0, 0.0, 1.0]),
                "strokes": stroke([1.0, 0.0, 1.0, 1.0], 10.0)
            }),
            json!({
                "name": "Gradient bar", "range": range,
                "transform": [[300.0, 560.0], [200.0, 50.0], [100.0, 100.0], 0.0, 100.0],
                "path": corners([0.0, 0.0]),
                "fills": fill(json!({
                    "type": "gradient", "gradientType": "linear", "start": [0.0, 0.0], "end": [400.0, 0.0],
                    "stops": [{"offset": 0.0, "color": [1.0, 0.0, 0.0, 1.0]},
                              {"offset": 1.0, "color": [0.0, 1.0, 0.0, 1.0]}]
                })),
                "strokes": null
            }),
            json!({
                "name": "Rounded bar", "range": range,
                "transform": [[1620.0, 560.0], [200.0, 50.0], [100.0, 100.0], 0.0, 100.0],
                "path": rounded, "fills": solid([0.0, 1.0, 1.0, 1.0]), "strokes": null
            }),
        ]
    );
}

/// The public matte placement with unchanged native Crop records from cap2.
/// This constituent regression is not a native combined-matte fidelity proof.
#[cfg(feature = "ffmpeg-library")]
fn cropped_color_matte_xml() -> String {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(FIXTURE);
    let mut xml = read_xml(&source);
    edit_record(
        &mut xml,
        "<VideoComponentChain ObjectID=\"156\"",
        "</VideoComponentChain>",
        |record| {
            record.replace(
            "<ComponentChain Version=\"3\">",
            "<ComponentChain Version=\"3\"><Components><Component Index=\"0\" ObjectRef=\"403\"/></Components>",
        )
        },
    );
    xml.replace(
        "</PremiereData>",
        &include_str!("../fixtures/cap2-native-static-crop.xml")
            .replace("<PremiereData Version=\"3\">", ""),
    )
}

#[cfg(feature = "ffmpeg-library")]
fn assert_color_matte_crop(
    document: &Value,
    color: [f64; 4],
    range: Value,
    position: [f64; 2],
    size: [f64; 2],
    hidden: bool,
) {
    let layers = document["composition"]["layers"].as_array().unwrap();
    let owner = layers
        .iter()
        .find(|layer| layer["rect"]["fillColor"] == json!(color))
        .unwrap();
    assert_eq!(owner["type"], "Rect");
    assert_eq!(owner["rect"]["fillEnabled"], true);
    assert_eq!(owner["rect"]["strokeEnabled"], false);
    assert_eq!(owner["rect"]["size"], json!([1920.0, 1080.0]));
    assert_eq!(owner["isHidden"].as_bool().unwrap_or(false), hidden);
    assert_eq!(*crate::test_support::layer_range(owner), range);
    assert_eq!(owner["masks"].as_array().unwrap().len(), 1);
    let mask = &owner["masks"][0];
    assert_eq!(mask["mode"], "add");
    assert_eq!(mask["feather"], json!([0.0, 0.0]));
    let guide = layers
        .iter()
        .find(|layer| layer["id"] == mask["layer"])
        .unwrap();
    assert_ne!(owner["id"], guide["id"]);
    assert_ne!(mask["id"], guide["id"]);
    assert_eq!(guide["type"], "Rect");
    assert_eq!(guide["rect"]["fillEnabled"], false);
    assert_eq!(guide["rect"]["strokeEnabled"], false);
    assert_eq!(guide["rect"]["position"], json!(position));
    let actual_size = &guide["rect"]["size"];
    for (index, expected) in size.into_iter().enumerate() {
        assert!((actual_size[index].as_f64().unwrap() - expected).abs() < 1e-8);
    }
    assert_eq!(guide["parent"], owner["parent"]);
    assert_eq!(*crate::test_support::layer_range(guide), range);
    assert!(!guide["isHidden"].as_bool().unwrap_or(false));
    assert_eq!(guide["transform"]["opacity"], 100.0);
    assert_eq!(owner["transform"]["opacity"], 100.0);
    assert!(owner["effects"].is_null() || owner["effects"].as_array().unwrap().is_empty());
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn native_static_crop_color_matte_keeps_owner_and_nonpainting_guide() {
    for hidden in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        std::fs::copy(
            fixtures.join("video-30fps-10s.mp4"),
            dir.path().join("video-30fps-10s.mp4"),
        )
        .unwrap();
        let mut xml = cropped_color_matte_xml();
        edit_record(
            &mut xml,
            "<VideoClipTrackItem ObjectID=\"157\"",
            "</VideoClipTrackItem>",
            |record| {
                record.replace(
                    "<ClipTrackItem Version=\"8\">",
                    &format!("<ClipTrackItem Version=\"8\"><IsMuted>{hidden}</IsMuted>"),
                )
            },
        );
        let source = dir.path().join("project.prproj");
        write_prproj(&source, &xml);
        let output = dir.path().join("converted");
        let omissions = premiere_to_tesseract(&source, &output, Some(SEQUENCE_UID), false).unwrap();
        assert!(omissions.is_empty(), "{omissions:?}");
        let archive = TesseractFile::open(first_project(&output)).unwrap();
        assert_eq!(
            archive.metadata().assets.len(),
            1,
            "only the ordinary video is packaged"
        );
        let document = archive.project_json().unwrap();
        assert_color_matte_crop(
            &document,
            [1.0, 0.0, 0.0, 1.0],
            json!({"start": 2000, "duration": 3000}),
            [0.0, 0.0],
            [977.1345703124928, 1080.0],
            hidden,
        );
        let blue = document["composition"]["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|layer| layer["rect"]["fillColor"] == json!([0.0, 0.0, 1.0, 1.0]))
            .unwrap();
        assert!(blue["masks"].is_null() || blue["masks"].as_array().unwrap().is_empty());
        assert_eq!(blue["rect"]["size"], json!([1920.0, 1080.0]));
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn native_static_crop_color_matte_unsupported_controls_never_expose_an_unmasked_owner() {
    for (param, from, to, reason) in [
        (
            "817",
            "-91445760000000000,0.,0,0,0,0,0,0",
            "-91445760000000000,NaN,0,0,0,0,0,0",
            "nonfinite initial value",
        ),
        (
            "817",
            "<ParameterID>1</ParameterID>",
            "<IsTimeVarying>true</IsTimeVarying><ParameterID>1</ParameterID>",
            "animated or malformed Crop Left",
        ),
        (
            "821",
            "-91445760000000000,false,0,0,0,0,0,0",
            "-91445760000000000,true,0,0,0,0,0,0",
            "Crop Zoom is unsupported",
        ),
        (
            "822",
            "-91445760000000000,0,0,0,0,0,0,0",
            "-91445760000000000,12,0,0,0,0,0,0",
            "Crop on a Color Matte is unsupported",
        ),
        (
            "156",
            "<Component Index=\"0\" ObjectRef=\"403\"/>",
            "<Component Index=\"0\" ObjectRef=\"403\"/><Component Index=\"1\" ObjectRef=\"405\"/>",
            "duplicate Crop component",
        ),
    ] {
        let mut xml = cropped_color_matte_xml();
        let tag = if param == "156" {
            "VideoComponentChain"
        } else {
            "VideoComponentParam"
        };
        edit_record(
            &mut xml,
            &format!("<{tag} ObjectID=\"{param}\""),
            &format!("</{tag}>"),
            |record| {
                assert!(record.contains(from));
                record.replace(from, to)
            },
        );
        let dir = tempfile::tempdir().unwrap();
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        std::fs::copy(
            fixtures.join("video-30fps-10s.mp4"),
            dir.path().join("video-30fps-10s.mp4"),
        )
        .unwrap();
        let source = dir.path().join("project.prproj");
        write_prproj(&source, &xml);
        let output = dir.path().join("converted");
        let omissions = premiere_to_tesseract(&source, &output, Some(SEQUENCE_UID), false).unwrap();
        assert!(omissions.iter().any(|omission| omission.record.contains("157") && omission.reason.contains(reason)), "{reason}: {omissions:?}");
        let archive = TesseractFile::open(first_project(&output)).unwrap();
        let document = archive.project_json().unwrap();
        assert!(
            !document["composition"]["layers"]
                .as_array()
                .unwrap()
                .iter()
                .any(|layer| layer["rect"]["fillColor"] == json!([1.0, 0.0, 0.0, 1.0])),
            "unsupported Crop must omit the red matte, not retain it unmasked"
        );
    }
}
