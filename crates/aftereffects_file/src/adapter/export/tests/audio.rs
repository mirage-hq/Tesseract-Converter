//! Supplementary archive/publication evidence, not an Adobe audio oracle.

use super::*;
use tesseract_file::{AssetKind, TesseractFile, TesseractFileBuilder};

fn wave() -> Vec<u8> {
    // Four seconds of synthetic mono PCM; do not substitute this for the
    // unavailable independently authored native fixture's reference audio.
    let size = 4 * 8_000 * 2_u32;
    let mut bytes = b"RIFF".to_vec();
    bytes.extend_from_slice(&(36 + size).to_le_bytes());
    bytes.extend_from_slice(
        b"WAVEfmt \x10\0\0\0\x01\0\x01\0\x40\x1f\0\0\x80\x3e\0\0\x02\0\x10\0data",
    );
    bytes.extend_from_slice(&size.to_le_bytes());
    bytes.resize(44 + usize::try_from(size).unwrap(), 0);
    bytes
}

fn find_audio(layers: &[fx_schema::Layer]) -> Option<&fx_schema::AudioLayer> {
    layers.iter().find_map(|layer| match layer.data() {
        LayerData::Audio(audio) => Some(audio),
        LayerData::Group(group) => find_audio(&group.layers),
        _ => None,
    })
}

#[test]
fn audio_package_check_write_and_reimport_preserve_edited_gain_and_wave_bytes() {
    let root = tempfile::tempdir().unwrap();
    let seed = TesseractFile::open(import_solid(root.path())).unwrap();
    let mut value = seed.project_json().unwrap();
    value["composition"]["layers"] = json!([{
        "type":"Audio", "id":700, "name":"Edited voice", "parent":null,
        "playback":{"type":"windowed","inputRange":{"start":1000,"duration":2000},"mapping":{"type":"linear","input":{"start":1000,"duration":2000},"output":{"start":500,"duration":2000}},"inputOffsetMs":0},
        "sourceRange":{"start":500,"duration":1000},
        "sourceIntrinsicDuration":4000, "volume":0.5,
        "source":{"assetId":"voice"}
    }]);
    value["composition"]["dynamics"] = json!({"entries":[]});
    let document = EditableFxCompositionDocument::from_json_value(value).unwrap();
    let bytes = wave();
    let media = root.path().join("voice.wav");
    fs::write(&media, &bytes).unwrap();
    let archive_path = root.path().join("edited.tsrct");
    drop(
        TesseractFileBuilder::try_new(document)
            .unwrap()
            .add_asset("voice", &media, AssetKind::Audio)
            .unwrap()
            .write(&archive_path)
            .unwrap(),
    );
    let output = root.path().join("exported");
    let checked = AfterEffects
        .export_from_tesseract(
            &archive_path,
            &output,
            &Default::default(),
            ConversionMode::Check,
        )
        .unwrap();
    assert!(!output.exists());
    let written = AfterEffects
        .export_from_tesseract(
            &archive_path,
            &output,
            &Default::default(),
            ConversionMode::Write,
        )
        .unwrap();
    assert_eq!(checked, written);
    let native_path = output.join("project.aep");
    let native = read_project(&fs::read(&native_path).unwrap()).unwrap();
    let source = native
        .items
        .iter()
        .find_map(|item| item.media.as_ref())
        .unwrap()
        .as_ref()
        .unwrap();
    // Publication binds native aliases to the final package's absolute path;
    // artifact paths remain relative to that package directory.
    let package_root = fs::canonicalize(&output).unwrap();
    let media_path = std::path::Path::new(&source.authored_path)
        .strip_prefix(&package_root)
        .unwrap()
        .to_str()
        .unwrap();
    assert_eq!(
        written.artifacts,
        [
            fx_conv::Artifact::project("project.aep"),
            fx_conv::Artifact::media(media_path),
        ]
    );
    assert!(
        written
            .artifacts
            .iter()
            .all(|artifact| output.join(&artifact.path).is_file())
    );
    assert_eq!(fs::read(output.join(&source.authored_path)).unwrap(), bytes);
    assert_eq!(source.audio_sample_rate, 8000.0);
    assert_eq!(source.duration.seconds(), 4.0);

    // Delete original inputs: import must resolve the freshly exported package,
    // not replay original AEP bytes or depend on the input archive/source path.
    fs::remove_file(&media).unwrap();
    fs::remove_file(&archive_path).unwrap();
    let reimported = root.path().join("reimported");
    AfterEffects
        .import_to_tesseract(
            &native_path,
            &reimported,
            &AfterEffectsImportOptions {
                composition: Some(1),
                ..Default::default()
            },
            ConversionMode::Write,
        )
        .unwrap();
    let archive = TesseractFile::open(reimported.join("project.tsrct")).unwrap();
    let document =
        EditableFxCompositionDocument::from_json_value(archive.project_json().unwrap()).unwrap();
    let audio = find_audio(document.composition().layers()).unwrap();
    assert!((audio.volume.as_f64() - 0.5).abs() < 1e-9);
    assert_eq!(
        archive
            .asset(audio.source.asset_id.as_str())
            .unwrap()
            .read_verified_bytes(100_000)
            .unwrap(),
        bytes
    );
}
