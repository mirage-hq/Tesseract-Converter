//! Supplementary admission/boundary controls over unchanged encoder payloads.
//! Original sRGB/ProRes/High10 corpus CLI evidence is private, outside product Git.

mod avc_configuration;

use super::*;
use crate::{ExportLossDomain, ExportLossSource};

fn be32(bytes: &[u8], offset: usize) -> usize {
    u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize
}

fn path(bytes: &[u8], tags: &[&[u8; 4]]) -> Vec<usize> {
    let (mut start, mut end) = (0, bytes.len());
    tags.iter()
        .map(|tag| {
            while &bytes[start + 4..start + 8] != *tag {
                start += be32(bytes, start);
                assert!(start < end);
            }
            let offset = start;
            end = start + be32(bytes, start);
            start += 8;
            offset
        })
        .collect()
}

fn srgb_video() -> Vec<u8> {
    let mut bytes = include_bytes!("../../../../tests/fixtures/video-30fps.mov").to_vec();
    let mut parents = path(
        &bytes,
        &[b"moov", b"trak", b"mdia", b"minf", b"stbl", b"stsd"],
    );
    let entry = parents.last().unwrap() + 16;
    parents.push(entry);
    let end = entry + be32(&bytes, entry);
    assert!(parents[0] > bytes.windows(4).position(|x| x == b"mdat").unwrap());
    // Only insert a container declaration; no encoded frame, clock, range or
    // dimension changes. This is not an independently Adobe-authored oracle.
    let colr = [
        19_u32.to_be_bytes().as_slice(),
        b"colrnclx",
        &[0, 1, 0, 13, 0, 2, 0],
    ]
    .concat();
    bytes.splice(end..end, colr);
    for offset in parents {
        let size = u32::try_from(be32(&bytes, offset) + 19).unwrap();
        bytes[offset..offset + 4].copy_from_slice(&size.to_be_bytes());
    }
    bytes
}

fn prepared_archive(root: &Path, bytes: &[u8]) -> TesseractFile {
    let mut value = crate::test_support::editable_document();
    let mut sibling = value["composition"]["layers"][0].clone();
    sibling["id"] = json!(3);
    sibling["name"] = json!("Native sibling");
    sibling["source"]["assetId"] = json!("supported");
    value["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .insert(1, sibling);
    fs::write(root.join("source.mov"), bytes).unwrap();
    TesseractFileBuilder::from_project_json(&serde_json::to_vec(&value).unwrap())
        .unwrap()
        .add_asset(
            "premiere-video-1",
            root.join("source.mov"),
            AssetKind::Video,
        )
        .unwrap()
        .add_asset(
            "supported",
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/video-30fps.mp4"),
            AssetKind::Video,
        )
        .unwrap()
        .write(root.join("input.tsrct"))
        .unwrap()
}

#[test]
fn export_media_srgb_records_picture_loss_and_keeps_supported_sibling_boundary() {
    let root = tempfile::tempdir().unwrap();
    let file = prepared_archive(root.path(), &srgb_video());
    let operation = Premiere
        .prepare_export(&file, file.project(), &Default::default())
        .unwrap();
    assert!(operation.losses().has_native_content);
    assert!(operation.losses().losses.iter().any(|loss| loss.source
        == ExportLossSource::Layer(1.into())
        && loss.domain == ExportLossDomain::Picture
        && loss.kind == crate::ExportLossKind::Field(crate::ExportField::PictureMedia)
        && loss.omission.reason.contains("sRGB")));
    let recipe = operation.packing_recipe();
    assert!(recipe.is_complete());
    assert!(recipe.boundaries().iter().any(|b| b.layer == 1.into()));
    let stage = operation
        .stage_with_picture_replacements(root.path(), &root.path().join("native-only"), &[])
        .unwrap();
    let project = read_native(&stage.directory().join("project.prproj"));
    assert_eq!(project.media.len(), 1);
    assert_eq!(
        project
            .single_sequence()
            .unwrap()
            .video_occurrences()
            .count(),
        1
    );
    assert_eq!(
        fs::read(stage.directory().join("media/video-30fps.mp4")).unwrap(),
        include_bytes!("../../../../tests/fixtures/video-30fps.mp4")
    );
    assert_eq!(
        file.project_json().unwrap()["composition"]["layers"][0]["source"]["assetId"],
        "premiere-video-1"
    );
}

#[test]
fn export_media_supported_source_has_no_media_picture_loss() {
    let root = tempfile::tempdir().unwrap();
    let file = prepared_archive(
        root.path(),
        include_bytes!("../../../../tests/fixtures/video-30fps.mov"),
    );
    let operation = Premiere
        .prepare_export(&file, file.project(), &Default::default())
        .unwrap();
    assert!(!operation
        .losses()
        .losses
        .iter()
        .any(|loss| loss.domain == ExportLossDomain::Picture));
}

#[test]
fn export_media_malformed_prores_retag_is_fatal_not_an_ae_loss() {
    let root = tempfile::tempdir().unwrap();
    let mut bytes = include_bytes!("../../../../tests/fixtures/video-30fps.mov").to_vec();
    let parents = path(
        &bytes,
        &[b"moov", b"trak", b"mdia", b"minf", b"stbl", b"stsd"],
    );
    let entry = parents.last().unwrap() + 16;
    bytes[entry + 4..entry + 8].copy_from_slice(b"ap4h");
    let file = prepared_archive(root.path(), &bytes);
    assert!(Premiere
        .prepare_export(&file, file.project(), &Default::default())
        .is_err());
}

#[test]
fn export_media_invalid_clock_remains_fatal_before_picture_loss() {
    let root = tempfile::tempdir().unwrap();
    let mut bytes = srgb_video();
    let edit = path(&bytes, &[b"moov", b"trak", b"edts", b"elst"])[3];
    bytes[edit + 24..edit + 28].copy_from_slice(&0_u32.to_be_bytes());
    let file = prepared_archive(root.path(), &bytes);
    assert!(Premiere
        .prepare_export(&file, file.project(), &Default::default())
        .is_err());
}

#[test]
fn export_media_corrupt_unsupported_asset_is_fatal() {
    let root = tempfile::tempdir().unwrap();
    let media = srgb_video();
    let file = prepared_archive(root.path(), &media);
    let input = root.path().join("input.tsrct");
    let mut bytes = fs::read(&input).unwrap();
    let start = bytes
        .windows(media.len())
        .position(|part| part == media)
        .unwrap();
    let mdat = media.windows(4).position(|part| part == b"mdat").unwrap();
    bytes[start + mdat + 40] ^= 1;
    fs::write(input, bytes).unwrap();
    assert!(Premiere
        .prepare_export(&file, file.project(), &Default::default())
        .is_err());
}
