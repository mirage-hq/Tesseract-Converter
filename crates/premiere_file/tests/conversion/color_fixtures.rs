#![cfg(feature = "ffmpeg-library")]

//! Premiere 26.5.2-authored colour fixtures (`color_fixtures.provenance.json`)
//! through a fresh import, and the generated native colour fields compared
//! with the fields Premiere saved.
//!
//! `color_clip_colorspace_override.prproj` overrides a Rec.709 H.264 clip to
//! "Sony S-Log3/S-Gamut3.Cine"; `color_clip_input_lut.prproj` (authored in
//! the Premiere UI) gives a still the Interpret Footage Input LUT
//! `color_warm_tint.cube`; `color_hdr_pq_sequence.prproj` and
//! `color_hdr_hlg_sequence.prproj` are Rec.2100 PQ / HLG sequences with
//! graphics white 203 showing one SDR still under Lumetri Exposure +1;
//! `color_hdr_pq_media.prproj` and `color_hdr_hlg_media.prproj` are PQ / HLG
//! sequences holding one in-house 10-bit HEVC clip of the same transfer
//! (`color_hdr_pq_clip.mp4`, `color_hdr_hlg_clip.mp4`, generated with
//! `color_hdr_clip_chart.py` and the FFmpeg commands in the provenance).
//! The native projects are the oracle for the field grammar;
//! reimports of our own exports are supplementary round trips only.

use super::support::*;
use fx_conv::{ConversionMode, ExportFromTesseract};
use premiere_file::{OmissionScope, Premiere, PremiereExportOptions};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
use tesseract_file::{AssetKind, TesseractFile, TesseractFileBuilder};

const OVERRIDE_PROJECT: &str = "color_clip_colorspace_override.prproj";
const OVERRIDE_SEQUENCE: &str = "ba91dc24-6434-451d-ad76-3a0b18dbbdca";
const OVERRIDE_MEDIA: &str = "feature_two_tracks_gap_clip_a.mp4";
/// The overridden clip's Media record.
const OVERRIDE_MEDIA_RECORD: &str = "Media:b57b949a-1784-441e-861e-f645e355fd24";
const PQ_PROJECT: &str = "color_hdr_pq_sequence.prproj";
const PQ_SEQUENCE: &str = "5f9c5055-361b-417f-bd3b-bd91f2968057";
/// The SDR still both Rec.2100 sequence fixtures show.
const HDR_SEQUENCE_STILL: &str = "a1_bg_chart.png";
const LUT_PROJECT: &str = "color_clip_input_lut.prproj";
const LUT_SEQUENCE: &str = "302bb7b5-d636-4913-b3ce-eb4fb7442310";
/// The registry GUID of the LUT that Premiere saved for the browsed `.cube`.
const LUT_ID: &str = "1e5c67d0-db8e-8948-7a35-6be118c21ae0";
const LUT_MEDIA: &str = "a1_bg_chart.png";
const LUT_FILE: &str = "color_warm_tint.cube";
const PQ_HINT: &str = "hint: sequence output is Rec.2100 PQ; FX documents carry no HDR delivery target, so use `tsrct export --hdr pq` to deliver it";
const HLG_PROJECT: &str = "color_hdr_hlg_sequence.prproj";
const HLG_SEQUENCE: &str = "45238be6-dbfc-46f6-b26f-2e2e0c50d82d";
const PQ_MEDIA_PROJECT: &str = "color_hdr_pq_media.prproj";
const PQ_MEDIA_SEQUENCE: &str = "20d47a25-2573-4214-8782-d3a82f04e208";
const PQ_CLIP: &str = "color_hdr_pq_clip.mp4";
const HLG_HINT: &str = "hint: sequence output is Rec.2100 HLG; FX documents carry no HDR delivery target, so use `tsrct export --hdr hlg` to deliver it";
const HLG_MEDIA_PROJECT: &str = "color_hdr_hlg_media.prproj";
const HLG_MEDIA_SEQUENCE: &str = "a24bd2f3-ae0f-4b0d-b318-f6ac8d5eeb6d";
const HLG_CLIP: &str = "color_hdr_hlg_clip.mp4";

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// The single report that names an Interpret Footage Color Space Override.
fn override_report(omissions: &[premiere_file::Omission]) -> &premiere_file::Omission {
    let reports: Vec<_> = omissions
        .iter()
        .filter(|omission| omission.reason.contains("Color Space Override"))
        .collect();
    let [report] = reports[..] else {
        panic!("{omissions:?}");
    };
    report
}

/// Every layer of `layers` and its descendants whose type is `kind`.
fn layers_of_type<'a>(layers: &'a Value, kind: &str, found: &mut Vec<&'a Value>) {
    for layer in layers.as_array().into_iter().flatten() {
        if layer["type"] == kind {
            found.push(layer);
        }
        layers_of_type(&layer["layers"], kind, found);
    }
}

fn asset_bytes(file: &TesseractFile, asset_id: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(
        &mut file.asset(asset_id).unwrap().open().unwrap(),
        &mut bytes,
    )
    .unwrap();
    bytes
}

/// The text of each child of the `parent` elements whose children include
/// `marker`, keyed by tag; every such element (a nested sequence repeats
/// the sequence colour fields) must carry the same texts.
fn child_texts(xml: &str, parent: &str, marker: &str) -> BTreeMap<String, String> {
    let document = roxmltree::Document::parse(xml).unwrap();
    let mut elements = document
        .descendants()
        .filter(|node| {
            node.has_tag_name(parent) && node.children().any(|child| child.has_tag_name(marker))
        })
        .map(|element| {
            element
                .children()
                .filter(roxmltree::Node::is_element)
                .filter(|child| {
                    !["TrackGroup", "FrameRect", "ComponentOwner"]
                        .contains(&child.tag_name().name())
                })
                .map(|child| {
                    (
                        child.tag_name().name().to_owned(),
                        child.text().unwrap_or_default().to_owned(),
                    )
                })
                .collect::<BTreeMap<_, _>>()
        });
    let first = elements
        .next()
        .unwrap_or_else(|| panic!("no {parent} with {marker}"));
    for other in elements {
        assert_eq!(other, first, "{parent}");
    }
    first
}

/// SHA-256 of each committed Premiere 26.5.2 colour project, as recorded in
/// `color_fixtures.provenance.json`.
const PINNED_PROJECTS: [(&str, &str); 7] = [
    (
        LUT_PROJECT,
        "a0787c1b939dd802ddc776d309111b86148cc2bb24ce4954e762341819250d78",
    ),
    (
        OVERRIDE_PROJECT,
        "0cc85719042c172d013515d5b8108c353328b1cf3ae53597429e5e74d9acc149",
    ),
    (
        "color_lumetri_basic.prproj",
        "8b78978e027e9b1af0df8a5c06009edd7a8b9bce0c4a6948b94a8d48195ff4a1",
    ),
    (
        PQ_PROJECT,
        "62bd85f16971ed2a6aa42642564186d9d4886777a0759c6b66529568d5bbf170",
    ),
    (
        PQ_MEDIA_PROJECT,
        "a5e694e35dd3ffa1a2b3a05e67c28831259cd06dd15c1f217e446c4010218c03",
    ),
    (
        HLG_MEDIA_PROJECT,
        "50d7358282feaebbbdff8243a7fe5ebc3034028883895603f7fb404f981cee63",
    ),
    (
        HLG_PROJECT,
        "7e6647d5ba3e74c697290dc48aac1228f7fb289bede6904e6f912d5096137d63",
    ),
];

#[test]
fn premiere_colour_fixtures_match_their_pinned_sha256() {
    use sha2::{Digest, Sha256};
    let provenance: Value =
        serde_json::from_slice(&fs::read(fixture_path("color_fixtures.provenance.json")).unwrap())
            .unwrap();
    for (name, sha256) in PINNED_PROJECTS {
        assert_eq!(
            format!(
                "{:x}",
                Sha256::digest(fs::read(fixture_path(name)).unwrap())
            ),
            sha256,
            "{name}"
        );
        let recorded: Vec<_> = provenance["cases"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|case| {
                case["project"]["repo_path"]
                    .as_str()
                    .is_some_and(|path| path.ends_with(&format!("/{name}")))
            })
            .map(|case| case["project"]["sha256"].as_str().unwrap())
            .collect();
        assert_eq!(recorded, [sha256], "{name}");
    }
}

#[test]
fn native_clip_color_space_override_imports_with_bytes_and_names_its_camera_encoding() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("converted");
    let omissions = premiere_to_tesseract(
        fixture_path(OVERRIDE_PROJECT),
        &output,
        Some(OVERRIDE_SEQUENCE),
        false,
    )
    .unwrap();
    let report = override_report(&omissions);
    assert_eq!(report.scope, OmissionScope::Feature);
    assert_eq!(report.record, OVERRIDE_MEDIA_RECORD);
    assert!(
        report.reason.starts_with(
            "Interpret Footage Color Space Override \"Sony S-Log3/S-Gamut3.Cine\" not converted"
        ),
        "{report:?}"
    );
    assert!(
        report
            .reason
            .contains("the equal Tesseract camera encoding is camera:sony-slog3-sgamut3-cine:v1"),
        "{report:?}"
    );
    assert!(
        report
            .reason
            .contains("original log bytes and placement retained"),
        "{report:?}"
    );

    let file = TesseractFile::open(first_project(&output)).unwrap();
    let document = file.project_json().unwrap();
    let mut videos = Vec::new();
    layers_of_type(&document["composition"]["layers"], "Video", &mut videos);
    let [video] = videos[..] else {
        panic!("{document}");
    };
    // Legacy FX holds an input colour space only through a LUT.
    assert!(video["source"].get("inputTransform").is_none(), "{video}");
    let asset_id = video["source"]["assetId"].as_str().unwrap();
    assert_eq!(
        asset_bytes(&file, asset_id),
        fs::read(fixture_path(OVERRIDE_MEDIA)).unwrap()
    );
}

/// The RGB rows of `.cube` text, in file order.
fn cube_rows(text: &str) -> Vec<[f32; 3]> {
    text.lines()
        .map(str::trim)
        .filter(|line| line.starts_with(|c: char| c.is_ascii_digit() || c == '-' || c == '.'))
        .map(|line| {
            let values: Vec<f32> = line
                .split_whitespace()
                .map(|v| v.parse().unwrap())
                .collect();
            <[f32; 3]>::try_from(values).unwrap()
        })
        .collect()
}

/// The decoded samples of every `LUTAsset` Blob in `xml`, with its names.
fn lut_assets(xml: &str) -> Vec<(String, String, String, Vec<u8>)> {
    use base64::Engine;
    let document = roxmltree::Document::parse(xml).unwrap();
    document
        .descendants()
        .filter(|node| node.has_tag_name("LUTAsset"))
        .map(|asset| {
            let text = |tag: &str| {
                asset
                    .children()
                    .find(|child| child.has_tag_name(tag))
                    .and_then(|child| child.text())
                    .unwrap_or_default()
                    .to_owned()
            };
            let blob: String = text("Blob").split_whitespace().collect();
            (
                text("Id"),
                text("Displayname"),
                text("Filename"),
                base64::engine::general_purpose::STANDARD
                    .decode(blob)
                    .unwrap(),
            )
        })
        .collect()
}

/// The fixture's one Image layer and the archive holding it.
fn imported_input_lut(dir: &Path) -> (PathBuf, Vec<premiere_file::Omission>, Value) {
    let output = dir.join("converted");
    let omissions = premiere_to_tesseract(
        fixture_path(LUT_PROJECT),
        &output,
        Some(LUT_SEQUENCE),
        false,
    )
    .unwrap();
    let archive = first_project(&output);
    let document = TesseractFile::open(&archive)
        .unwrap()
        .project_json()
        .unwrap();
    let mut images = Vec::new();
    layers_of_type(&document["composition"]["layers"], "Image", &mut images);
    let [image] = images[..] else {
        panic!("{document}");
    };
    let image = image.clone();
    (archive, omissions, image)
}

/// Premiere 26.5.2 saves a clip Interpret Footage Input LUT as a `LUTAsset`
/// that embeds the `.cube` samples; a fresh import makes it the still's
/// `source.inputTransform` with the samples packaged as a `.cube` asset.
#[test]
fn native_clip_input_lut_imports_as_the_source_lut3d_with_its_embedded_samples() {
    let dir = tempfile::tempdir().unwrap();
    let (archive, omissions, image) = imported_input_lut(dir.path());
    let asset_id = format!("premiere-input-lut-{LUT_ID}");
    assert_eq!(
        image["source"]["inputTransform"],
        json!({"semanticVersion": 1, "transform": {
            "type": "lut3d", "assetId": asset_id,
            "inputEncoding": "jerboa:sdr-rec709-display:v1",
            "outputEncoding": "jerboa:sdr-rec709-display:v1",
            "interpolation": "trilinear"
        }}),
        "{image}"
    );
    // No Color Space Override declares the LUT's input: that is reported.
    assert!(
        omissions.iter().any(|omission| omission.reason
            == "Input LUT source encoding is unknown: the LUT is declared to read jerboa:sdr-rec709-display:v1 values"),
        "{omissions:?}"
    );
    // Premiere interpolates the LUT tetrahedrally, FX trilinearly (measured).
    assert!(
        omissions.iter().any(|omission| omission.reason.starts_with(
            "Interpret Footage Input LUT: Premiere interpolates the LUT tetrahedrally"
        )),
        "{omissions:?}"
    );
    // No Color Space Override report.
    assert!(
        !omissions.iter().any(|omission| omission
            .reason
            .contains("Interpret Footage Color Space Override")),
        "{omissions:?}"
    );
    let file = TesseractFile::open(&archive).unwrap();
    let descriptor = &file.metadata().assets[&asset_id];
    assert_eq!(descriptor.kind, AssetKind::Other);
    assert!(
        descriptor.path.ends_with(&format!("/{LUT_FILE}")),
        "{descriptor:?}"
    );
    // The packaged table is the browsed file's, to f32 precision.
    let packaged = String::from_utf8(asset_bytes(&file, &asset_id)).unwrap();
    assert!(packaged.contains("LUT_3D_SIZE 17\n"), "{packaged}");
    let browsed = cube_rows(&fs::read_to_string(fixture_path(LUT_FILE)).unwrap());
    let rows = cube_rows(&packaged);
    assert_eq!(rows.len(), 17 * 17 * 17);
    assert_eq!(rows.len(), browsed.len());
    for (ours, theirs) in rows.iter().zip(&browsed) {
        for channel in 0..3 {
            assert!(
                (ours[channel] - theirs[channel]).abs() < 1e-6,
                "{ours:?} {theirs:?}"
            );
        }
    }
    assert_eq!(
        asset_bytes(&file, image["source"]["assetId"].as_str().unwrap()),
        fs::read(fixture_path(LUT_MEDIA)).unwrap()
    );
}

/// The imported Input LUT exports in the native grammar: the media's
/// `VideoStream` names a `LUTAsset` whose embedded samples equal the ones
/// Premiere saved, and the `.cube` is written beside the project.
#[test]
fn source_lut3d_exports_the_native_clip_input_lut_grammar() {
    let dir = tempfile::tempdir().unwrap();
    let (archive, _, _) = imported_input_lut(dir.path());
    let package = dir.path().join("native");
    let omissions = tesseract_to_premiere(&archive, &package, false).unwrap();
    assert!(
        !omissions
            .iter()
            .any(|omission| omission.reason.contains("input transform")),
        "{omissions:?}"
    );
    assert!(
        omissions.iter().any(|omission| omission.kind == premiere_file::OmissionKind::Approximated
            && omission.reason.starts_with(
                "source.inputTransform written as the clip Interpret Footage Input LUT; Premiere interpolates the LUT tetrahedrally"
            )),
        "{omissions:?}"
    );
    let ours_xml = read_xml(&package.join("project.prproj"));
    let native_xml = read_xml(&fixture_path(LUT_PROJECT));
    let ours = lut_assets(&ours_xml);
    let native = lut_assets(&native_xml);
    let ([ours], [native]) = (&ours[..], &native[..]) else {
        panic!("one LUTAsset each");
    };
    assert_eq!((&ours.1, &ours.2), (&native.1, &native.2));
    assert_eq!(ours.3, native.3, "embedded ITX3 samples");
    let stream = child_texts(&ours_xml, "VideoStream", "InputLUTSpecified");
    let saved = child_texts(&native_xml, "VideoStream", "InputLUTSpecified");
    for field in [
        "InputLUTSpecified",
        "InputLUTDisplayName",
        "InputLUTFileName",
    ] {
        assert_eq!(stream.get(field), saved.get(field), "{field}");
    }
    assert_eq!(stream["InputLUTID"], ours.0);
    assert!(stream.contains_key("InputLUTAsset"));
    let beside = fs::read_to_string(package.join(LUT_FILE)).unwrap();
    assert_eq!(cube_rows(&beside).len(), 17 * 17 * 17);

    // Supplementary: our own project reimports with the same Input Transform.
    let reimported = dir.path().join("reimported");
    premiere_to_tesseract(package.join("project.prproj"), &reimported, None, false).unwrap();
    let document = TesseractFile::open(first_project(&reimported))
        .unwrap()
        .project_json()
        .unwrap();
    let mut images = Vec::new();
    layers_of_type(&document["composition"]["layers"], "Image", &mut images);
    assert_eq!(
        images[0]["source"]["inputTransform"]["transform"]["type"], "lut3d",
        "{document}"
    );
}

#[test]
fn camera_lut_input_transform_exports_the_native_override_and_input_lut_grammar() {
    let dir = tempfile::tempdir().unwrap();
    let mut document = document(dir.path());
    document["composition"]["layers"][0]["source"]["inputTransform"] = json!({
        "semanticVersion": 1,
        "transform": {
            "type": "lut3d", "assetId": "slog-to-709",
            "inputEncoding": "camera:sony-slog3-sgamut3-cine:v1",
            "outputEncoding": "jerboa:sdr-rec709-display:v1",
            "interpolation": "trilinear"
        }
    });
    let archive = dir.path().join("override.tsrct");
    TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap())
        .unwrap()
        .add_asset(
            "premiere-video-1",
            dir.path().join("source.mp4"),
            AssetKind::Video,
        )
        .unwrap()
        .add_asset("slog-to-709", fixture_path(LUT_FILE), AssetKind::Other)
        .unwrap()
        .write(&archive)
        .unwrap();
    let package = dir.path().join("native");
    let omissions = tesseract_to_premiere(&archive, &package, false).unwrap();
    assert!(
        !omissions
            .iter()
            .any(|omission| omission.reason.starts_with("input transform")),
        "{omissions:?}"
    );
    let packaged: Vec<_> = walk(&package)
        .into_iter()
        .filter(|path| path.extension().is_some_and(|ext| ext == "cube"))
        .collect();
    assert_eq!(packaged, [package.join(LUT_FILE)]);

    // The generated stream carries the override fields Premiere saved, and
    // the clip Input LUT fields of `color_clip_input_lut.prproj`.
    let ours_xml = read_xml(&package.join("project.prproj"));
    let ours = child_texts(&ours_xml, "VideoStream", "OverriddenColorSpace");
    let native = child_texts(
        &read_xml(&fixture_path(OVERRIDE_PROJECT)),
        "VideoStream",
        "OverriddenColorSpace",
    );
    for field in [
        "OverriddenColorSpace",
        "IsColorSpacOverridden",
        "OriginalColorSpace",
    ] {
        assert_eq!(ours.get(field), native.get(field), "{field}");
    }
    assert_eq!(
        native["OverriddenColorSpace"],
        r#"{"baseColorProfile":{"colorProfileName":"Sony S-Log3/S-Gamut3.Cine"},"baseProfileType":1}"#
    );
    assert_eq!(ours["InputLUTSpecified"], "true");
    assert_eq!(ours["InputLUTFileName"], LUT_FILE);
    let native_lut = lut_assets(&read_xml(&fixture_path(LUT_PROJECT)));
    assert_eq!(lut_assets(&ours_xml)[0].3, native_lut[0].3);

    // Supplementary: our own project reimports the LUT, reading the camera
    // encoding that the override names.
    let reimported = dir.path().join("reimported");
    let omissions =
        premiere_to_tesseract(package.join("project.prproj"), &reimported, None, false).unwrap();
    assert!(
        !omissions
            .iter()
            .any(|omission| omission.reason.contains("Color Space Override")),
        "{omissions:?}"
    );
    let rebuilt = TesseractFile::open(first_project(&reimported))
        .unwrap()
        .project_json()
        .unwrap();
    let mut videos = Vec::new();
    layers_of_type(&rebuilt["composition"]["layers"], "Video", &mut videos);
    assert_eq!(
        videos[0]["source"]["inputTransform"]["transform"]["inputEncoding"],
        "camera:sony-slog3-sgamut3-cine:v1"
    );
}

fn walk(root: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            paths.extend(walk(&path));
        } else {
            paths.push(path);
        }
    }
    paths
}

/// A natively authored Rec.2100 sequence (graphics white 203, one SDR still
/// under Lumetri Exposure +1): fresh import admits the white and names the
/// `--hdr` delivery, and the `--hdr` export writes every saved sequence and
/// project colour field with the native value.
fn assert_native_hdr_sequence(
    project: &str,
    sequence: &str,
    transfer: fx_conv::HdrTransfer,
    hint: &str,
    output_profile: &str,
) {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("converted");
    let omissions =
        premiere_to_tesseract(fixture_path(project), &output, Some(sequence), false).unwrap();
    assert!(
        omissions.iter().any(|omission| omission.reason == hint),
        "{omissions:?}"
    );
    // The admitted target binds the sequence colour management; Premiere's
    // default tone-map desaturation (0.5) is not reported.
    assert!(
        !omissions.iter().any(|omission| omission
            .reason
            .contains("HDR sequence target not admitted")
            || omission.reason.contains("ColorManagementSettings")),
        "{omissions:?}"
    );
    assert!(
        !omissions
            .iter()
            .any(|omission| omission.reason.contains("ToneMappingDesaturation")),
        "{omissions:?}"
    );
    let archive = first_project(&output);
    let file = TesseractFile::open(&archive).unwrap();
    let document = file.project_json().unwrap();
    assert_eq!(document["hdrGraphicsWhiteNits"], json!(203.0));
    let mut images = Vec::new();
    layers_of_type(&document["composition"]["layers"], "Image", &mut images);
    let [image] = images[..] else {
        panic!("{document}");
    };
    assert_eq!(
        asset_bytes(&file, image["source"]["assetId"].as_str().unwrap()),
        fs::read(fixture_path(HDR_SEQUENCE_STILL)).unwrap()
    );

    let package = dir.path().join("native");
    Premiere
        .export_from_tesseract(
            &archive,
            &package,
            &PremiereExportOptions {
                hdr: Some(transfer),
                ..Default::default()
            },
            ConversionMode::Write,
        )
        .unwrap();
    let ours_xml = read_xml(&package.join("project.prproj"));
    let native_xml = read_xml(&fixture_path(project));
    let ours = child_texts(&ours_xml, "VideoTrackGroup", "WorkingColorSpace");
    let native = child_texts(&native_xml, "VideoTrackGroup", "WorkingColorSpace");
    for (field, value) in &native {
        assert_eq!(ours.get(field), Some(value), "{field}");
    }
    // Generated-only declarations, kept from the earlier Adobe-opened
    // generated projects; the native PQ sequence saves Premiere's defaults
    // by omitting them.
    let extra: Vec<_> = ours
        .keys()
        .filter(|field| !native.contains_key(*field))
        .map(String::as_str)
        .collect();
    assert_eq!(
        extra,
        [
            "GamutCompressionAlgorithm",
            "ImmersiveVideoVRConfiguration",
            "OutputColorSpace",
            "ToneMappingAlgorithm"
        ]
    );
    assert!(ours["OutputColorSpace"].contains(output_profile));
    let project_settings = |xml: &str| {
        child_texts(xml, "ProjectSettings", "ColorManagementSettings")["ColorManagementSettings"]
            .clone()
    };
    assert_eq!(project_settings(&ours_xml), project_settings(&native_xml));

    // Supplementary: our own project reimports with the same white and hint.
    let reimported = dir.path().join("reimported");
    let omissions =
        premiere_to_tesseract(package.join("project.prproj"), &reimported, None, false).unwrap();
    assert!(
        omissions.iter().any(|omission| omission.reason == hint),
        "{omissions:?}"
    );
    let rebuilt = TesseractFile::open(first_project(&reimported))
        .unwrap()
        .project_json()
        .unwrap();
    assert_eq!(rebuilt["hdrGraphicsWhiteNits"], json!(203.0));
}

#[test]
fn native_pq_sequence_imports_its_graphics_white_and_exports_the_native_sequence_colour_fields() {
    assert_native_hdr_sequence(
        PQ_PROJECT,
        PQ_SEQUENCE,
        fx_conv::HdrTransfer::Pq,
        PQ_HINT,
        "BT.2100 PQ,10-bit,Display-Referred",
    );
}

/// Also proves the reader admits a natively saved HLG sequence without
/// `OutputColorSpace` / `ToneMappingAlgorithm` / `GamutCompressionAlgorithm`.
#[test]
fn native_hlg_sequence_imports_its_graphics_white_and_exports_the_native_sequence_colour_fields() {
    assert_native_hdr_sequence(
        HLG_PROJECT,
        HLG_SEQUENCE,
        fx_conv::HdrTransfer::Hlg,
        HLG_HINT,
        "BT.2100 HLG,10-bit,Display-Referred",
    );
}

/// A natively authored Rec.2100 sequence holding one in-house HEVC Main 10
/// clip of `transfer`: fresh import admits the sequence white and keeps the
/// clip's bytes, and the `--hdr` export packages the same bytes under the
/// source profile Premiere saved.
fn assert_native_hdr_media_sequence(
    project: &str,
    sequence: &str,
    clip: &str,
    transfer: fx_conv::HdrTransfer,
    hint: &str,
    colour: &str,
) {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("converted");
    let omissions =
        premiere_to_tesseract(fixture_path(project), &output, Some(sequence), false).unwrap();
    assert!(
        omissions.iter().any(|omission| omission.reason == hint),
        "{omissions:?}"
    );
    let passthrough =
        format!("video colour {colour} passes through unchanged; display depends on the player");
    assert!(
        omissions
            .iter()
            .any(|omission| omission.reason == passthrough),
        "{omissions:?}"
    );
    // Premiere's saved tone-mapping defaults are not reported.
    assert!(
        !omissions
            .iter()
            .any(|omission| omission.reason.contains("not admitted")
                || omission.reason.contains("ToneMapping")
                || omission.reason.contains("ColorManagementSettings")),
        "{omissions:?}"
    );
    let archive = first_project(&output);
    let file = TesseractFile::open(&archive).unwrap();
    let document = file.project_json().unwrap();
    assert_eq!(document["hdrGraphicsWhiteNits"], json!(203.0));
    let mut videos = Vec::new();
    layers_of_type(&document["composition"]["layers"], "Video", &mut videos);
    let [video] = videos[..] else {
        panic!("{document}");
    };
    let clip_bytes = fs::read(fixture_path(clip)).unwrap();
    assert_eq!(
        asset_bytes(&file, video["source"]["assetId"].as_str().unwrap()),
        clip_bytes
    );

    let package = dir.path().join("native");
    Premiere
        .export_from_tesseract(
            &archive,
            &package,
            &PremiereExportOptions {
                hdr: Some(transfer),
                ..Default::default()
            },
            ConversionMode::Write,
        )
        .unwrap();
    let packaged: Vec<_> = walk(&package)
        .into_iter()
        .filter(|path| path.extension().is_some_and(|ext| ext == "mp4"))
        .collect();
    let [packaged] = &packaged[..] else {
        panic!("{packaged:?}");
    };
    assert_eq!(fs::read(packaged).unwrap(), clip_bytes);
    let ours_xml = read_xml(&package.join("project.prproj"));
    let native_xml = read_xml(&fixture_path(project));
    let profile = |xml: &str| {
        child_texts(xml, "VideoStream", "OriginalColorSpace")["OriginalColorSpace"].clone()
    };
    assert_eq!(profile(&ours_xml), profile(&native_xml));
    let working = |xml: &str| {
        child_texts(xml, "VideoTrackGroup", "WorkingColorSpace")["WorkingColorSpace"].clone()
    };
    assert_eq!(working(&ours_xml), working(&native_xml));
}

#[test]
fn native_pq_media_sequence_imports_the_hevc_pq_clip_and_exports_its_bytes_unchanged() {
    assert_native_hdr_media_sequence(
        PQ_MEDIA_PROJECT,
        PQ_MEDIA_SEQUENCE,
        PQ_CLIP,
        fx_conv::HdrTransfer::Pq,
        PQ_HINT,
        "BT.2020/PQ/BT.2020nc",
    );
}

#[test]
fn native_hlg_media_sequence_imports_the_hevc_hlg_clip_and_exports_its_bytes_unchanged() {
    assert_native_hdr_media_sequence(
        HLG_MEDIA_PROJECT,
        HLG_MEDIA_SEQUENCE,
        HLG_CLIP,
        fx_conv::HdrTransfer::Hlg,
        HLG_HINT,
        "BT.2020/HLG/BT.2020nc",
    );
}
