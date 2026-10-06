//! Supplementary public Check/Write regression. The sRGB declaration is added
//! to real encoder output; independent original-source evidence stays private.
use super::*;

fn srgb_video() -> Vec<u8> {
    let mut bytes = fs::read(fixture("video-30fps.mov")).unwrap();
    let mut parents = Vec::new();
    let mut start = 0;
    for tag in [b"moov", b"trak", b"mdia", b"minf", b"stbl", b"stsd"] {
        while &bytes[start + 4..start + 8] != tag {
            start += u32::from_be_bytes(bytes[start..start + 4].try_into().unwrap()) as usize;
        }
        parents.push(start);
        start += 8;
    }
    let entry = start + 8;
    parents.push(entry);
    let end = entry + u32::from_be_bytes(bytes[entry..entry + 4].try_into().unwrap()) as usize;
    assert!(parents[0] > bytes.windows(4).position(|p| p == b"mdat").unwrap());
    bytes.splice(
        end..end,
        [
            19_u32.to_be_bytes().as_slice(),
            b"colrnclx",
            &[0, 1, 0, 13, 0, 2, 0],
        ]
        .concat(),
    );
    for offset in parents {
        let size = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) + 19;
        bytes[offset..offset + 4].copy_from_slice(&size.to_be_bytes());
    }
    bytes
}

#[test]
fn export_media_srgb_uses_bounded_editable_ae_and_preserves_native_picture_and_audio() {
    let parent = tempfile::tempdir().unwrap();
    let mut value = document_value();
    value["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .retain(|layer| layer["id"] != 10);
    let mut native = value["composition"]["layers"][1].clone();
    native["id"] = json!(3);
    native["source"]["assetId"] = json!("native-picture");
    value["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .insert(2, native);
    let source_media = srgb_video();
    let source_path = parent.path().join("srgb.mov");
    fs::write(&source_path, &source_media).unwrap();
    let input = parent.path().join("input.tsrct");
    TesseractFileBuilder::from_project_json(&serde_json::to_vec(&value).unwrap())
        .unwrap()
        .add_asset("premiere-video-1", &source_path, AssetKind::Video)
        .unwrap()
        .add_asset(
            "native-picture",
            fixture("video-30fps.mp4"),
            AssetKind::Video,
        )
        .unwrap()
        .add_asset("music", fixture("audio-mono.wav"), AssetKind::Audio)
        .unwrap()
        .write(&input)
        .unwrap();
    let original = fs::read(&input).unwrap();
    let output = parent.path().join("output");
    let checked = export(
        &request(&input, &output, ConversionMode::Check),
        &Default::default(),
    )
    .unwrap();
    assert!(!output.exists());
    let written = export(
        &request(&input, &output, ConversionMode::Write),
        &Default::default(),
    )
    .unwrap();
    assert_eq!(checked, written);
    let text = xml(&output.join("project.prproj"));
    assert!(text.contains("./media/ae-0001/compositions.aep"));
    assert!(!text.contains("./media/ae-0002/compositions.aep"));
    assert!(text.contains("./media/video-30fps.mp4"));
    assert!(text.contains("./media/audio-mono.wav"));
    assert!(written
        .diagnostics
        .iter()
        .any(|d| d.code == "HYBRID-EXPERIMENTAL"));
    assert!(written
        .diagnostics
        .iter()
        .any(|d| d.message.contains("sRGB")));
    let packaged_source = written
        .artifacts
        .iter()
        .find(|a| a.path.extension().is_some_and(|e| e == "mov"))
        .unwrap();
    assert_eq!(
        fs::read(output.join(&packaged_source.path)).unwrap(),
        source_media
    );
    assert_eq!(
        fs::read(output.join("media/video-30fps.mp4")).unwrap(),
        fs::read(fixture("video-30fps.mp4")).unwrap()
    );
    assert_eq!(
        fs::read(output.join("media/audio-mono.wav")).unwrap(),
        fs::read(fixture("audio-mono.wav")).unwrap()
    );
    assert_eq!(fs::read(input).unwrap(), original);
}

#[test]
fn export_media_original_video_policy_also_preserves_other_selected_native_siblings() {
    let parent = tempfile::tempdir().unwrap();
    let mut value = document_value();
    value["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .retain(|layer| layer["id"] != 10);
    let mut native = value["composition"]["layers"][1].clone();
    native["id"] = json!(3);
    native["source"]["assetId"] = json!("native-picture");
    native["sourceIntrinsicDuration"] = json!(2000);
    native["effects"] = json!([{"id":300,"effect":{
        "type":"pixelMotionBlur", "shutterControl":"manual", "shutterAngle":120.0,
        "shutterSamples":8.0, "vectorDetail":20.0
    }}]);
    value["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .insert(2, native);
    let source = parent.path().join("srgb.mov");
    fs::write(&source, srgb_video()).unwrap();
    let input = parent.path().join("input.tsrct");
    TesseractFileBuilder::from_project_json(&serde_json::to_vec(&value).unwrap())
        .unwrap()
        .add_asset("premiere-video-1", &source, AssetKind::Video)
        .unwrap()
        .add_asset(
            "native-picture",
            fixture("feature_video_formats_hevc.mp4"),
            AssetKind::Video,
        )
        .unwrap()
        .add_asset("music", fixture("audio-mono.wav"), AssetKind::Audio)
        .unwrap()
        .write(&input)
        .unwrap();
    let original = fs::read(&input).unwrap();
    let output = parent.path().join("output");
    let checked = export(
        &request(&input, &output, ConversionMode::Check),
        &Default::default(),
    )
    .unwrap();
    assert!(!output.exists());
    let written = export(
        &request(&input, &output, ConversionMode::Write),
        &Default::default(),
    )
    .unwrap();
    assert_eq!(checked, written);
    assert!(written.diagnostics.iter().any(|d| d
        .message
        .contains("original video bytes required, destination preparation disabled")));
    let text = xml(&output.join("project.prproj"));
    assert!(text.contains("./media/ae-0001/compositions.aep"));
    assert!(!text.contains("./media/ae-0002/compositions.aep"));
    assert!(text.contains("./media/feature_video_formats_hevc.mp4"));
    assert_eq!(
        fs::read(output.join("media/feature_video_formats_hevc.mp4")).unwrap(),
        fs::read(fixture("feature_video_formats_hevc.mp4")).unwrap()
    );
    assert_eq!(fs::read(input).unwrap(), original);
}
