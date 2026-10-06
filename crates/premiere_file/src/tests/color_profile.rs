//! Saved-profile metadata must not remove native SDR matte consumers.

#[cfg(feature = "ffmpeg-library")]
use crate::{Premiere, PremiereImportOptions};
#[cfg(feature = "ffmpeg-library")]
use fx_conv::{ConversionMode, ImportToTesseract};
#[cfg(feature = "ffmpeg-library")]
use serde_json::json;
use serde_json::Value;
#[cfg(feature = "ffmpeg-library")]
use std::{fs, path::Path};
#[cfg(feature = "ffmpeg-library")]
use tesseract_file::TesseractFile;

#[cfg(feature = "ffmpeg-library")]
const FIXTURE: &str = "feature_track_matte_key_26_5_strict.prproj";
#[cfg(feature = "ffmpeg-library")]
const TARGET: &str = "3776e3eb-791f-4e6a-a2bb-7e77eff235ef";
const PROFILE: &str = r#"{"baseColorProfile":{"colorProfileData":"AQAAAGQAAAA=","colorProfileName":"BT.709 RGB Full"},"baseProfileType":1,"colorSpaceMetadata":{"peakLuminance":100}}"#;

#[cfg(feature = "ffmpeg-library")]
fn fixtures() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures"))
}

#[cfg(feature = "ffmpeg-library")]
fn inline_layers(layers: &[Value]) -> Vec<&Value> {
    let mut all = Vec::new();
    for layer in layers {
        all.push(layer);
        if let Some(children) = layer["layers"].as_array() {
            all.extend(inline_layers(children));
        }
    }
    all
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn saved_sequence_sdr_profile_retains_native_alpha_and_luma_consumers() {
    // The native clips, keys, track IDs and media are the pinned public Adobe
    // fixture. Only its sequence profile is supplemented with saved SDR metadata;
    // this is an admission regression, not a new native render oracle.
    let native = crate::format::read_xml(&fixtures().join(FIXTURE)).unwrap();
    let baseline = crate::format::inspect_project_with_media(&native, Some(TARGET)).unwrap();
    let expected = crate::tests::support::project_document_with_media(
        baseline.single_sequence().unwrap(),
        &baseline.media,
    );
    let consumers: Vec<_> = expected["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|layer| layer["trackMatte"].is_object())
        .collect();
    assert!(consumers
        .iter()
        .any(|layer| layer["trackMatte"]["mode"] == "alpha"));
    assert!(consumers
        .iter()
        .any(|layer| layer["trackMatte"]["mode"] == "luma"));

    for profile in [
        PROFILE.to_owned(),
        PROFILE.replace("100", "203"),
        PROFILE.replace("AQAAAGQAAAA=", "different-saved-payload"),
        PROFILE.replace(r#""colorProfileData":"AQAAAGQAAAA=","#, ""),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let project = directory.path().join(FIXTURE);
        let group = native.find(r#"<VideoTrackGroup ObjectID="98""#).unwrap();
        let insertion = group + native[group..].find('>').unwrap() + 1;
        let mut xml = native.clone();
        xml.insert_str(
            insertion,
            &format!("<OutputColorSpace>{profile}</OutputColorSpace>"),
        );
        crate::test_support::write_prproj(&project, &xml);
        for file in [
            "feature_linked_av_source.mp4",
            "feature_timecoded_source.mp4",
            "tmk_alpha_rect.png",
            "tmk_red.png",
            "tmk_green.png",
            "tmk_blue.png",
            "tmk_grey128.png",
        ] {
            fs::copy(fixtures().join(file), directory.path().join(file)).unwrap();
        }
        let parsed = crate::format::inspect_project_with_media(&xml, Some(TARGET)).unwrap();
        assert_eq!(
            crate::tests::support::project_document_with_media(
                parsed.single_sequence().unwrap(),
                &parsed.media,
            )["composition"],
            expected["composition"],
            "{profile}"
        );
        for mode in [ConversionMode::Check, ConversionMode::Write] {
            let output = directory.path().join(format!("result-{mode:?}"));
            let report = Premiere
                .import_to_tesseract(
                    &project,
                    &output,
                    &PremiereImportOptions {
                        sequence: Some(TARGET.into()),
                    },
                    mode,
                )
                .unwrap();
            assert!(
                !report
                    .diagnostics
                    .iter()
                    .any(|note| note.reason.contains("OutputColorSpace")),
                "{report:?}"
            );
            if mode == ConversionMode::Check {
                assert!(!output.exists());
                continue;
            }
            let archive = TesseractFile::open(output.join("project.tsrct")).unwrap();
            let document = archive.project_json().unwrap();
            assert_eq!(
                document["composition"], expected["composition"],
                "{profile}"
            );
            let layers = inline_layers(document["composition"]["layers"].as_array().unwrap());
            // Each hidden matte provider remains referenced by its consumer;
            // profile recovery neither exposes it independently nor removes alpha.
            for consumer in layers
                .iter()
                .filter(|layer| layer["trackMatte"].is_object())
            {
                let provider = layers
                    .iter()
                    .find(|layer| layer["id"] == consumer["trackMatte"]["layer"])
                    .unwrap();
                assert_ne!(provider["id"], consumer["id"]);
                if provider["type"] == "Image" && consumer["trackMatte"]["mode"] != "luma" {
                    let id = provider["source"]["assetId"].as_str().unwrap();
                    let asset = archive.asset(id).unwrap();
                    assert_eq!(
                        asset.read_verified_bytes(1_000_000).unwrap(),
                        fs::read(fixtures().join("tmk_alpha_rect.png")).unwrap()
                    );
                }
            }
            let keyed = layers.iter().find(|layer| {
                layer["trackMatte"]["mode"] == "alpha"
                    && *crate::test_support::layer_range(layer)
                        == json!({"start":0,"duration":2000})
            });
            assert!(
                keyed.is_some(),
                "first native alpha consumer survives: {layers:?}"
            );
            assert!(
                layers.iter().any(|layer| layer["type"] == "Video"),
                "meaningful picture survives"
            );
        }
    }
}

#[test]
fn saved_sequence_sdr_profile_preserves_parsed_metadata() {
    let value: Value = serde_json::from_str(PROFILE).unwrap();
    let profile: crate::schema::ColorSpace = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(profile).unwrap(), value);
}
