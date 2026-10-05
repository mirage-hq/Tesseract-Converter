//! PR #4442 package/publication cases.
//!
//! The tiny EXR/WAVE/QuickTime byte strings below are synthetic metadata
//! fixtures. They supplement, and never replace, an independently authored AEP
//! source or an Adobe render/open oracle.

use std::path::{Path, PathBuf};

use fx_conv::ExportFromTesseract;
use fx_schema::{AssetId, EditableFxCompositionDocument, LayerId};
use tesseract_file::{AssetKind, TesseractFile, TesseractFileBuilder};

use super::super::package;
use super::*;
use crate::{
    export_document::media::MediaRequest,
    structure::{ItemKind, read_project},
    writer::footage::{FootageKind, NativeFrameRate, NativeSourceFormat, RelativeMediaPath},
};

#[cfg(feature = "ffmpeg-library")]
#[test]
fn native_mp4_media_stages_without_rewriting_its_bytes() {
    // This is the unchanged footage referenced by independently Adobe-authored
    // import_audio_media_controls.aep; native opening of our export is separate proof.
    let root = tempfile::tempdir().unwrap();
    let media = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../premiere_file/tests/fixtures/feature_rate_24_blue.mp4");
    let archive_path = archive_with_assets(
        root.path(),
        Vec::new(),
        &[("native-mp4", &media, AssetKind::Video)],
    );
    let archive = TesseractFile::open(archive_path).unwrap();
    let staging = root.path().join("staging");
    fs::create_dir(&staging).unwrap();
    let prepared = package::prepare_media(
        &archive,
        &[request("native-mp4", FootageKind::Video)],
        &staging,
        archive.project().dimensions(),
        crate::AepPreparationControl::default(),
    )
    .unwrap();
    assert!(
        prepared.unsupported.is_empty(),
        "{:?}",
        prepared.unsupported
    );
    let source = &prepared.sources["native-mp4"];
    assert_eq!(source.format, NativeSourceFormat::QuickTime);
    assert!(source.path.as_str().ends_with(".mp4"));
    assert_eq!(source.frame_rate, NativeFrameRate::integer(24));
    assert_eq!(
        fs::read(staging.join(source.path.as_str())).unwrap(),
        fs::read(media).unwrap()
    );
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn native_mp4_check_write_keeps_editable_video_and_silent_unity_gain() {
    let root = tempfile::tempdir().unwrap();
    let media = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../premiere_file/tests/fixtures/feature_rate_24_blue.mp4");
    let input = archive_with_assets(
        root.path(),
        vec![serde_json::json!({
            "type":"Video", "id":901, "name":"Editable MP4", "parent":null,
            "playback":{"type":"windowed", "inputRange":{"start":0,"duration":1000},
                "mapping":{"type":"linear", "input":{"start":0,"duration":1000},
                    "output":{"start":0,"duration":1000}}, "inputOffsetMs":0},
            "sourceRange":{"start":0,"duration":1000}, "sourceIntrinsicDuration":5000,
            "transform":transform(), "volume":1, "source":{"assetId":"native-mp4","fit":"contain"}
        })],
        &[("native-mp4", &media, AssetKind::Video)],
    );
    let check_output = root.path().join("check-only");
    let checked = AfterEffects
        .export_from_tesseract(
            &input,
            &check_output,
            &Default::default(),
            ConversionMode::Check,
        )
        .unwrap();
    assert!(!check_output.exists());
    let output = root.path().join("published");
    let written = AfterEffects
        .export_from_tesseract(&input, &output, &Default::default(), ConversionMode::Write)
        .unwrap();
    assert_eq!(checked.diagnostics, written.diagnostics);
    assert!(
        !written
            .diagnostics
            .iter()
            .any(|entry| entry.layer_id.is_some() && entry.message.contains("omitted")),
        "{:?}",
        written.diagnostics
    );
    let asset = written
        .artifacts
        .iter()
        .find(|entry| entry.kind == fx_conv::ArtifactKind::Media)
        .unwrap();
    assert_eq!(asset.path.extension(), Some(std::ffi::OsStr::new("mp4")));
    assert_eq!(
        fs::read(output.join(&asset.path)).unwrap(),
        fs::read(&media).unwrap()
    );
    let native = read_project(&fs::read(output.join("project.aep")).unwrap()).unwrap();
    let comp = native
        .items
        .iter()
        .find_map(|item| match &item.kind {
            ItemKind::Composition(comp) => Some(comp.as_ref()),
            _ => None,
        })
        .unwrap();
    assert_eq!(comp.layers.len(), 1);
    assert_eq!(comp.layers[0].name.as_ref(), "Editable MP4");
    assert!(
        native
            .items
            .iter()
            .any(|item| item.id == comp.layers[0].record.source_id() && item.media.is_some())
    );
}

#[test]
fn native_png_media_stages_bytes_and_colour_metadata_without_exr_transfer() {
    for rgba in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let media = root.path().join("tagged.png");
        if rgba {
            image::RgbaImage::from_pixel(3, 2, image::Rgba([35, 19, 43, 128]))
                .save(&media)
                .unwrap();
        } else {
            image::RgbImage::from_pixel(3, 2, image::Rgb([35, 19, 43]))
                .save(&media)
                .unwrap();
        }
        // A valid gAMA chunk makes metadata retention observable, not merely
        // decoded-pixel equality after re-encoding an untagged image.
        let mut bytes = fs::read(&media).unwrap();
        let mut chunk = 4_u32.to_be_bytes().to_vec();
        chunk.extend_from_slice(b"gAMA");
        chunk.extend_from_slice(&45_455_u32.to_be_bytes());
        let mut crc = !0_u32;
        for byte in &chunk[4..] {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = (crc >> 1) ^ (0xedb88320_u32 & 0_u32.wrapping_sub(crc & 1));
            }
        }
        chunk.extend_from_slice(&(!crc).to_be_bytes());
        bytes.splice(33..33, chunk);
        fs::write(&media, &bytes).unwrap();
        let archive_path = archive_with_assets(
            root.path(),
            Vec::new(),
            &[("native-png", &media, AssetKind::Image)],
        );
        let archive = TesseractFile::open(archive_path).unwrap();
        let staging = root.path().join("staging");
        fs::create_dir(&staging).unwrap();
        let prepared = package::prepare_media(
            &archive,
            &[request("native-png", FootageKind::Image)],
            &staging,
            archive.project().dimensions(),
            crate::AepPreparationControl::default(),
        )
        .unwrap();
        assert!(prepared.unsupported.is_empty());
        let source = &prepared.sources["native-png"];
        assert!(source.path.as_str().ends_with(".png"), "{:?}", source.path);
        assert_eq!(source.dimensions, [3, 2]);
        assert_eq!(source.duration_millis, 0);
        assert!(source.frame_rate.is_zero());
        assert_eq!(fs::read(staging.join(source.path.as_str())).unwrap(), bytes);
        assert!(
            !prepared
                .approximations
                .iter()
                .any(|(_, message)| message.contains("decoded to OpenEXR"))
        );
    }
}

#[test]
fn native_png_check_write_keeps_editable_image_and_straight_alpha_bytes() {
    let root = tempfile::tempdir().unwrap();
    let media = root.path().join("editable.png");
    image::RgbaImage::from_pixel(3, 2, image::Rgba([35, 19, 43, 128]))
        .save(&media)
        .unwrap();
    let input = archive_with_assets(
        root.path(),
        vec![serde_json::json!({
            "type":"Image", "id":901, "name":"Editable PNG", "parent":null,
            "activeRange":{"start":0,"duration":1000}, "transform":transform(),
            "source":{"assetId":"native-png","fit":"contain"}
        })],
        &[("native-png", &media, AssetKind::Image)],
    );
    let check_output = root.path().join("check");
    let checked = AfterEffects
        .export_from_tesseract(
            &input,
            &check_output,
            &Default::default(),
            ConversionMode::Check,
        )
        .unwrap();
    assert!(!check_output.exists());
    let output = root.path().join("published");
    let written = AfterEffects
        .export_from_tesseract(&input, &output, &Default::default(), ConversionMode::Write)
        .unwrap();
    assert_eq!(checked.diagnostics, written.diagnostics);
    assert!(
        !written
            .diagnostics
            .iter()
            .any(|entry| entry.layer_id == Some(LayerId::new(901))
                && entry.message.contains("omitted")),
        "{:?}",
        written.diagnostics
    );
    let artifact = written
        .artifacts
        .iter()
        .find(|entry| entry.kind == fx_conv::ArtifactKind::Media)
        .unwrap();
    assert_eq!(artifact.path.extension(), Some(std::ffi::OsStr::new("png")));
    assert_eq!(
        fs::read(output.join(&artifact.path)).unwrap(),
        fs::read(&media).unwrap()
    );
    let native = read_project(&fs::read(output.join("project.aep")).unwrap()).unwrap();
    let ItemKind::Composition(comp) = &native.item(1).unwrap().kind else {
        panic!("root composition")
    };
    assert_eq!(comp.layers.len(), 1);
    assert_eq!(comp.layers[0].name.as_ref(), "Editable PNG");
    let source = native.item(comp.layers[0].record.source_id()).unwrap();
    let media = source.media.as_ref().unwrap().as_ref().unwrap();
    assert_eq!(media.source_format, *b"png!");
}

#[test]
fn native_png_hint_does_not_change_mislabeled_jpeg_preparation() {
    let root = tempfile::tempdir().unwrap();
    let media = root.path().join("jpeg.png");
    image::RgbImage::from_pixel(3, 2, image::Rgb([35, 19, 43]))
        .save_with_format(&media, image::ImageFormat::Jpeg)
        .unwrap();
    let archive_path = archive_with_assets(
        root.path(),
        Vec::new(),
        &[("native-png-hint", &media, AssetKind::Image)],
    );
    let archive = TesseractFile::open(archive_path).unwrap();
    let staging = root.path().join("staging");
    fs::create_dir(&staging).unwrap();
    let prepared = package::prepare_media(
        &archive,
        &[request("native-png-hint", FootageKind::Image)],
        &staging,
        archive.project().dimensions(),
        crate::AepPreparationControl::default(),
    )
    .unwrap();
    assert!(prepared.unsupported.is_empty());
    let source = &prepared.sources["native-png-hint"];
    assert_eq!(source.format, NativeSourceFormat::OpenExr);
    assert_eq!(source.dimensions, [3, 2]);
    assert!(source.path.as_str().ends_with(".exr"));
    assert!(
        prepared
            .approximations
            .iter()
            .any(|(_, message)| message.contains("decoded to OpenEXR"))
    );
}

fn transform() -> serde_json::Value {
    serde_json::json!({
        "anchorPoint":[0.0,0.0], "position":[0.0,0.0], "scale":[100.0,100.0],
        "rotation":0.0, "opacity":100.0
    })
}

fn archive_with_assets(
    root: &Path,
    layers: Vec<serde_json::Value>,
    assets: &[(&str, &Path, AssetKind)],
) -> PathBuf {
    let imported = import_solid(root);
    let source = TesseractFile::open(imported).unwrap();
    let mut value = source.project_json().unwrap();
    value["composition"]["layers"] = layers.into();
    value["composition"]["dynamics"] = serde_json::json!({"entries":[]});
    archive_value_with_assets(root, value, assets)
}

fn archive_value_with_assets(
    root: &Path,
    value: serde_json::Value,
    assets: &[(&str, &Path, AssetKind)],
) -> PathBuf {
    let document = EditableFxCompositionDocument::from_json_value(value).unwrap();
    let mut builder = TesseractFileBuilder::try_new(document).unwrap();
    for (id, path, kind) in assets {
        builder = builder.add_asset(*id, path, *kind).unwrap();
    }
    let path = root.join("media-input.tsrct");
    drop(builder.write(&path).unwrap());
    path
}

fn request(id: &str, kind: FootageKind) -> MediaRequest {
    MediaRequest {
        layer_id: LayerId::new(match kind {
            FootageKind::Image => 10,
            FootageKind::Video => 11,
            FootageKind::Audio => 12,
        }),
        asset_id: AssetId::new(id).unwrap(),
        kind,
        preferred_name: id.to_owned(),
    }
}

fn exr(width: i32, height: i32) -> Vec<u8> {
    let mut bytes = 20_000_630_u32.to_le_bytes().to_vec();
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    bytes.extend_from_slice(b"dataWindow\0box2i\0");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&0_i32.to_le_bytes());
    bytes.extend_from_slice(&0_i32.to_le_bytes());
    bytes.extend_from_slice(&(width - 1).to_le_bytes());
    bytes.extend_from_slice(&(height - 1).to_le_bytes());
    bytes.push(0);
    bytes
}

fn wave() -> Vec<u8> {
    b"RIFF\x26\0\0\0WAVEfmt \x10\0\0\0\x01\0\x01\0\x40\x1f\0\0\x80\x3e\0\0\x02\0\x10\0data\x02\0\0\0\0\0".to_vec()
}

fn extensible_pcm_wave(subtype_pcm: bool) -> Vec<u8> {
    let mut format = vec![0_u8; 40];
    format[0..2].copy_from_slice(&0xfffe_u16.to_le_bytes());
    format[2..4].copy_from_slice(&1_u16.to_le_bytes());
    format[4..8].copy_from_slice(&48_000_u32.to_le_bytes());
    format[8..12].copy_from_slice(&144_000_u32.to_le_bytes());
    format[12..14].copy_from_slice(&3_u16.to_le_bytes());
    format[14..16].copy_from_slice(&24_u16.to_le_bytes());
    format[16..18].copy_from_slice(&22_u16.to_le_bytes());
    format[18..20].copy_from_slice(&24_u16.to_le_bytes());
    format[20..24].copy_from_slice(&4_u32.to_le_bytes());
    format[24..40].copy_from_slice(&[
        if subtype_pcm { 1 } else { 3 },
        0,
        0,
        0,
        0,
        0,
        16,
        0,
        128,
        0,
        0,
        170,
        0,
        56,
        155,
        113,
    ]);
    let samples = [0x11_u8, 0x22, 0x33];
    let mut bytes = b"RIFF\0\0\0\0WAVE".to_vec();
    bytes.extend_from_slice(b"fmt ");
    bytes.extend_from_slice(&40_u32.to_le_bytes());
    bytes.extend_from_slice(&format);
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(&samples);
    bytes.push(0); // RIFF chunk padding is not a sample.
    let riff_length = u32::try_from(bytes.len() - 8).unwrap();
    bytes[4..8].copy_from_slice(&riff_length.to_le_bytes());
    bytes
}

fn atom(kind: [u8; 4], payload: &[u8]) -> Vec<u8> {
    let size = u32::try_from(payload.len() + 8).unwrap();
    [size.to_be_bytes().as_slice(), &kind, payload].concat()
}

fn quicktime() -> Vec<u8> {
    let ftyp = atom(*b"ftyp", &[b"qt  ".as_slice(), &[0, 0, 0, 0]].concat());
    let mut mvhd = vec![0_u8; 20];
    mvhd[12..16].copy_from_slice(&1_000_u32.to_be_bytes());
    mvhd[16..20].copy_from_slice(&2_000_u32.to_be_bytes());
    let mut tkhd = vec![0_u8; 84];
    tkhd[3] = 3;
    tkhd[20..24].copy_from_slice(&2_000_u32.to_be_bytes());
    for (index, value) in [0x0001_0000_u32, 0, 0, 0, 0x0001_0000, 0, 0, 0, 0x4000_0000]
        .into_iter()
        .enumerate()
    {
        let offset = 40 + index * 4;
        tkhd[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
    }
    tkhd[76..80].copy_from_slice(&(1920_u32 << 16).to_be_bytes());
    tkhd[80..84].copy_from_slice(&(1080_u32 << 16).to_be_bytes());
    let mut mdhd = vec![0_u8; 20];
    mdhd[12..16].copy_from_slice(&3_u32.to_be_bytes());
    mdhd[16..20].copy_from_slice(&3_u32.to_be_bytes());
    let mut hdlr = vec![0_u8; 12];
    hdlr[8..12].copy_from_slice(b"vide");
    let mut entry = vec![0_u8; 78];
    entry[24..26].copy_from_slice(&1920_u16.to_be_bytes());
    entry[26..28].copy_from_slice(&1080_u16.to_be_bytes());
    let stsd = atom(
        *b"stsd",
        &[
            &[0_u8; 4],
            &1_u32.to_be_bytes(),
            atom(*b"avc1", &entry).as_slice(),
        ]
        .concat(),
    );
    let stts = atom(
        *b"stts",
        &[
            [0_u8; 4].as_slice(),
            1_u32.to_be_bytes().as_slice(),
            3_u32.to_be_bytes().as_slice(),
            1_u32.to_be_bytes().as_slice(),
        ]
        .concat(),
    );
    let stbl = atom(*b"stbl", &[stsd, stts].concat());
    let minf = atom(*b"minf", &stbl);
    let mdia = atom(
        *b"mdia",
        &[atom(*b"mdhd", &mdhd), atom(*b"hdlr", &hdlr), minf].concat(),
    );
    let trak = atom(*b"trak", &[atom(*b"tkhd", &tkhd), mdia].concat());
    let moov = atom(*b"moov", &[atom(*b"mvhd", &mvhd), trak].concat());
    [ftyp, moov].concat()
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn supplementary_package_metadata_stages_exact_exr_wave_and_fractional_quicktime_profiles() {
    let root = tempfile::tempdir().unwrap();
    let exr_path = root.path().join("still.exr");
    let wave_path = root.path().join("voice.wav");
    let movie_path = root.path().join("shared.mov");
    fs::write(&exr_path, exr(321, 123)).unwrap();
    fs::write(&wave_path, wave()).unwrap();
    fs::write(&movie_path, quicktime()).unwrap();
    let archive_path = archive_with_assets(
        root.path(),
        Vec::new(),
        &[
            ("still-exr", &exr_path, AssetKind::Image),
            ("voice-wave", &wave_path, AssetKind::Audio),
            ("shared-mov", &movie_path, AssetKind::Video),
        ],
    );
    let archive = TesseractFile::open(archive_path).unwrap();
    let staging = root.path().join("staging");
    fs::create_dir(&staging).unwrap();
    let prepared = package::prepare_media(
        &archive,
        &[
            request("still-exr", FootageKind::Image),
            request("voice-wave", FootageKind::Audio),
            request("shared-mov", FootageKind::Video),
        ],
        &staging,
        archive.project().dimensions(),
        crate::AepPreparationControl::default(),
    )
    .unwrap();
    assert!(prepared.unsupported.is_empty());
    assert_eq!(prepared.files.len(), 3);
    assert!(!staging.join(".asset-materialization-cache").exists());

    let still = &prepared.sources["still-exr"];
    assert_eq!(still.format, NativeSourceFormat::OpenExr);
    assert_eq!(still.dimensions, [321, 123]);
    assert_eq!(
        (
            still.duration_millis,
            still.frame_rate,
            still.audio_sample_rate
        ),
        (0, NativeFrameRate::integer(0), 0.0)
    );
    assert_eq!(
        fs::read(staging.join(still.path.as_str())).unwrap(),
        exr(321, 123)
    );

    let voice = &prepared.sources["voice-wave"];
    assert_eq!(voice.format, NativeSourceFormat::Wave);
    assert_eq!(voice.dimensions, [0, 0]);
    assert_eq!(voice.duration_millis, 1);
    assert_eq!(voice.audio_sample_rate, 8_000.0);

    let movie = &prepared.sources["shared-mov"];
    assert_eq!(movie.format, NativeSourceFormat::QuickTime);
    assert_eq!(movie.dimensions, [1920, 1080]);
    assert_eq!(movie.duration_millis, 2_000);
    assert_eq!(
        movie.frame_rate,
        NativeFrameRate {
            integer: 1,
            fractional: 32_768
        }
    );
    assert_eq!(movie.audio_sample_rate, 0.0);
}

#[test]
fn extensible_pcm24_wave_is_prepared_without_changing_sample_bytes() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("source.wav");
    let original = extensible_pcm_wave(true);
    fs::write(&path, &original).unwrap();
    let archive_path = archive_with_assets(
        root.path(),
        Vec::new(),
        &[("pcm24", &path, AssetKind::Audio)],
    );
    let archive = TesseractFile::open(archive_path).unwrap();
    let staging = root.path().join("staging");
    fs::create_dir(&staging).unwrap();
    let prepared = package::prepare_media(
        &archive,
        &[request("pcm24", FootageKind::Audio)],
        &staging,
        archive.project().dimensions(),
        crate::AepPreparationControl::default(),
    )
    .unwrap();
    assert!(
        prepared.unsupported.is_empty(),
        "{:?}",
        prepared.unsupported
    );
    assert_eq!(prepared.files.len(), 1);
    let source = &prepared.sources["pcm24"];
    assert_eq!(source.format, NativeSourceFormat::Wave);
    assert_eq!(source.audio_sample_rate, 48_000.0);
    assert_eq!(source.wave_metadata.as_ref().unwrap().sample_frames, 1);
    let output = fs::read(staging.join(source.path.as_str())).unwrap();
    assert_eq!(&output[20..22], &1_u16.to_le_bytes());
    assert_eq!(&output[44..47], &original[68..71]);
}

#[test]
fn primed_mp3_audio_is_prepared_as_staged_pcm_wave() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("source.mp3");
    fs::write(
        &path,
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../media_transcode/tests/fixtures/ae_primed_mono.mp3"
        )),
    )
    .unwrap();
    let archive_path = archive_with_assets(
        root.path(),
        Vec::new(),
        &[("primed", &path, AssetKind::Audio)],
    );
    let archive = TesseractFile::open(archive_path).unwrap();
    let staging = root.path().join("staging");
    fs::create_dir(&staging).unwrap();
    let prepared = package::prepare_media(
        &archive,
        &[request("primed", FootageKind::Audio)],
        &staging,
        archive.project().dimensions(),
        crate::AepPreparationControl::default(),
    )
    .unwrap();
    assert!(
        prepared.unsupported.is_empty(),
        "{:?}",
        prepared.unsupported
    );
    let source = &prepared.sources["primed"];
    assert_eq!(source.format, NativeSourceFormat::Wave);
    assert!(source.wave_metadata.as_ref().unwrap().sample_frames > 0);
    assert_eq!(prepared.files.len(), 1);
}

#[test]
fn non_pcm_wave_extensible_subtype_remains_unsupported() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("source.wav");
    fs::write(&path, extensible_pcm_wave(false)).unwrap();
    let archive_path = archive_with_assets(
        root.path(),
        Vec::new(),
        &[("wrong-subtype", &path, AssetKind::Audio)],
    );
    let archive = TesseractFile::open(archive_path).unwrap();
    let staging = root.path().join("staging");
    fs::create_dir(&staging).unwrap();
    let prepared = package::prepare_media(
        &archive,
        &[request("wrong-subtype", FootageKind::Audio)],
        &staging,
        archive.project().dimensions(),
        crate::AepPreparationControl::default(),
    )
    .unwrap();
    assert_eq!(prepared.unsupported.len(), 1);
    assert!(prepared.files.is_empty());
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn unsupported_valid_format_keeps_sibling_with_check_write_parity_and_no_media_publication() {
    let root = tempfile::tempdir().unwrap();
    let png = root.path().join("unsupported.png");
    fs::write(&png, b"synthetic-valid-profile-placeholder").unwrap();
    let imported = import_solid(root.path());
    let source = TesseractFile::open(imported).unwrap();
    let mut value = source.project_json().unwrap();
    let mut solid =
        value["composition"]["layers"][0]["layers"][0]["layers"][0]["layers"][0].clone();
    solid["parent"] = serde_json::Value::Null;
    solid["id"] = serde_json::json!(801);
    let image = serde_json::json!({
        "type":"Image", "id":800, "name":"unsupported PNG", "parent":null,
        "activeRange":{"start":0,"duration":1000}, "transform":transform(),
        "source":{"assetId":"unsupported-image","fit":"contain"}
    });
    value["composition"]["layers"] = vec![image, solid].into();
    value["composition"]["dynamics"] = serde_json::json!({"entries":[]});
    let input = archive_value_with_assets(
        root.path(),
        value,
        &[("unsupported-image", &png, AssetKind::Image)],
    );
    let output = root.path().join("output");

    let checked = AfterEffects
        .export_from_tesseract(&input, &output, &Default::default(), ConversionMode::Check)
        .unwrap();
    assert!(!output.exists());
    let written = AfterEffects
        .export_from_tesseract(&input, &output, &Default::default(), ConversionMode::Write)
        .unwrap();
    assert_eq!(checked, written);
    assert!(written.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("source-backed OpenEXR native profile")
            && diagnostic.message.contains("affected media layers omitted")
    }));
    assert!(!output.join("media").exists());
    let native = read_project(&fs::read(output.join("project.aep")).unwrap()).unwrap();
    let ItemKind::Composition(composition) = &native.item(1).unwrap().kind else {
        panic!("root composition")
    };
    assert_eq!(composition.layers.len(), 1);
    assert_eq!(composition.layers[0].record.id(), 801);
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn malformed_required_metadata_is_fatal_before_check_or_write_publication() {
    let root = tempfile::tempdir().unwrap();
    let malformed = root.path().join("broken.exr");
    fs::write(&malformed, b"not an EXR").unwrap();
    let image = serde_json::json!({
        "type":"Image", "id":810, "name":"broken", "parent":null,
        "activeRange":{"start":0,"duration":1000}, "transform":transform(),
        "source":{"assetId":"broken-exr","fit":"contain"}
    });
    let input = archive_with_assets(
        root.path(),
        vec![image],
        &[("broken-exr", &malformed, AssetKind::Image)],
    );
    for (index, mode) in [ConversionMode::Check, ConversionMode::Write]
        .into_iter()
        .enumerate()
    {
        let output = root.path().join(format!("output-{index}"));
        let error = AfterEffects
            .export_from_tesseract(&input, &output, &Default::default(), mode)
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("malformed or missing dataWindow")
        );
        assert!(!output.exists());
    }
}

/// Package-path regression only; the tiny WAVE supplements native render proof.
#[test]
fn published_file_alias_points_to_final_package_and_retains_relocation_hints() {
    let root = tempfile::tempdir().unwrap();
    let media = root.path().join("voice.wav");
    let original = wave();
    fs::write(&media, &original).unwrap();
    let input = archive_with_assets(
        root.path(),
        vec![serde_json::json!({
            "type":"Audio","id":901,"name":"Packaged audio","parent":null,
            "playback":{"type":"windowed","inputRange":{"start":0,"duration":1},
                "mapping":{"type":"linear","input":{"start":0,"duration":1},
                    "output":{"start":0,"duration":1}},"inputOffsetMs":0},
            "sourceRange":{"start":0,"duration":1},"sourceIntrinsicDuration":1,
            "volume":1,"source":{"assetId":"voice"}
        })],
        &[("voice", &media, AssetKind::Audio)],
    );
    let output = root.path().join("published");
    let report = AfterEffects
        .export_from_tesseract(&input, &output, &Default::default(), ConversionMode::Write)
        .unwrap();
    let asset = report
        .artifacts
        .iter()
        .find(|entry| entry.kind == fx_conv::ArtifactKind::Media)
        .unwrap();
    let expected = fs::canonicalize(&output).unwrap().join(&asset.path);
    let bytes = fs::read(output.join("project.aep")).unwrap();
    let native = read_project(&bytes).unwrap();
    let source = native
        .items
        .iter()
        .find_map(|item| item.media.as_ref())
        .unwrap()
        .as_ref()
        .unwrap();
    assert_eq!(Path::new(&source.authored_path), expected);
    assert_eq!(fs::read(expected).unwrap(), original);
    for hint in [
        br#""ascendcount_base":1"#.as_slice(),
        br#""ascendcount_target":2"#,
    ] {
        assert!(bytes.windows(hint.len()).any(|value| value == hint));
    }
    assert_eq!(fs::read(media).unwrap(), original);
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn staged_publication_rolls_back_only_owned_entries_and_rejects_unsafe_paths() {
    for unsafe_path in [
        "media/../escape.mov",
        "../media/escape.mov",
        "/media/escape.mov",
        "media\\escape.mov",
    ] {
        assert!(
            RelativeMediaPath::new(unsafe_path).is_err(),
            "{unsafe_path}"
        );
    }

    let root = tempfile::tempdir().unwrap();
    let staged = root.path().join("staged");
    fs::create_dir_all(staged.join("media")).unwrap();
    fs::write(staged.join("project.aep"), b"fresh project").unwrap();
    fs::write(staged.join("media/present.wav"), b"owned media").unwrap();
    let output = root.path().join("output");
    let media = [
        RelativeMediaPath::new("media/present.wav").unwrap(),
        RelativeMediaPath::new("media/missing.wav").unwrap(),
    ];
    assert!(package::publish_package(&staged, &output, &media, &[]).is_err());
    assert!(!output.exists());
    assert_eq!(
        fs::read(staged.join("project.aep")).unwrap(),
        b"fresh project"
    );
    assert_eq!(
        fs::read(staged.join("media/present.wav")).unwrap(),
        b"owned media"
    );

    fs::create_dir(&output).unwrap();
    fs::write(output.join("concurrent-user-file"), b"keep").unwrap();
    assert!(package::publish_package(&staged, &output, &[], &[]).is_err());
    assert_eq!(
        fs::read(output.join("concurrent-user-file")).unwrap(),
        b"keep"
    );
}
