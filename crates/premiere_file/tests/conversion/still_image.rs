//! Still-image clips: the derived `premiere_isolated_still_image` fixture through
//! both public conversion directions, plus file-level still rejections.
use super::support::*;
use premiere_file::{Omission, OmissionScope, PrProjectFile};
use serde_json::{json, Value};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};
use tesseract_file::{AssetKind, TesseractFile, TesseractFileBuilder};

const SEQUENCE: &str = "c8acf9c1-34b2-4086-9f55-d528950a7059";
const FIXTURE: &str = "premiere_isolated_still_image.prproj";
const JPEG: &str = "feature_still_opaque.jpg";
const PNG: &str = "feature_still_transparent.png";
const VIDEO: &str = "video-30fps-10s.mp4";

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// Stage the fixture and its media in a scratch directory so tests can alter bytes.
fn staged_fixture(root: &Path) -> PathBuf {
    for name in [FIXTURE, JPEG, PNG, VIDEO] {
        fs::copy(fixtures().join(name), root.join(name)).unwrap();
    }
    root.join(FIXTURE)
}

fn asset_bytes(file: &TesseractFile, asset_id: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    file.asset(asset_id)
        .unwrap()
        .open()
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    bytes
}

fn layers_of(document: &Value, kind: &str) -> Vec<Value> {
    document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|layer| layer["type"] == kind)
        .cloned()
        .collect()
}

/// The asset ID whose packaged file keeps the fixture name `name`.
fn asset_named(file: &TesseractFile, name: &str) -> String {
    file.metadata()
        .assets
        .iter()
        .find(|(_, descriptor)| descriptor.path.ends_with(name))
        .map(|(id, _)| id.clone())
        .unwrap()
}

fn still_table(project: &PrProjectFile) -> Vec<(String, bool, [i64; 2], [i64; 2])> {
    let sequence = project.sequences().next().unwrap();
    let mut rows: Vec<_> = sequence
        .video_occurrences()
        .map(|clip| {
            let media = project.media(clip).unwrap();
            let timeline = clip.timeline_ticks();
            let source = clip.source_ticks();
            (
                media.name().to_owned(),
                media.is_still(),
                [timeline.start, timeline.end],
                [source.start, source.end],
            )
        })
        .collect();
    rows.sort();
    rows
}

/// The fixture JPEG with one more segment after SOI.
fn jpeg_with_segment(marker: u8, payload: &[u8]) -> Vec<u8> {
    let jpeg = fs::read(fixtures().join(JPEG)).unwrap();
    let mut edited = jpeg[..2].to_vec();
    edited.extend_from_slice(&[0xff, marker]);
    edited.extend_from_slice(&(payload.len() as u16 + 2).to_be_bytes());
    edited.extend_from_slice(payload);
    edited.extend_from_slice(&jpeg[2..]);
    edited
}

/// The fixture JPEG with an Exif APP1 segment declaring orientation 3 (180°).
fn rotated_jpeg() -> Vec<u8> {
    // Big-endian TIFF header, IFD0 with one entry: Orientation, SHORT, count 1, value 3.
    jpeg_with_segment(
        0xe1,
        b"Exif\0\0MM\0*\0\0\0\x08\0\x01\x01\x12\0\x03\0\0\0\x01\0\x03\0\0\0\0\0\0",
    )
}

/// A canvas-sized 8-bit RGB PNG (colour type 2, no `tRNS`): opaque by construction.
/// Inspection decodes only the headers, so the image data is a stub.
fn opaque_rgb_png() -> Vec<u8> {
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&1920_u32.to_be_bytes());
    ihdr.extend_from_slice(&1080_u32.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    for (chunk_type, data) in [
        (b"IHDR", ihdr),
        (b"IDAT", vec![0; 4]),
        (b"IEND", Vec::new()),
    ] {
        let mut crc = flate2::Crc::new();
        crc.update(chunk_type);
        crc.update(&data);
        png.extend_from_slice(&(data.len() as u32).to_be_bytes());
        png.extend_from_slice(chunk_type);
        png.extend_from_slice(&data);
        png.extend_from_slice(&crc.sum().to_be_bytes());
    }
    png
}

/// The fixture's editable document and its assets as (asset ID, fixture file, kind).
fn imported(root: &Path) -> (Value, Vec<(String, PathBuf, AssetKind)>) {
    let output = root.join("imported");
    premiere_to_tesseract(fixtures().join(FIXTURE), &output, Some(SEQUENCE), false).unwrap();
    let file = TesseractFile::open(&project_files(&output)[0]).unwrap();
    let assets = file
        .metadata()
        .assets
        .iter()
        .map(|(id, descriptor)| {
            let name = Path::new(&descriptor.path).file_name().unwrap();
            (id.clone(), fixtures().join(name), descriptor.kind)
        })
        .collect();
    (file.project_json().unwrap(), assets)
}

fn package(path: &Path, document: &Value, assets: &[(String, PathBuf, AssetKind)]) {
    let mut builder =
        TesseractFileBuilder::from_project_json(&serde_json::to_vec(document).unwrap()).unwrap();
    for (id, source, kind) in assets {
        builder = builder.add_asset(id.as_str(), source, *kind).unwrap();
    }
    builder.write(path).unwrap();
}

#[test]
fn derived_still_fixture_imports_editable_image_layers_and_packages_original_images() {
    let dir = tempfile::tempdir().unwrap();
    let source = fixtures().join(FIXTURE);
    let native = PrProjectFile::load(&source).unwrap().0;
    assert_eq!(native.sequences().len(), 1);
    let output = dir.path().join("converted");
    let omissions = premiere_to_tesseract(&source, &output, Some(SEQUENCE), false).unwrap();
    assert!(
        omissions
            .iter()
            .all(|item| item.scope == OmissionScope::Feature),
        "{omissions:?}"
    );
    let projects = project_files(&output);
    assert_eq!(projects.len(), 1);
    let file = TesseractFile::open(&projects[0]).unwrap();
    let document = file.project_json().unwrap();
    assert_eq!(document["duration"], 10.0);

    let images = layers_of(&document, "Image");
    assert_eq!(images.len(), 2);
    let jpeg = images
        .iter()
        .find(|layer| (*crate::test_support::layer_range(layer))["start"] == 1000)
        .unwrap();
    let png = images
        .iter()
        .find(|layer| (*crate::test_support::layer_range(layer))["start"] == 5000)
        .unwrap();
    assert_eq!(
        (*crate::test_support::layer_range(jpeg)),
        json!({"start": 1000, "duration": 3000})
    );
    assert_eq!(
        (*crate::test_support::layer_range(png)),
        json!({"start": 5000, "duration": 3000})
    );
    for layer in [jpeg, png] {
        assert_eq!(layer["source"]["fit"], "contain");
        assert_eq!(
            layer["source"]["sourceRect"],
            json!({"x": 0.0, "y": 0.0, "width": 1920.0, "height": 1080.0})
        );
        assert!(layer.get("sourceRange").is_none());
        // Default Motion: the picture's centre at the canvas centre.
        assert_eq!(layer["transform"]["anchorPoint"], json!([960.0, 540.0]));
        assert_eq!(layer["transform"]["position"], json!([960.0, 540.0]));
    }
    let video = layers_of(&document, "Video");
    assert_eq!(video.len(), 1);
    assert_eq!(
        (*crate::test_support::layer_range(&video[0])),
        json!({"start": 0, "duration": 10000})
    );
    // Stills paint above the video track.
    let order: Vec<_> = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|layer| layer["type"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(order, ["Image", "Image", "Video", "Rect"]);

    let assets = &file.metadata().assets;
    assert_eq!(assets.len(), 3);
    for (layer, name, content_type) in [(jpeg, JPEG, "image/jpeg"), (png, PNG, "image/png")] {
        let asset_id = layer["source"]["assetId"].as_str().unwrap();
        let descriptor = &assets[asset_id];
        assert_eq!(descriptor.kind, AssetKind::Image);
        assert_eq!(descriptor.content_type, content_type);
        assert!(descriptor.path.ends_with(name));
        // The transparent PNG is packaged byte for byte; its alpha survives untouched.
        assert_eq!(
            asset_bytes(&file, asset_id),
            fs::read(fixtures().join(name)).unwrap()
        );
    }

    let reencoded = dir.path().join("premiere");
    tesseract_to_premiere(&projects[0], &reencoded, false).unwrap();
    for name in [JPEG, PNG, VIDEO] {
        assert_eq!(
            fs::read(reencoded.join("media").join(name)).unwrap(),
            fs::read(fixtures().join(name)).unwrap()
        );
    }
    let rebuilt = PrProjectFile::load(reencoded.join("project.prproj"))
        .unwrap()
        .0;
    assert_eq!(still_table(&rebuilt), still_table(&native));
    assert_eq!(still_table(&rebuilt).len(), 3);
    // The written project declares straight alpha for the PNG only, and every
    // clip keeps Premiere's default Motion.
    let xml = read_xml(&reencoded.join("project.prproj"));
    assert!(!xml.contains("AE.ADBE Motion"));
    assert_eq!(xml.matches("<IsStill>true</IsStill>").count(), 2);
    assert_eq!(xml.matches("<AlphaType>1</AlphaType>").count(), 1);
    assert!(xml.contains(&format!("<RelativePath>./media/{PNG}</RelativePath>")));
}

#[test]
fn still_files_that_contradict_their_native_record_omit_only_their_occurrence() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let source = staged_fixture(root);
    // A GIF under the PNG's name is neither PNG nor JPEG data.
    fs::write(root.join(PNG), b"GIF89a\x01\x00\x01\x00\x00\x00\x00;").unwrap();
    let output = root.join("gif");
    let omissions = premiere_to_tesseract(&source, &output, Some(SEQUENCE), false).unwrap();
    let dropped: Vec<_> = omissions
        .iter()
        .filter(|item| item.scope == OmissionScope::Occurrence)
        .collect();
    assert_eq!(dropped.len(), 1, "{omissions:?}");
    assert!(
        dropped[0].reason.contains("image/gif"),
        "{}",
        dropped[0].reason
    );
    let file = TesseractFile::open(&project_files(&output)[0]).unwrap();
    assert_eq!(file.metadata().assets.len(), 2);
    assert_eq!(layers_of(&file.project_json().unwrap(), "Image").len(), 1);

    type Alter = Box<dyn Fn(&Path)>;
    let rewrite_xml = |edit: fn(String) -> String| -> Alter {
        Box::new(move |root: &Path| {
            let source = root.join(FIXTURE);
            write_prproj(&source, &edit(read_xml(&source)));
        })
    };
    let cases: [(&str, Alter, &str); 5] = [
        (
            "JPEG bytes under the PNG name",
            Box::new(|root: &Path| {
                fs::copy(fixtures().join(JPEG), root.join(PNG)).unwrap();
            }),
            "extension does not match",
        ),
        (
            "opaque RGB PNG declared with straight alpha",
            Box::new(|root: &Path| fs::write(root.join(PNG), opaque_rgb_png()).unwrap()),
            "has no alpha channel although its native AlphaType declares straight alpha",
        ),
        (
            // Premiere would render the undeclared alpha channel opaque.
            "transparent PNG without AlphaType",
            rewrite_xml(|xml| xml.replace("<AlphaType>1</AlphaType>", "")),
            "carries alpha that its native AlphaType does not declare",
        ),
        (
            "JPEG with Exif orientation 3",
            Box::new(|root: &Path| fs::write(root.join(JPEG), rotated_jpeg()).unwrap()),
            "Exif orientation 3 is unsupported",
        ),
        (
            // The corpus has two .psd stills declared with AlphaType 1.
            "Photoshop still",
            {
                let retarget = rewrite_xml(|xml| {
                    xml.replace(
                        "<RelativePath>feature_still_transparent.png</RelativePath>",
                        "<RelativePath>feature_still_transparent.psd</RelativePath>",
                    )
                });
                Box::new(move |root: &Path| {
                    retarget(root);
                    fs::write(root.join("feature_still_transparent.psd"), b"8BPS\0\x01").unwrap();
                })
            },
            "still media must be a PNG or JPEG file",
        ),
    ];
    for (name, alter, expected) in cases {
        let dir = tempfile::tempdir().unwrap();
        let source = staged_fixture(dir.path());
        alter(dir.path());
        let omissions =
            premiere_to_tesseract(&source, dir.path().join("out"), Some(SEQUENCE), true).unwrap();
        let dropped: Vec<_> = omissions
            .iter()
            .filter(|item| item.scope == OmissionScope::Occurrence)
            .collect();
        assert_eq!(dropped.len(), 1, "{name}: {omissions:?}");
        assert!(
            dropped[0].reason.contains(expected),
            "{name}: {}",
            dropped[0].reason
        );
    }
}

#[test]
fn export_rejects_packaged_stills_whose_name_kind_or_orientation_misrepresent_the_image() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (document, assets) = imported(root);
    // PNG bytes under a .jpg name would reach Premiere declared as JPEG.
    let misnamed = root.join("transparent.jpg");
    fs::copy(fixtures().join(PNG), &misnamed).unwrap();
    // The renderer would draw this JPEG turned 180°.
    let rotated = root.join("rotated.jpg");
    fs::write(&rotated, rotated_jpeg()).unwrap();
    for (index, (replaced, source, kind, expected)) in [
        (
            PNG,
            misnamed,
            AssetKind::Image,
            "packaged still file extension does not match its image data",
        ),
        (
            PNG,
            fixtures().join(PNG),
            AssetKind::Video,
            "writer requires a packaged PNG or JPEG image asset with matching content type",
        ),
        (JPEG, rotated, AssetKind::Image, "Exif orientation 3"),
    ]
    .into_iter()
    .enumerate()
    {
        let edited: Vec<_> = assets
            .iter()
            .map(|(id, path, asset_kind)| {
                if path.ends_with(replaced) {
                    (id.clone(), source.clone(), kind)
                } else {
                    (id.clone(), path.clone(), *asset_kind)
                }
            })
            .collect();
        let archive = root.join(format!("edited-{index}.tsrct"));
        package(&archive, &document, &edited);
        let output = root.join(format!("out-{index}"));
        let error = tesseract_to_premiere(&archive, &output, true)
            .unwrap_err()
            .to_string();
        assert!(error.contains(expected), "{expected}: {error}");
        assert!(!output.exists());
    }
}

#[test]
fn embedded_icc_profile_is_reported_in_both_directions_and_the_still_kept() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let source = staged_fixture(root);
    fs::write(
        root.join(JPEG),
        jpeg_with_segment(0xe2, b"ICC_PROFILE\0\x01\x01profile"),
    )
    .unwrap();
    let note = "still image embeds an ICC colour profile; colours are kept as stored and parity with Premiere colour management is inferred";
    let notes = |omissions: Vec<premiere_file::Omission>| -> Vec<(OmissionScope, String)> {
        omissions
            .into_iter()
            .filter(|omission| omission.reason == note)
            .map(|omission| (omission.scope, omission.record))
            .collect()
    };
    let output = root.join("tesseract");
    let imported = premiere_to_tesseract(&source, &output, Some(SEQUENCE), false).unwrap();
    let archive = project_files(&output)[0].clone();
    let file = TesseractFile::open(&archive).unwrap();
    assert_eq!(layers_of(&file.project_json().unwrap(), "Image").len(), 2);
    let exported = tesseract_to_premiere(&archive, root.join("premiere"), false).unwrap();
    // One note for the JPEG placement on import and for its layer on export.
    let feature = |record: &str| vec![(OmissionScope::Feature, record.to_owned())];
    assert_eq!(notes(imported), feature("VideoClipTrackItem:153"));
    assert_eq!(notes(exported), feature("layer 2 (\"Premiere still 2\")"));
}

#[test]
fn repeated_still_placements_share_one_asset_while_separate_records_stay_separate() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    // Repeat the JPEG layer at 8–9 s on the same track.
    let (mut document, assets) = imported(root);
    let layers = document["composition"]["layers"].as_array_mut().unwrap();
    let mut repeat = layers
        .iter()
        .find(|layer| (*crate::test_support::layer_range(layer))["start"] == 1000)
        .unwrap()
        .clone();
    repeat["id"] = json!(99);
    repeat["activeRange"] = json!({"start": 8000, "duration": 1000});
    layers.insert(0, repeat);
    let archive = root.join("repeated.tsrct");
    package(&archive, &document, &assets);
    let native = root.join("native");
    tesseract_to_premiere(&archive, &native, false).unwrap();
    // Three still placements, two still media records, three packaged files.
    assert_eq!(
        read_xml(&native.join("project.prproj"))
            .matches("<IsStill>true</IsStill>")
            .count(),
        2
    );
    assert_eq!(fs::read_dir(native.join("media")).unwrap().count(), 3);
    let again = root.join("again");
    premiere_to_tesseract(native.join("project.prproj"), &again, None, false).unwrap();
    let file = TesseractFile::open(&project_files(&again)[0]).unwrap();
    assert_eq!(file.metadata().assets.len(), 3);
    let jpeg = asset_named(&file, JPEG);
    let mut starts: Vec<_> = layers_of(&file.project_json().unwrap(), "Image")
        .into_iter()
        .filter(|layer| layer["source"]["assetId"] == jpeg.as_str())
        .map(|layer| {
            (*crate::test_support::layer_range(&layer))["start"]
                .as_i64()
                .unwrap()
        })
        .collect();
    starts.sort_unstable();
    assert_eq!(starts, [1000, 8000]);

    // Two native still records naming one file remain two assets both ways.
    let source = staged_fixture(root);
    let xml = read_xml(&source)
        .replace(
            "<RelativePath>feature_still_transparent.png</RelativePath>",
            &format!("<RelativePath>{JPEG}</RelativePath>"),
        )
        .replace("<AlphaType>1</AlphaType>", "");
    write_prproj(&source, &xml);
    let output = root.join("two-records");
    let omissions = premiere_to_tesseract(&source, &output, Some(SEQUENCE), false).unwrap();
    assert!(
        omissions
            .iter()
            .all(|item| item.scope == OmissionScope::Feature),
        "{omissions:?}"
    );
    let input = project_files(&output)[0].clone();
    let file = TesseractFile::open(&input).unwrap();
    let images = layers_of(&file.project_json().unwrap(), "Image");
    assert_eq!(images.len(), 2);
    assert_ne!(
        images[0]["source"]["assetId"],
        images[1]["source"]["assetId"]
    );
    for layer in &images {
        let asset_id = layer["source"]["assetId"].as_str().unwrap();
        assert_eq!(
            asset_bytes(&file, asset_id),
            fs::read(fixtures().join(JPEG)).unwrap()
        );
    }
    let native = root.join("two-records-native");
    tesseract_to_premiere(&input, &native, false).unwrap();
    assert_eq!(
        read_xml(&native.join("project.prproj"))
            .matches("<IsStill>true</IsStill>")
            .count(),
        2
    );
    assert_eq!(fs::read_dir(native.join("media")).unwrap().count(), 3);
}

/// The parameter records of each `match_name` component in `xml`, in document
/// order, without their `ObjectID` and with key times counted from each
/// parameter's first key.
fn component_params(xml: &str, match_name: &str) -> Vec<Vec<String>> {
    let document = roxmltree::Document::parse(xml).unwrap();
    let records: std::collections::BTreeMap<_, _> = document
        .root_element()
        .children()
        .filter_map(|node| Some((node.attribute("ObjectID")?, node)))
        .collect();
    let param = |reference: &str| {
        let record = records[reference];
        let mut text = xml[record.range()].replacen(&format!(" ObjectID=\"{reference}\""), "", 1);
        let keys = record
            .children()
            .find(|child| child.has_tag_name("Keyframes"))
            .and_then(|keys| keys.text());
        if let Some(keys) = keys {
            let time = |row: &str| -> i64 { row.split(',').next().unwrap().parse().unwrap() };
            let first = time(keys);
            let counted: String = keys
                .split_terminator(';')
                .map(|row| format!("{},{};", time(row) - first, row.split_once(',').unwrap().1))
                .collect();
            text = text.replace(keys, &counted);
        }
        text
    };
    document
        .root_element()
        .children()
        .filter(|node| {
            node.children()
                .any(|child| child.has_tag_name("MatchName") && child.text() == Some(match_name))
        })
        .map(|component| {
            component
                .descendants()
                .filter(|node| node.has_tag_name("Param"))
                .map(|reference| param(reference.attribute("ObjectRef").unwrap()))
                .collect()
        })
        .collect()
}

#[test]
fn still_motion_opacity_and_keys_export_as_a_videos_do() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (mut document, assets) = imported(root);
    // The 1920x1080 JPEG and video get one transform, Opacity 20, and the same
    // Position, Scale, Rotation and Opacity keys over their first second.
    let mut entries = Vec::new();
    for layer in document["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .filter(|layer| {
            layer["type"] == "Video" || (*crate::test_support::layer_range(layer))["start"] == 1000
        })
    {
        layer["transform"] = json!({
            "anchorPoint": [480, 810], "position": [1228.8, 464.4], "scale": [135, 135],
            "rotation": 27, "opacity": 20
        });
        let id = &layer["id"];
        for (property, [first, last]) in [
            ("positionX", [1228.8, 960.0]),
            ("positionY", [464.4, 540.0]),
            ("scaleX", [135.0, 50.0]),
            ("scaleY", [135.0, 50.0]),
            ("rotation", [27.0, -13.0]),
            ("opacity", [20.0, 100.0]),
        ] {
            entries.push(json!({
                "target": {"kind": "layer", "layerId": id, "propertyType": property},
                "animator": {"type": "keyframes", "enabled": true, "keyframes": [
                    {"id": format!("{property}-{id}-a"), "layerTime": 0, "value": {"type": "float", "value": first}, "easing": {"type": "linear"}},
                    {"id": format!("{property}-{id}-b"), "layerTime": 1000, "value": {"type": "float", "value": last}, "easing": {"type": "linear"}}
                ]}
            }));
        }
    }
    assert_eq!(entries.len(), 12);
    document["composition"]["dynamics"] = json!({"entries": entries});
    let archive = root.join("edited.tsrct");
    package(&archive, &document, &assets);
    let output = root.join("premiere");
    let omissions = tesseract_to_premiere(&archive, &output, false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let xml = read_xml(&output.join("project.prproj"));
    // The still's keys are on its synthetic clock, from its one-hour in-point.
    assert!(xml.contains("<Keyframes>914457600000000,"));
    // One writer: both clips' Motion and Opacity records are the same bytes,
    // their keys counted from each clip's in-point.
    for (component, params) in [("AE.ADBE Motion", 7), ("AE.ADBE Opacity", 3)] {
        let written = component_params(&xml, component);
        assert_eq!(written.len(), 2, "{component}");
        assert_eq!(written[0].len(), params, "{component}");
        assert_eq!(written[0], written[1], "{component}");
    }
    let opacity = &component_params(&xml, "AE.ADBE Opacity")[0][0];
    assert!(
        opacity.contains("<StartKeyframe>-91445760000000000,20,")
            && opacity.contains("<Keyframes>0,20,"),
        "{opacity}"
    );
}

/// `premiere_isolated_images_nests_26_5` (Premiere 26.5.1, Oracle IN) over the
/// timecoded video: stills A 960x540, B 3840x2160 and C 1080x1350 at default
/// Motion, D (B's image) with Scale to Frame Size and E (C's image) with
/// Position and Scale keys, 2 s each from 0 s. F, A's image with static
/// Motion, keeps a 5 s source span on its 2 s placement (the Oracle's
/// duration fallback), which the shared timeline rules reject.
const IMAGES_NESTS: &str = "feature_images_nests_26_5.prproj";
const IMAGES_NESTS_SEQUENCE: &str = "f3c651e6-0302-4499-b6f5-814b7b22c207";

/// Each image layer, by start: its start, source frame, transform and keys
/// as `[layer time, value, easing]` by property.
fn image_rows(document: &Value) -> Vec<Value> {
    let entries = document["composition"]["dynamics"]["entries"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let mut rows: Vec<_> = layers_of(document, "Image")
        .into_iter()
        .map(|layer| {
            let keys: serde_json::Map<_, _> = entries
                .iter()
                .filter(|entry| entry["target"]["layerId"] == layer["id"])
                .map(|entry| {
                    let keys = entry["animator"]["keyframes"].as_array().unwrap().iter();
                    (
                        entry["target"]["propertyType"].as_str().unwrap().to_owned(),
                        keys.map(|key| {
                            json!([key["layerTime"], key["value"]["value"], key["easing"]])
                        })
                        .collect(),
                    )
                })
                .collect();
            json!({
                "start": (*crate::test_support::layer_range(&layer))["start"],
                "sourceRect": layer["source"]["sourceRect"],
                "transform": layer["transform"],
                "keys": keys,
            })
        })
        .collect();
    rows.sort_by_key(|row| row["start"].as_i64());
    rows
}

#[test]
fn adobe_stills_of_three_sizes_import_at_their_native_size_and_export_their_motion() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let imported = root.join("imported");
    let omissions = premiere_to_tesseract(
        fixtures().join(IMAGES_NESTS),
        &imported,
        Some(IMAGES_NESTS_SEQUENCE),
        false,
    )
    .unwrap();
    let omitted = |record: &str| {
        omissions
            .iter()
            .find(|omission| {
                omission.scope == OmissionScope::Occurrence && omission.record == record
            })
            .map(|omission| omission.reason.as_str())
    };
    assert_eq!(
        omitted("144"),
        Some("unsupported conversion: VideoClip:265: Scale to Frame Size on a still is not converted")
    );
    assert_eq!(
        omitted("146"),
        Some("invalid Premiere project: source span does not match the constant playback rate")
    );
    let document = TesseractFile::open(&project_files(&imported)[0])
        .unwrap()
        .project_json()
        .unwrap();
    let rows = image_rows(&document);
    // G1: default Motion centres each still at its pixel size, B although
    // its master clip (shared with D) carries Scale to Frame Size.
    let sizes = [
        (0, [960.0, 540.0]),
        (2000, [3840.0, 2160.0]),
        (4000, [1080.0, 1350.0]),
        (8000, [1080.0, 1350.0]),
    ];
    assert_eq!(rows.len(), sizes.len());
    for (row, (start, [width, height])) in rows.iter().zip(sizes) {
        assert_eq!(row["start"], start);
        assert_eq!(
            row["sourceRect"],
            json!({"x": 0.0, "y": 0.0, "width": width, "height": height})
        );
        let transform = &row["transform"];
        assert_eq!(
            transform["anchorPoint"],
            json!([width / 2.0, height / 2.0]),
            "{start}"
        );
        assert_eq!(transform["position"], json!([960.0, 540.0]), "{start}");
        assert_eq!(transform["scale"], json!([100.0, 100.0]), "{start}");
        assert_eq!(transform["opacity"], json!(100.0), "{start}");
    }
    // G4: E's Position keys at 0.5 s and 1.5 s after its in-point, and its
    // Scale keys, whose Bezier ease keeps the native handles (1/6, 1/60) and
    // (5/6, 59/60).
    let linear = json!({"type": "linear"});
    let ease = &rows[3]["keys"]["scaleX"][1][2];
    for (handle, value) in [
        ("x1", 1.0 / 6.0),
        ("y1", 1.0 / 60.0),
        ("x2", 5.0 / 6.0),
        ("y2", 59.0 / 60.0),
    ] {
        assert!(
            (ease[handle].as_f64().unwrap() - value).abs() < 1e-12,
            "{ease}"
        );
    }
    let track = |[first, last]: [f64; 2], easing: &Value| {
        json!([[500, first, linear], [1500, last, easing]])
    };
    assert_eq!(
        rows[3]["keys"],
        json!({
            "positionX": track([960.0, 960.0], &linear),
            "positionY": track([540.0, 324.0], &linear),
            "scaleX": track([100.0, 60.0], ease),
            "scaleY": track([100.0, 60.0], ease),
        })
    );
    assert!(rows[..3].iter().all(|row| row["keys"] == json!({})));

    // Export writes each still at its pixel size with that Motion, never
    // Scale to Frame Size, and the stills import back unchanged.
    let exported = root.join("premiere");
    tesseract_to_premiere(&project_files(&imported)[0], &exported, false).unwrap();
    let project = exported.join("project.prproj");
    assert!(!read_xml(&project).contains("ScaleToFramePolicy"));
    let reimported = root.join("reimported");
    let root_sequence = exported_root_sequence(&project);
    premiere_to_tesseract(&project, &reimported, Some(&root_sequence), false).unwrap();
    let document = TesseractFile::open(&project_files(&reimported)[0])
        .unwrap()
        .project_json()
        .unwrap();
    assert_eq!(image_rows(&document), rows);
}

/// The nest "Inner" of the images/nests fixture and its one audio layer.
fn nest_sound(document: &Value) -> (Value, Value) {
    let group = layers_of(document, "Group")
        .into_iter()
        .find(|group| group["name"] == "Inner")
        .expect("nest N imports as a group");
    let sound = group["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["type"] == "Audio")
        .expect("the nest keeps its inner sound")
        .clone();
    (group, sound)
}

#[test]
fn adobe_nest_sound_imports_through_its_audio_item_and_exports_linked() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let imported = root.join("imported");
    let omissions = premiere_to_tesseract(
        fixtures().join(IMAGES_NESTS),
        &imported,
        Some(IMAGES_NESTS_SEQUENCE),
        false,
    )
    .unwrap();
    // G6: audio item 115 pairs with nest 116 over 10-14 s; nothing of N is
    // omitted but I2 (source span).
    for record in ["115", "116"] {
        assert!(
            !omissions.iter().any(|omission| omission.record == record),
            "{record}: {omissions:?}"
        );
    }
    let document = TesseractFile::open(&project_files(&imported)[0])
        .unwrap()
        .project_json()
        .unwrap();
    let (group, sound) = nest_sound(&document);
    assert_eq!(
        (*crate::test_support::layer_range(&group)),
        json!({"start": 10000, "duration": 4000})
    );
    assert_eq!(sound["parent"], group["id"]);
    assert_eq!(
        (*crate::test_support::layer_range(&sound)),
        json!({"start": 0, "duration": 4000})
    );
    assert_eq!(sound["sourceRange"], json!({"start": 0, "duration": 4000}));
    // G7: I4's Level 10^((-6-15)/20) plays at -6 dB through the default item.
    let volume = sound["volume"].as_f64().unwrap();
    // Premiere stores the Level as 0.089125096797943115, 3e-9 off 10^(-21/20).
    assert!((volume - 10f64.powf(-6.0 / 20.0)).abs() < 1e-7, "{volume}");

    // Sequences, audio items and links that an export writes.
    let written = |project: &Path| {
        let xml = read_xml(project);
        [
            "<Sequence ObjectUID=",
            "<AudioClipTrackItem ",
            "<Link ObjectID=",
        ]
        .map(|tag| xml.matches(tag).count())
    };
    // N, with its video I1, exports as a nest with its sound on an inner
    // audio track and one linked audio item (IN2 export gate, Part B), and
    // reimports with both.
    let exported = root.join("premiere");
    let omissions = tesseract_to_premiere(&project_files(&imported)[0], &exported, false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let project = exported.join("project.prproj");
    // The nest's sequence; its audio item and the inner sound; the link.
    assert_eq!(written(&project), [2, 2, 1]);
    let reimported = root.join("reimported");
    let omissions = premiere_to_tesseract(
        &project,
        &reimported,
        Some(&exported_root_sequence(&project)),
        false,
    )
    .unwrap();
    let reread = TesseractFile::open(&project_files(&reimported)[0])
        .unwrap()
        .project_json()
        .unwrap();
    let (group_back, sound_back) = nest_sound(&reread);
    assert_eq!(
        (*crate::test_support::layer_range(&group_back)),
        (*crate::test_support::layer_range(&group)),
        "{omissions:?}"
    );
    assert!(group_back["layers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|layer| layer["type"] == "Video"));
    for field in ["activeRange", "sourceRange", "volume"] {
        assert_eq!(sound_back[field], sound[field], "{field}");
    }

    // Without its video, N holds only sound, which Premiere draws as opaque
    // black (IN2 export gate, Part A): it is not exported, and nothing of its
    // sound reaches the export. Without the root's linked-A/V video too, the
    // sound's media belongs to N alone and is not packaged.
    let mut edited = document.clone();
    let layers = edited["composition"]["layers"].as_array_mut().unwrap();
    layers.retain(|layer| {
        layer["type"] != "Video" || layer["source"]["assetId"] != sound["source"]["assetId"]
    });
    layers
        .iter_mut()
        .find(|layer| layer["id"] == group["id"])
        .unwrap()["layers"]
        .as_array_mut()
        .unwrap()
        .retain(|layer| layer["type"] != "Video");
    let file = TesseractFile::open(&project_files(&imported)[0]).unwrap();
    let assets: Vec<_> = file
        .metadata()
        .assets
        .iter()
        .map(|(id, descriptor)| {
            let name = Path::new(&descriptor.path).file_name().unwrap();
            (id.clone(), fixtures().join(name), descriptor.kind)
        })
        .collect();
    let edited_path = root.join("edited.tsrct");
    package(&edited_path, &edited, &assets);
    let exported = root.join("premiere-without-picture");
    let omissions = tesseract_to_premiere(&edited_path, &exported, false).unwrap();
    let nest = format!("layer {} (\"Inner\")", group["id"]);
    assert_eq!(
        omissions
            .iter()
            .map(|omission| (omission.record.as_str(), omission.reason.as_str()))
            .collect::<Vec<_>>(),
        [(
            nest.as_str(),
            "a group with sound but no picture is not exported: Premiere draws a nested sequence without video as opaque black"
        )]
    );
    assert_eq!(written(&exported.join("project.prproj")), [1, 0, 0]);
    assert!(!exported.join("media/feature_linked_av_source.mp4").exists());
}

/// N of the images/nests fixture with its video item 116 disabled, as
/// Premiere 26.5.1 saves a disabled nest whose audio item stays enabled
/// (`premiere_isolated_hidden_nest_26_5`, items 89/90; the fixture's pinned
/// render has no nest sound); that Premiere then hides only the picture and
/// still plays the sound is inferred from per-item Enable. N imports as a
/// hidden group of its picture and its audio item 115 plays alone; export
/// writes a disabled nest without sound and the sound as an audio item of
/// the root sequence, which reimport reads back as it was imported.
#[test]
fn a_nest_sound_under_a_hidden_picture_plays_alone_and_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut xml = read_xml(&fixtures().join(IMAGES_NESTS));
    let start = xml.find(r#"<VideoClipTrackItem ObjectID="116""#).unwrap();
    let end = start + xml[start..].find("</ClipTrackItem>").unwrap();
    xml.insert_str(end, "<IsMuted>true</IsMuted>");
    let source = root.join(IMAGES_NESTS);
    write_prproj(&source, &xml);
    for name in [
        "feature_linked_av_source.mp4",
        "feature_timecoded_source.mp4",
        "in_small.png",
        "in_large.png",
        "in_portrait.png",
    ] {
        fs::copy(fixtures().join(name), root.join(name)).unwrap();
    }
    let imported = root.join("imported");
    let omissions =
        premiere_to_tesseract(&source, &imported, Some(IMAGES_NESTS_SEQUENCE), false).unwrap();
    assert!(
        !omissions
            .iter()
            .any(|omission| omission.record.ends_with("115")),
        "{omissions:?}"
    );
    // The hidden picture and the sound that plays alone over N's range, from
    // the linked-A/V source at I4's -6 dB through the item.
    let hidden_picture_and_sound = |imported: &Path| {
        let file = TesseractFile::open(&project_files(imported)[0]).unwrap();
        let document = file.project_json().unwrap();
        let group = layers_of(&document, "Group")
            .into_iter()
            .find(|group| group["name"] == "Inner")
            .expect("N imports as a group");
        assert_eq!(group["isHidden"], true);
        let children: Vec<_> = group["layers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|layer| layer["type"].clone())
            .collect();
        assert_eq!(children, [json!("Video")]);
        let [sound] = layers_of(&document, "Audio").try_into().unwrap();
        assert_eq!(
            sound["source"]["assetId"],
            asset_named(&file, "feature_linked_av_source.mp4")
        );
        assert_eq!(
            (*crate::test_support::layer_range(&sound)),
            json!({"start": 10000, "duration": 4000})
        );
        assert_eq!(sound["sourceRange"], json!({"start": 0, "duration": 4000}));
        let volume = sound["volume"].as_f64().unwrap();
        assert!((volume - 10f64.powf(-6.0 / 20.0)).abs() < 1e-7, "{volume}");
        sound
    };
    let sound = hidden_picture_and_sound(&imported);

    let exported = root.join("premiere");
    let omissions = tesseract_to_premiere(&project_files(&imported)[0], &exported, false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let project = exported.join("project.prproj");
    let xml = read_xml(&project);
    // N's sequence and the root; one audio item, of media; no nest audio item or link.
    for (tag, count) in [
        ("<Sequence ObjectUID=", 2),
        ("<AudioClipTrackItem ", 1),
        ("<AudioMediaSource ", 1),
        ("<Link ObjectID=", 0),
    ] {
        assert_eq!(xml.matches(tag).count(), count, "{tag}");
    }
    let reimported = root.join("reimported");
    let root_sequence = exported_root_sequence(&project);
    let omissions =
        premiere_to_tesseract(&project, &reimported, Some(&root_sequence), false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let sound_back = hidden_picture_and_sound(&reimported);
    for field in ["activeRange", "sourceRange", "volume"] {
        assert_eq!(sound_back[field], sound[field], "{field}");
    }
}

/// A volume key as (layer time, gain, easing type).
type VolumeKey = (i64, f64, String);
/// A sound's volume and its volume keys, `None` without a key track.
type SoundVolume = (f64, Option<Vec<VolumeKey>>);

/// The volume keys of the audio layer `layer`, or `None` without a key
/// track.
fn volume_keys(document: &Value, layer: &Value) -> Option<Vec<VolumeKey>> {
    let entry = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| {
            entry["target"]
                == json!({"kind": "layer", "layerId": layer["id"], "propertyType": "volume"})
        })?;
    Some(
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
            .collect(),
    )
}

/// Checks the volume of a sound and its keys, `expected` scaled by `gain`,
/// or that it has no key track.
fn assert_volume(
    (volume, keys): SoundVolume,
    expected_volume: f64,
    expected: Option<&[(i64, f64, &str)]>,
    gain: f64,
) {
    assert!((volume - expected_volume).abs() < 1e-7, "{volume}");
    let Some(expected) = expected else {
        assert_eq!(keys, None);
        return;
    };
    let keys = keys.expect("the sound has volume keys");
    assert_eq!(keys.len(), expected.len(), "{keys:?}");
    for ((time, value, easing), (expected_time, expected_value, expected_easing)) in
        keys.iter().zip(expected)
    {
        assert_eq!((time, easing.as_str()), (expected_time, *expected_easing));
        assert!((value - expected_value * gain).abs() < 1e-7, "{keys:?}");
    }
}

/// Item 115's clip Volume of its own, in the keyed form that Premiere
/// 26.5.1 saves a clip Level (`feature_audio_volume_keys_strict`, param
/// 142) and a nest audio item's (`premiere_isolated_nest_audio_outer_keys_26_5`,
/// param 174): chain 9135, Volume 9159, Mute 9210 and Level 9211, whose
/// native Keyframes `keys` lie on the item's source clock, Inner's. An XML
/// edit of this fixture.
fn with_item_level_keys(xml: &mut String, keys: &str) {
    edit_record(
        xml,
        r#"<AudioClipTrackItem ObjectID="115""#,
        "</AudioClipTrackItem>",
        |record| {
            record.replacen(
                r#"<Components ObjectRef="135"/>"#,
                r#"<Components ObjectRef="9135"/>"#,
                1,
            )
        },
    );
    let layout =
        r#"<AudioChannelLayout>[{"channellabel":100},{"channellabel":101}]</AudioChannelLayout>"#;
    let config = r#"<ChannelConfigData>{"in":[{"layout":[100,101],"name":"Stereo In","type":0}],"out":[{"layout":[100,101],"name":"Stereo Out","type":0}]}</ChannelConfigData>"#;
    let records = format!(
        r#"<AudioComponentChain ObjectID="9135" ClassID="3cb131d1-d3c0-47ae-a19a-bdf75ea11674" Version="4"><ComponentChain Version="3"><Components Version="1"><Component Index="0" ObjectRef="9159"/></Components></ComponentChain>{layout}<ChannelType>1</ChannelType></AudioComponentChain>
<AudioFilterComponent ObjectID="9159" ClassID="d77a90a0-6c9e-44bf-9b20-de8c21168fe1" Version="4"><AudioComponent Version="3"><Component Version="7"><Params Version="1"><Param Index="0" ObjectRef="9210"/><Param Index="1" ObjectRef="9211"/></Params><ID>1</ID><Intrinsic>true</Intrinsic></Component>{layout}<AudioComponentType>0</AudioComponentType><FrameRate>5292000</FrameRate><ChannelType>1</ChannelType></AudioComponent><FilterPreset>0</FilterPreset>{config}<FilterMatchName>Internal Volume Stereo</FilterMatchName><FilterIndex>-1</FilterIndex></AudioFilterComponent>
<AudioComponentParam ObjectID="9210" ClassID="32657501-3aa4-445f-a49b-d09ecb9fa1ae" Version="10"><IsTimeVarying>false</IsTimeVarying><Name>Mute</Name><RangeLocked>false</RangeLocked></AudioComponentParam>
<AudioComponentParam ObjectID="9211" ClassID="a714635e-a628-4b27-9d59-77eba47dbc1a" Version="10"><StartKeyframe>-91445760000000000,0.177827939391,0,0,0,0,0,0</StartKeyframe><CurrentValue>0.17782793939113617</CurrentValue><Keyframes>{keys}</Keyframes><Name>Level</Name><UnitsString>dB</UnitsString></AudioComponentParam>
</PremiereData>"#
    );
    *xml = xml.replacen("</PremiereData>", &records, 1);
}

/// Inner's I4 (clip 161) playing the linked-A/V source from 0.5 s to 4.5 s,
/// so that its source clock runs 0.5 s ahead of Inner's.
fn with_inner_sound_from_half_a_second(xml: &mut String) {
    edit_record(
        xml,
        r#"<AudioClip ObjectID="161""#,
        "</AudioClip>",
        |record| {
            record
                .replacen("<InPoint>0</InPoint>", "<InPoint>127008000000</InPoint>", 1)
                .replacen(
                    "<OutPoint>1016064000000</OutPoint>",
                    "<OutPoint>1143072000000</OutPoint>",
                    1,
                )
        },
    );
}

/// I4's Level 211 with the native Keyframes `keys` on its source clock.
fn with_inner_level_keys(xml: &mut String, keys: &str) {
    let level = "<IsTimeVarying>false</IsTimeVarying>\n\t\t<Name>Level</Name>";
    assert_eq!(xml.matches(level).count(), 1);
    *xml = xml.replacen(
        level,
        &format!("<Keyframes>{keys}</Keyframes>\n\t\t<Name>Level</Name>"),
        1,
    );
}

/// A copy of I4 (item 112) under ObjectID 9112 at default Volume (chain
/// 135), as the one item of Inner's second audio track: a sibling of I4
/// that plays the same source over the same ranges.
fn with_static_inner_sound(xml: &mut String) {
    edit_record(
        xml,
        r#"<AudioClipTrackItem ObjectID="112""#,
        "</AudioClipTrackItem>",
        |record| {
            record.to_owned()
                + &record
                    .replacen(r#"ObjectID="112""#, r#"ObjectID="9112""#, 1)
                    .replacen(
                        r#"<Components ObjectRef="121"/>"#,
                        r#"<Components ObjectRef="135"/>"#,
                        1,
                    )
        },
    );
    edit_record(
        xml,
        r#"<AudioClipTrack ObjectUID="f1f32858-b51c-4131-8fe2-367659b28c9a""#,
        "</AudioClipTrack>",
        |record| {
            record.replacen(
                r#"<ClipItems Version="3">"#,
                r#"<ClipItems Version="3"><TrackItems Version="1"><TrackItem Index="0" ObjectRef="9112"/></TrackItems>"#,
                1,
            )
        },
    );
}

/// Stages `xml`, an edit of the images/nests fixture, in `root` with the
/// fixture's media, and imports its root sequence into `root/imported`.
fn import_images_nests(root: &Path, xml: &str) -> (PathBuf, Vec<Omission>) {
    fs::create_dir_all(root).unwrap();
    let source = root.join(IMAGES_NESTS);
    write_prproj(&source, xml);
    for name in [
        "feature_linked_av_source.mp4",
        "feature_timecoded_source.mp4",
        "in_small.png",
        "in_large.png",
        "in_portrait.png",
    ] {
        fs::copy(fixtures().join(name), root.join(name)).unwrap();
    }
    let imported = root.join("imported");
    let omissions =
        premiere_to_tesseract(&source, &imported, Some(IMAGES_NESTS_SEQUENCE), false).unwrap();
    (imported, omissions)
}

/// N's group, which is visible.
fn inner_group(document: &Value) -> Value {
    let group = layers_of(document, "Group")
        .into_iter()
        .find(|group| group["name"] == "Inner")
        .expect("N imports as a group");
    assert_ne!(group["isHidden"], true);
    group
}

/// Exports `document`, an edit of the project imported into `imported`, with
/// its fixture media to `root/premiere` and imports the export's root
/// sequence into `root/reimported`; neither direction reports anything.
/// Returns the exported XML and the reimported project's directory.
fn round_trip(root: &Path, imported: &Path, document: &Value) -> (String, PathBuf) {
    let file = TesseractFile::open(&project_files(imported)[0]).unwrap();
    let assets: Vec<_> = file
        .metadata()
        .assets
        .iter()
        .map(|(id, descriptor)| {
            let name = Path::new(&descriptor.path).file_name().unwrap();
            (id.clone(), fixtures().join(name), descriptor.kind)
        })
        .collect();
    let edited = root.join("edited.tsrct");
    package(&edited, document, &assets);
    let exported = root.join("premiere");
    let omissions = tesseract_to_premiere(&edited, &exported, false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let project = exported.join("project.prproj");
    let reimported = root.join("reimported");
    let root_sequence = exported_root_sequence(&project);
    let omissions =
        premiere_to_tesseract(&project, &reimported, Some(&root_sequence), false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    (read_xml(&project), reimported)
}

/// Item 115's Level keys on Inner's clock: 0 dB at 0.5 s, Linear to -12 dB
/// at 2 s, held until it falls silent at 2.5 s, held silent until 0 dB at
/// 3 s, then Linear to -6 dB at 5 s, after its Out.
const ITEM_LEVEL_KEYS: &str = "127008000000,0.177827939391,0,0,0,0,0,0;508032000000,0.044668357819,4,0,0,0,0,0;635040000000,0.,4,0,0,0,0,0;762048000000,0.177827939391,0,0,0,0,0,0;1270080000000,0.089125096798,0,0,0,0,0,0;";

/// N of the images/nests fixture whose audio item 115 has
/// [`ITEM_LEVEL_KEYS`] and is trimmed to In 1 s at 11 s, over Inner's I4
/// playing the linked-A/V source from 0.5 s: XML edits. The item plays
/// alone: N imports as a visible group of its picture only, and I4's sound
/// as one root audio layer over 11-14 s from source 1.5 s whose volume keys
/// are the item's on the layer clock, times I4's -6 dB: 0 dB at Inner 0.5 s
/// (before In), Linear to -12 dB at 2 s, held until silence at 2.5 s, held
/// silent until 0 dB at 3 s, and Linear to -6 dB at 5 s (after Out). An edit
/// of the silent key survives export, which writes the sound as a root
/// audio item, and reimport.
#[test]
fn a_nest_audio_items_level_keys_import_editable_on_its_sound_and_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut xml = read_xml(&fixtures().join(IMAGES_NESTS));
    with_item_level_keys(&mut xml, ITEM_LEVEL_KEYS);
    edit_record(
        &mut xml,
        r#"<AudioClipTrackItem ObjectID="115""#,
        "</AudioClipTrackItem>",
        |record| {
            record.replacen(
                "<Start>2540160000000</Start>",
                "<Start>2794176000000</Start>",
                1,
            )
        },
    );
    edit_record(
        &mut xml,
        r#"<AudioClip ObjectID="178""#,
        "</AudioClip>",
        |record| record.replacen("<InPoint>0</InPoint>", "<InPoint>254016000000</InPoint>", 1),
    );
    with_inner_sound_from_half_a_second(&mut xml);
    let (imported, omissions) = import_images_nests(root, &xml);
    assert!(
        !omissions
            .iter()
            .any(|omission| omission.record.ends_with("115")
                || (omission.scope == OmissionScope::Occurrence
                    && omission.record.ends_with("116"))),
        "{omissions:?}"
    );
    let i4 = 10f64.powf(-6.0 / 20.0);
    let db = |db: f64| 10f64.powf(db / 20.0);
    // N's visible picture without sound, and I4's one sound with the keys.
    let picture_and_sound = |imported: &Path| {
        let file = TesseractFile::open(&project_files(imported)[0]).unwrap();
        let document = file.project_json().unwrap();
        let children: Vec<_> = inner_group(&document)["layers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|layer| layer["type"].clone())
            .collect();
        assert_eq!(children, [json!("Video")]);
        let [sound] = layers_of(&document, "Audio").try_into().unwrap();
        assert_eq!(
            sound["source"]["assetId"],
            asset_named(&file, "feature_linked_av_source.mp4")
        );
        assert_eq!(
            (*crate::test_support::layer_range(&sound)),
            json!({"start": 11000, "duration": 3000})
        );
        assert_eq!(
            sound["sourceRange"],
            json!({"start": 1500, "duration": 3000})
        );
        let volume = sound["volume"].as_f64().unwrap();
        assert!((volume - i4).abs() < 1e-7, "{volume}");
        (document, sound)
    };
    let (mut document, sound) = picture_and_sound(&imported);
    let keys = volume_keys(&document, &sound).unwrap();
    let expected = [
        (-500, i4, "linear"),
        (1000, i4 * db(-12.0), "cubicBezier"),
        (1500, 0.0, "hold"),
        (2000, i4, "hold"),
        (4000, i4 * i4, "cubicBezier"),
    ];
    assert_eq!(keys.len(), expected.len(), "{keys:?}");
    for ((time, gain, easing), (expected_time, expected_gain, expected_easing)) in
        keys.iter().zip(expected)
    {
        assert_eq!((*time, easing.as_str()), (expected_time, expected_easing));
        assert!((gain - expected_gain).abs() < 1e-7, "{keys:?}");
    }

    // Edit the silent key to -30 dB, export and reimport.
    let edited_gain = db(-30.0);
    let entry = document["composition"]["dynamics"]["entries"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|entry| entry["target"]["layerId"] == sound["id"])
        .unwrap();
    entry["animator"]["keyframes"][2]["value"]["value"] = json!(edited_gain);
    let (xml, reimported) = round_trip(root, &imported, &document);
    // N's sequence and the root; one audio item, of media; no nest audio item or link.
    for (tag, count) in [
        ("<Sequence ObjectUID=", 2),
        ("<AudioClipTrackItem ", 1),
        ("<AudioMediaSource ", 1),
        ("<Link ObjectID=", 0),
    ] {
        assert_eq!(xml.matches(tag).count(), count, "{tag}");
    }
    let (document_back, sound_back) = picture_and_sound(&reimported);
    // Every edited key comes back; export may add Linear pieces on the FX
    // curve inside a fitted segment.
    let keys_back = volume_keys(&document_back, &sound_back).unwrap();
    let mut edited_keys = keys;
    edited_keys[2].1 = edited_gain;
    for (time, gain, easing) in &edited_keys {
        assert!(
            keys_back
                .iter()
                .any(|(time_back, gain_back, easing_back)| time_back == time
                    && (gain_back - gain).abs() < 1e-9
                    && (easing != "hold" || easing_back == easing)),
            "{time} ms: {keys_back:?}"
        );
    }
}

/// Item 115's Level keys on Inner's clock that only step: 0 dB at 0.5 s,
/// then Holds to -12 dB at 1.5 s, to silence at 2.5 s, to 0 dB at 3 s and to
/// -6 dB at 5 s, after its Out.
const ITEM_STEP_KEYS: &str = "127008000000,0.177827939391,4,0,0,0,0,0;381024000000,0.044668357819,4,0,0,0,0,0;635040000000,0.,4,0,0,0,0,0;762048000000,0.177827939391,4,0,0,0,0,0;1270080000000,0.089125096798,4,0,0,0,0,0;";

/// I4's Level keys on its source clock that only step: -6 dB at 0.25 s,
/// before its In; a Hold to -20 dB at 2 s, where the item's -12 dB step
/// lands once I4 plays from 0.5 s; Linear to the same -20 dB at 2.5 s; and
/// Holds to 0 dB at 3.25 s, to -3 dB at 4.5 s, its Out, and to -40 dB at 6 s.
const INNER_STEP_KEYS: &str = "63504000000,0.089125096798,4,0,0,0,0,0;508032000000,0.017782794312,0,0,0,0,0,0;635040000000,0.017782794312,4,0,0,0,0,0;825552000000,0.177827939391,4,0,0,0,0,0;1143072000000,0.125892541179,4,0,0,0,0,0;1524096000000,0.00177827939391,0,0,0,0,0,0;";

/// N of the images/nests fixture whose audio item 115 and a copy of it, 9115
/// on the root's second audio track, share [`ITEM_STEP_KEYS`], over Inner's
/// I4 playing from 0.5 s with [`INNER_STEP_KEYS`] and a static copy of I4,
/// 9112: XML edits. Each item plays alone, so N imports as a visible group
/// of its picture only, and each inner sound as one root audio layer per
/// item over 10-14 s from source 0.5 s: I4 with Hold keys at every time
/// either Level has a key, valued at their product, 9112 with the item's
/// keys. With 115 disabled, its two sounds import at zero gain and 9115's
/// as before. An edit of one product key survives export and reimport, and
/// the other three key tracks come back unchanged.
#[test]
fn a_nest_audio_items_hold_levels_multiply_into_each_keyed_sound_and_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let edited = |disabled: bool| {
        let mut xml = read_xml(&fixtures().join(IMAGES_NESTS));
        with_inner_sound_from_half_a_second(&mut xml);
        with_inner_level_keys(&mut xml, INNER_STEP_KEYS);
        with_static_inner_sound(&mut xml);
        with_item_level_keys(&mut xml, ITEM_STEP_KEYS);
        edit_record(
            &mut xml,
            r#"<AudioClipTrackItem ObjectID="115""#,
            "</AudioClipTrackItem>",
            |record| {
                let copy = record.replacen(r#"ObjectID="115""#, r#"ObjectID="9115""#, 1);
                let record = if disabled {
                    record.replacen(
                        "</ClipTrackItem>",
                        "<IsMuted>true</IsMuted></ClipTrackItem>",
                        1,
                    )
                } else {
                    record.to_owned()
                };
                record + &copy
            },
        );
        edit_record(
            &mut xml,
            r#"<AudioClipTrack ObjectUID="5b8321fd-17ff-40c1-bfdf-cec174272d14""#,
            "</AudioClipTrack>",
            |track| {
                track.replacen(
                    r#"<ClipItems Version="3">"#,
                    r#"<ClipItems Version="3"><TrackItems Version="1"><TrackItem Index="0" ObjectRef="9115"/></TrackItems>"#,
                    1,
                )
            },
        );
        xml
    };
    let i4 = 10f64.powf(-6.0 / 20.0);
    let db = |db: f64| 10f64.powf(db / 20.0);
    // On the layer clock, the source clock less 0.5 s.
    let product = [
        (-250, db(-6.0), "linear"),
        (500, db(-6.0), "hold"),
        (1500, db(-32.0), "hold"),
        (2000, db(-32.0), "hold"),
        (2500, 0.0, "hold"),
        (2750, 0.0, "hold"),
        (3000, 1.0, "hold"),
        (4000, db(-3.0), "hold"),
        (5000, db(-9.0), "hold"),
        (5500, db(-46.0), "hold"),
    ];
    let item = [
        (500, 1.0, "linear"),
        (1500, db(-12.0), "hold"),
        (2500, 0.0, "hold"),
        (3000, 1.0, "hold"),
        (5000, db(-6.0), "hold"),
    ];
    // N shows only its picture; each root sound's volume and keys.
    let sounds_of = |imported: &Path| {
        let file = TesseractFile::open(&project_files(imported)[0]).unwrap();
        let document = file.project_json().unwrap();
        let children: Vec<_> = inner_group(&document)["layers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|layer| layer["type"].clone())
            .collect();
        assert_eq!(children, [json!("Video")]);
        let sounds: Vec<_> = layers_of(&document, "Audio")
            .iter()
            .map(|sound| {
                assert_eq!(
                    sound["source"]["assetId"],
                    asset_named(&file, "feature_linked_av_source.mp4")
                );
                assert_eq!(
                    (*crate::test_support::layer_range(sound)),
                    json!({"start": 10000, "duration": 4000})
                );
                assert_eq!(
                    sound["sourceRange"],
                    json!({"start": 500, "duration": 4000})
                );
                (
                    sound["volume"].as_f64().unwrap(),
                    volume_keys(&document, sound),
                )
            })
            .collect();
        (document, sounds)
    };
    // I4 and 9112 through 115, then through 9115: once per item each.
    let rows = [
        (false, [(i4, &product[..], 1.0), (1.0, &item[..], 1.0)]),
        (true, [(0.0, &product[..], 0.0), (0.0, &item[..], 0.0)]),
    ];
    let mut imported = Vec::new();
    for (disabled, through_115) in rows {
        let root = dir
            .path()
            .join(if disabled { "disabled" } else { "enabled" });
        let (project, omissions) = import_images_nests(&root, &edited(disabled));
        assert!(
            !omissions
                .iter()
                .any(|omission| omission.record.ends_with("115")
                    || omission.reason.contains("volume")),
            "{omissions:?}"
        );
        let (document, sounds) = sounds_of(&project);
        let expected = through_115
            .into_iter()
            .chain([(i4, &product[..], 1.0), (1.0, &item[..], 1.0)]);
        assert_eq!(sounds.len(), 4, "{sounds:?}");
        for (sound, (volume, keys, gain)) in sounds.into_iter().zip(expected) {
            assert_volume(sound, volume, Some(keys), gain);
        }
        imported.push((root, project, document));
    }

    // Edit the product's silent key at 2500 ms, on 115's I4, to -30 dB.
    let (root, project, mut document) = imported.into_iter().next().unwrap();
    let first = layers_of(&document, "Audio")[0]["id"].clone();
    let entry = document["composition"]["dynamics"]["entries"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|entry| entry["target"]["layerId"] == first)
        .unwrap();
    entry["animator"]["keyframes"][4]["value"]["value"] = json!(db(-30.0));
    let (xml, reimported) = round_trip(&root, &project, &document);
    // Each sound is an audio item of media; no nest audio item or link.
    for (tag, count) in [("<AudioClipTrackItem ", 4), ("<Link ObjectID=", 0)] {
        assert_eq!(xml.matches(tag).count(), count, "{tag}");
    }
    // Overlapping sounds may change order: I4's two, the unedited first,
    // then 9112's.
    let (_, mut sounds) = sounds_of(&reimported);
    let order = |(_, keys): &SoundVolume| {
        let keys = keys.as_ref().unwrap();
        (keys[0].0, keys[4].1)
    };
    sounds.sort_by(|a, b| order(a).partial_cmp(&order(b)).unwrap());
    let mut edited_product = product;
    edited_product[4].1 = db(-30.0);
    let expected = [
        (i4, &product[..]),
        (i4, &edited_product[..]),
        (1.0, &item[..]),
        (1.0, &item[..]),
    ];
    for (sound, (volume, keys)) in sounds.into_iter().zip(expected) {
        assert_volume(sound, volume, Some(keys), 1.0);
    }
}

/// Level keys with a silent interval and two keys 0.4 ms apart, which round
/// to one millisecond of a layer clock that starts at a whole millisecond
/// of their source clock: 0 dB at 0.5 s, a Hold to silence at 1.5 s, a Hold
/// to 0 dB at 2 s, and Linear to -12 dB at 3 s and to the same -12 dB at
/// `last` ticks.
fn keys_with_last(last: i64) -> String {
    format!("127008000000,0.177827939391,4,0,0,0,0,0;381024000000,0.,4,0,0,0,0,0;508032000000,0.177827939391,0,0,0,0,0,0;762048000000,0.044668357819,0,0,0,0,0,0;{last},0.044668357819,0,0,0,0,0,0;")
}

/// A key track that cannot import, because two of its keys round to one
/// millisecond of the sound's layer clock, leaves the sound's layer at zero
/// gain with one report on the sound, where its static level would play
/// through the keys' silence; the layer keeps its placement and source.
/// Each row edits N of the images/nests fixture. Item 115's keys 0.4 ms
/// apart reach I4, which plays from 0.5 s, as a root sound, and 1.4 ms
/// apart they import. I4's own keys 0.4 ms apart leave its sound silent in
/// N's group. Stepping keys of item 115 and of I4 whose product has keys
/// 0.4 ms apart, at I4's source 2 s and 2.0004 s, leave I4 silent while its
/// static copy 9112 keeps the item's keys.
#[test]
fn nested_sounds_whose_level_keys_cannot_import_are_kept_silent() {
    let dir = tempfile::tempdir().unwrap();
    let i4 = 10f64.powf(-6.0 / 20.0);
    let db = |db: f64| 10f64.powf(db / 20.0);
    // 3 s plus 0.4 ms (101606400 ticks) and plus 1.4 ms.
    let (close, apart) = (
        keys_with_last(762_149_606_400),
        keys_with_last(762_403_622_400),
    );
    let edited = |edit: &dyn Fn(&mut String)| {
        let mut xml = read_xml(&fixtures().join(IMAGES_NESTS));
        edit(&mut xml);
        xml
    };
    let imported_keys = [
        (500, 1.0, "linear"),
        (1500, 0.0, "hold"),
        (2000, 1.0, "hold"),
        (3000, db(-12.0), "cubicBezier"),
        (3001, db(-12.0), "linear"),
    ];
    let item_steps = [
        (500, 1.0, "linear"),
        (1500, db(-12.0), "hold"),
        (2500, 0.0, "hold"),
        (3000, 1.0, "hold"),
        (5000, db(-6.0), "hold"),
    ];
    type Sound<'a> = (f64, Option<&'a [(i64, f64, &'a str)]>, f64);
    // (row, XML, N's group carries a sound, each sound's volume and keys
    // with their gain, one report on I4)
    let rows: [(&str, String, bool, Vec<Sound>, bool); 4] = [
        (
            "item keys 0.4 ms apart",
            edited(&|xml| {
                with_inner_sound_from_half_a_second(xml);
                with_item_level_keys(xml, &close);
            }),
            false,
            vec![(0.0, None, 0.0)],
            true,
        ),
        (
            "item keys 1.4 ms apart",
            edited(&|xml| {
                with_inner_sound_from_half_a_second(xml);
                with_item_level_keys(xml, &apart);
            }),
            false,
            vec![(i4, Some(&imported_keys[..]), i4)],
            false,
        ),
        (
            "I4's keys 0.4 ms apart",
            edited(&|xml| with_inner_level_keys(xml, &close)),
            true,
            vec![(0.0, None, 0.0)],
            true,
        ),
        (
            "product keys 0.4 ms apart",
            edited(&|xml| {
                with_inner_sound_from_half_a_second(xml);
                // -6 dB at 0.25 s, then a Hold to -20 dB at 2.0004 s.
                with_inner_level_keys(
                    xml,
                    "63504000000,0.089125096798,4,0,0,0,0,0;508133606400,0.017782794312,4,0,0,0,0,0;",
                );
                with_static_inner_sound(xml);
                with_item_level_keys(xml, ITEM_STEP_KEYS);
            }),
            false,
            vec![(0.0, None, 0.0), (1.0, Some(&item_steps[..]), 1.0)],
            true,
        ),
    ];
    for (name, xml, in_group, expected, reported) in rows {
        let (imported, omissions) = import_images_nests(&dir.path().join(name), &xml);
        let file = TesseractFile::open(&project_files(&imported)[0]).unwrap();
        let document = file.project_json().unwrap();
        let group = inner_group(&document);
        let children = group["layers"].as_array().unwrap();
        // A sound in N's group is on the group's clock.
        let (sounds, start): (Vec<_>, _) = if in_group {
            assert_eq!(children.len(), 2, "{name}");
            let sounds = children
                .iter()
                .filter(|layer| layer["type"] == "Audio")
                .cloned()
                .collect();
            (sounds, 0)
        } else {
            assert_eq!(children.len(), 1, "{name}");
            (layers_of(&document, "Audio"), 10000)
        };
        assert_eq!(sounds.len(), expected.len(), "{name}");
        for (sound, (volume, keys, gain)) in sounds.iter().zip(expected) {
            assert_eq!(
                sound["source"]["assetId"],
                asset_named(&file, "feature_linked_av_source.mp4"),
                "{name}"
            );
            assert_eq!(
                (*crate::test_support::layer_range(sound)),
                json!({"start": start, "duration": 4000}),
                "{name}"
            );
            assert_volume(
                (
                    sound["volume"].as_f64().unwrap(),
                    volume_keys(&document, sound),
                ),
                volume,
                keys,
                gain,
            );
        }
        let reports: Vec<_> = omissions
            .iter()
            .filter(|omission| omission.reason.contains("volume"))
            .collect();
        if !reported {
            assert!(reports.is_empty(), "{name}: {reports:?}");
            continue;
        }
        let [report] = reports.as_slice() else {
            panic!("{name}: {reports:?}");
        };
        assert_eq!(
            (report.scope, report.record.as_str()),
            (OmissionScope::Feature, "AudioClipTrackItem:112"),
            "{name}"
        );
        assert!(
            report.reason.starts_with(
                "volume animation was not imported: unsupported conversion: Premiere volume keyframe times/values cannot be imported: "
            ) && report.reason.ends_with(
                " is not strictly after the preceding key; the sound was kept at zero gain"
            ),
            "{name}: {}",
            report.reason
        );
    }
}

/// A copy of I4 (item 112) under ObjectID 9112 at default Volume (chain
/// 135), as the one item of Inner's second audio track, whose own clip 9161
/// (SubClip 9122) plays the linked-A/V source over the same 4 s from
/// `in_ticks`: a sibling of I4 that an edit of I4's clip does not reach.
fn with_inner_sound_copy(xml: &mut String, in_ticks: i64) {
    let once = |record: &str, from: &str, to: &str| {
        assert_eq!(record.matches(from).count(), 1, "{from}");
        record.replacen(from, to, 1)
    };
    with_static_inner_sound(xml);
    edit_record(
        xml,
        r#"<AudioClipTrackItem ObjectID="9112""#,
        "</AudioClipTrackItem>",
        |record| {
            once(
                record,
                r#"<SubClip ObjectRef="122"/>"#,
                r#"<SubClip ObjectRef="9122"/>"#,
            )
        },
    );
    edit_record(xml, r#"<SubClip ObjectID="122""#, "</SubClip>", |record| {
        let copy = once(record, r#"ObjectID="122""#, r#"ObjectID="9122""#);
        record.to_owned()
            + &once(
                &copy,
                r#"<Clip ObjectRef="161"/>"#,
                r#"<Clip ObjectRef="9161"/>"#,
            )
    });
    edit_record(
        xml,
        r#"<AudioClip ObjectID="161""#,
        "</AudioClip>",
        |record| {
            let copy = once(record, r#"ObjectID="161""#, r#"ObjectID="9161""#);
            let copy = once(
                &copy,
                "<InPoint>0</InPoint>",
                &format!("<InPoint>{in_ticks}</InPoint>"),
            );
            record.to_owned()
                + &once(
                    &copy,
                    "<OutPoint>1016064000000</OutPoint>",
                    &format!("<OutPoint>{}</OutPoint>", in_ticks + 1016064000000),
                )
        },
    );
}

/// Item 115's one Level key, at 0 dB, at a tick that stays within
/// Premiere's tick range on the clock of a sound played from In 0 but not
/// 0.5 s later, on the clock of a sound played from In 0.5 s.
const TICK_RANGE_KEY: &str = "9223372036854775000,0.177827939391,0,0,0,0,0,0;";

/// Hold Level keys past +1000 dB, finite on their own, whose products with
/// each other or with such a gain overflow: item 115's on Inner's clock at
/// 0.5 s and 1.5 s, and a sound's on its source clock at 0.25 s and 2 s.
const HUGE_ITEM_STEPS: &str = "127008000000,1e155,4,0,0,0,0,0;381024000000,2e155,4,0,0,0,0,0;";
const HUGE_SOUND_STEPS: &str = "63504000000,1e155,4,0,0,0,0,0;508032000000,2e155,4,0,0,0,0,0;";

/// A sound that cannot take the Level keys of the nest audio item that
/// plays it alone is omitted on its own, with one report on the item that
/// names it, and its sibling imports as it does without it: once, with the
/// item's keys, and no report. Each row edits N of the images/nests fixture,
/// whose item 115 plays I4 and a copy of it, 9112, that plays after I4 from
/// a clip of its own, and makes I4 or the copy fail: its source clock moves
/// the item's key past Premiere's tick range, its Hold keys' product with
/// the item's overflows, or its gain times one of the item's keys does.
#[test]
fn a_nest_sound_that_cannot_take_its_items_level_keys_is_omitted_alone() {
    let dir = tempfile::tempdir().unwrap();
    let tick_range = "nested sequence audio item exceeds Premiere's tick range";
    let overflow = "nested sequence audio gain overflows";
    let key_gain = "invalid Premiere project: clip Volume keys must be finite, nonnegative gains";
    // The fixture with the copy playing from `copy_in`, then `edit`.
    let edited = |copy_in: i64, edit: &dyn Fn(&mut String)| {
        let mut xml = read_xml(&fixtures().join(IMAGES_NESTS));
        with_inner_sound_copy(&mut xml, copy_in);
        edit(&mut xml);
        xml
    };
    let volume = |xml: &mut String, item: &str, from: &str, to: &str| {
        edit_record(
            xml,
            &format!(r#"<AudioClipTrackItem ObjectID="{item}""#),
            "</AudioClipTrackItem>",
            |record| {
                record.replacen(
                    &format!(r#"<Components ObjectRef="{from}"/>"#),
                    &format!(r#"<Components ObjectRef="{to}"/>"#),
                    1,
                )
            },
        );
    };
    let clip_gain = |xml: &mut String, clip: &str| {
        edit_record(
            xml,
            &format!(r#"<AudioClip ObjectID="{clip}""#),
            "</AudioClip>",
            |record| {
                record.replacen(
                    "</AudioChannelLayout>",
                    "</AudioChannelLayout><Gain>1e155</Gain>",
                    1,
                )
            },
        );
    };
    // (row, XML, the failing sound, its reason)
    let rows: [(&str, String, &str, &str); 6] = [
        (
            "I4 from 0.5 s, the copy from 0",
            edited(0, &|xml| {
                with_inner_sound_from_half_a_second(xml);
                with_item_level_keys(xml, TICK_RANGE_KEY);
            }),
            "112",
            tick_range,
        ),
        (
            "I4 from 0, the copy from 0.5 s",
            edited(127008000000, &|xml| {
                with_item_level_keys(xml, TICK_RANGE_KEY)
            }),
            "9112",
            tick_range,
        ),
        (
            "I4 keyed",
            edited(0, &|xml| {
                with_inner_level_keys(xml, HUGE_SOUND_STEPS);
                with_item_level_keys(xml, HUGE_ITEM_STEPS);
            }),
            "112",
            overflow,
        ),
        (
            "the copy keyed",
            edited(0, &|xml| {
                // I4 at default Volume, the copy with I4's Volume and Level 211.
                volume(xml, "112", "121", "135");
                volume(xml, "9112", "135", "121");
                with_inner_level_keys(xml, HUGE_SOUND_STEPS);
                with_item_level_keys(xml, HUGE_ITEM_STEPS);
            }),
            "9112",
            overflow,
        ),
        (
            "I4 with Clip Gain",
            edited(0, &|xml| {
                clip_gain(xml, "161");
                with_item_level_keys(xml, HUGE_ITEM_STEPS);
            }),
            "112",
            key_gain,
        ),
        (
            "the copy with Clip Gain",
            edited(0, &|xml| {
                clip_gain(xml, "9161");
                with_item_level_keys(xml, HUGE_ITEM_STEPS);
            }),
            "9112",
            key_gain,
        ),
    ];
    for (name, xml, failing, reason) in rows {
        // Each root sound as its ranges, volume and keys, and the reports.
        let heard = |root: &str, xml: &str| {
            let (imported, omissions) = import_images_nests(&dir.path().join(name).join(root), xml);
            let file = TesseractFile::open(&project_files(&imported)[0]).unwrap();
            let document = file.project_json().unwrap();
            // N's group keeps its picture only.
            assert_eq!(
                inner_group(&document)["layers"].as_array().unwrap().len(),
                1,
                "{name}"
            );
            let sounds: Vec<_> = layers_of(&document, "Audio")
                .iter()
                .map(|sound| {
                    (
                        (*crate::test_support::layer_range(sound)).clone(),
                        sound["sourceRange"].clone(),
                        sound["volume"].clone(),
                        volume_keys(&document, sound),
                    )
                })
                .collect();
            (sounds, omissions)
        };
        let (sounds, omissions) = heard("both", &xml);
        let mut alone = xml.clone();
        let item = format!(r#"<TrackItem Index="0" ObjectRef="{failing}"/>"#);
        assert_eq!(alone.matches(&item).count(), 1, "{name}");
        alone = alone.replacen(&item, "", 1);
        let (sibling, sibling_omissions) = heard("sibling alone", &alone);
        let [(_, _, _, keys)] = sibling.as_slice() else {
            panic!("{name}: {sibling:?}");
        };
        assert!(keys.is_some(), "{name}");
        assert_eq!(sounds, sibling, "{name}");
        assert!(
            !sibling_omissions
                .iter()
                .any(|omission| omission.record.ends_with("115")),
            "{name}: {sibling_omissions:?}"
        );
        let reported: Vec<_> = omissions
            .iter()
            .filter(|omission| !sibling_omissions.contains(omission))
            .map(|omission| {
                (
                    omission.scope,
                    omission.record.as_str(),
                    omission.reason.clone(),
                )
            })
            .collect();
        assert_eq!(
            reported,
            [(
                OmissionScope::Occurrence,
                "AudioClipTrackItem:115",
                format!("nested sound AudioClipTrackItem:{failing} not converted: {reason}")
            )],
            "{name}"
        );
    }
}
