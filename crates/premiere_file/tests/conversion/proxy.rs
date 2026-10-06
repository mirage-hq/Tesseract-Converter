//! Saved Premiere 26.5.2 proxy attachment.
//! Import and ordinary packaged-export structure proof, not render proof.
use super::support::*;
#[cfg(feature = "ffmpeg-library")]
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{fs, path::Path};
#[cfg(feature = "ffmpeg-library")]
use tesseract_file::{AssetKind, TesseractFile, TesseractFileBuilder};

const SEQUENCE: &str = "e039e9b8-f6c9-45e8-a9f4-5518b7b7792a";
const ORIGINAL_ID: &str = "55212ddf-6f52-4b70-8967-170955399b93";
#[cfg(feature = "ffmpeg-library")]
const PROXY_ID: &str = "158958af-e18f-4cd5-bf94-edfa14096597";
const ORIGINAL: &[u8] = include_bytes!("../fixtures/native-proxy/original.mp4");
const PROXY: &[u8] = include_bytes!("../fixtures/native-proxy/proxy.mp4");

fn native_xml() -> String {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/native-proxy/project.prproj");
    for (bytes, hash) in [
        (
            fs::read(&path).unwrap(),
            "466ce3edbe45d457f9c8a32cab5cd988c1695a1467778e00392eec5aa52ce8f3",
        ),
        (
            ORIGINAL.to_vec(),
            "93ac4bb3308000732640f0b0c57c1fa1c2cb3264bc9675afe2cbc9cce02169a3",
        ),
        (
            PROXY.to_vec(),
            "85546262a2bcfe5feb7d5f2e16a606ee71ab018d673f6e1562d32b71cd51e380",
        ),
    ] {
        assert_eq!(format!("{:x}", Sha256::digest(bytes)), hash);
    }
    read_xml(&path)
}

// Replace every resolver alias, not global filename strings or native content.
// Both positive and negative run independently of the author's retained paths.
#[cfg(feature = "ffmpeg-library")]
fn relocate(xml: &str, media_id: &str, path: &Path) -> String {
    let document = roxmltree::Document::parse(xml).unwrap();
    let media = document
        .descendants()
        .find(|node| node.has_tag_name("Media") && node.attribute("ObjectUID") == Some(media_id))
        .unwrap();
    let aliases: Vec<_> = media
        .children()
        .filter(|node| {
            matches!(
                node.tag_name().name(),
                "RelativePath" | "ActualMediaFilePath" | "FilePath"
            )
        })
        .collect();
    assert_eq!(aliases.len(), 3);
    let mut relocated = xml.to_owned();
    for alias in aliases.into_iter().rev() {
        let text = alias.first_child().unwrap();
        assert!(text.is_text());
        let path = if alias.has_tag_name("RelativePath") {
            Path::new(path.file_name().unwrap())
        } else {
            path
        };
        relocated.replace_range(
            text.range(),
            &quick_xml::escape::escape(path.to_str().unwrap()),
        );
    }
    relocated
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn native_proxy_content_packages_original_with_saved_timing() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let original = root.join("original.mp4");
    let proxy = root.join("proxy.mp4");
    fs::write(&original, ORIGINAL).unwrap();
    fs::write(&proxy, PROXY).unwrap();
    let xml = relocate(
        &relocate(&native_xml(), ORIGINAL_ID, &original),
        PROXY_ID,
        &proxy,
    );
    let input = root.join("positive.prproj");
    write_prproj(&input, &xml);
    let output = root.join("positive");
    let omissions = premiere_to_tesseract(&input, &output, Some(SEQUENCE), false).unwrap();
    // Existing sequence color-management losses are not proxy substitution.
    assert_eq!(
        omissions
            .iter()
            .map(|item| (item.record.as_str(), item.reason.as_str()))
            .collect::<Vec<_>>(),
        [
            (
                "VideoTrackGroup:59",
                "ToneMappingDesaturation not converted"
            ),
            (
                "VideoTrackGroup:59",
                "nondefault ColorManagementSettings not converted"
            ),
        ]
    );
    let file = TesseractFile::open(first_project(&output)).unwrap();
    let document = file.project_json().unwrap();
    assert_eq!(
        document["dimensions"],
        json!({"width": 1920, "height": 1080})
    );
    assert_eq!(document["duration"], 1.0);
    let layers = video_layers(&document);
    assert_eq!(layers.len(), 1);
    assert!(!document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|layer| layer["type"] == "Audio"));
    let video = layers[0];
    assert_eq!(video["volume"].as_f64(), Some(0.0));
    for (field, value) in [
        ("x", 0.0),
        ("y", 0.0),
        ("width", 1920.0),
        ("height", 1080.0),
    ] {
        assert_eq!(video["source"]["sourceRect"][field].as_f64(), Some(value));
    }
    assert_eq!(
        *crate::test_support::layer_range(video),
        json!({"start": 0, "duration": 1000})
    );
    assert_eq!(
        video["sourceRange"],
        json!({"start": 500, "duration": 1000})
    );
    assert_eq!(
        video["playback"],
        crate::test_support::linear_playback(
            json!({"start": 0, "duration": 1000}),
            json!({"start": 500, "duration": 1000})
        )
    );
    assert_eq!(video["sourceIntrinsicDuration"], 2000);
    assert_eq!(file.metadata().assets.len(), 1);
    let bytes = file
        .asset(video["source"]["assetId"].as_str().unwrap())
        .unwrap()
        .read_verified_bytes(ORIGINAL.len() as u64)
        .unwrap();
    assert_eq!(bytes, ORIGINAL);
    assert_ne!(bytes, PROXY);

    // An edited second use shares the original asset, not the attached proxy.
    let mut edited = document.clone();
    let mut repeated = video.clone();
    repeated["id"] = json!(9001);
    repeated["name"] = json!("Original repeated");
    repeated["playback"] = crate::test_support::linear_playback(
        json!({"start": 1000, "duration": 1000}),
        json!({"start": 500, "duration": 1000}),
    );
    edited["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .push(repeated);
    edited["duration"] = json!(2.0);
    let archive = root.join("repeated.tsrct");
    TesseractFileBuilder::from_project_json(&serde_json::to_vec(&edited).unwrap())
        .unwrap()
        .add_asset(
            video["source"]["assetId"].as_str().unwrap(),
            &original,
            AssetKind::Video,
        )
        .unwrap()
        .write(&archive)
        .unwrap();
    let package = root.join("export");
    tesseract_to_premiere(&archive, &package, false).unwrap();
    let exported = read_xml(&package.join("project.prproj"));
    assert!(!exported.contains("ProxyMedia"));
    assert!(!exported.contains(PROXY_ID));
    let media: Vec<_> = fs::read_dir(package.join("media"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(media.len(), 1);
    assert_eq!(fs::read(&media[0]).unwrap(), ORIGINAL);
    let relocated = root.join("relocated-package");
    fs::rename(&package, &relocated).unwrap();
    let reimport = root.join("reimport");
    premiere_to_tesseract(relocated.join("project.prproj"), &reimport, None, false).unwrap();
    let reopened = TesseractFile::open(first_project(&reimport)).unwrap();
    let reopened_document = reopened.project_json().unwrap();
    let mut pictures = video_layers(&reopened_document);
    pictures.sort_by_key(|layer| {
        crate::test_support::layer_range(layer)["start"]
            .as_i64()
            .unwrap()
    });
    assert_eq!(pictures.len(), 2);
    assert_eq!(reopened.metadata().assets.len(), 1);
    assert_eq!(reopened_document["duration"], 2.0);
    for (index, picture) in pictures.iter().enumerate() {
        assert_eq!(
            crate::test_support::layer_range(picture),
            &json!({"start": index * 1000, "duration": 1000})
        );
        assert_eq!(picture["sourceRange"], video["sourceRange"]);
        assert_eq!(picture["sourceIntrinsicDuration"], 2000);
        assert_eq!(
            picture["source"]["sourceRect"],
            video["source"]["sourceRect"]
        );
        assert_eq!(picture["volume"], 0.0);
        let bytes = reopened
            .asset(picture["source"]["assetId"].as_str().unwrap())
            .unwrap()
            .read_verified_bytes(ORIGINAL.len() as u64)
            .unwrap();
        assert_eq!(bytes, ORIGINAL);
    }

    // The proxy stays live; all three primary aliases point to a missing file.
    // Retained originals are never hidden, renamed or removed.
    let missing = root.join("missing-original.mp4");
    let negative = root.join("negative.prproj");
    write_prproj(&negative, &relocate(&xml, ORIGINAL_ID, &missing));
    let output = root.join("negative");
    let error = premiere_to_tesseract(&negative, &output, Some(SEQUENCE), false).unwrap_err();
    assert!(error.to_string().contains("missing media:"), "{error}");
    assert!(!output.exists());
    assert_eq!(fs::read(original).unwrap(), ORIGINAL);
    assert_eq!(fs::read(proxy).unwrap(), PROXY);
}

#[test]
fn native_proxy_content_does_not_admit_unknown_content_or_replace_primary_link() {
    let xml = native_xml();
    let primary = format!("<Media ObjectURef=\"{ORIGINAL_ID}\"/>");
    assert_eq!(xml.matches(&primary).count(), 1);
    for (xml, expected) in [
        (
            xml.replace("<ProxyMedia ", "<UnknownMedia "),
            "unknown field `UnknownMedia`",
        ),
        (
            xml.replace(&primary, ""),
            "VideoMediaSource:49: missing Media",
        ),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let input = temp.path().join("project.prproj");
        write_prproj(&input, &xml);
        let output = temp.path().join("output");
        let error = premiere_to_tesseract(&input, &output, Some(SEQUENCE), false).unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
        assert!(!output.exists());
    }
}
