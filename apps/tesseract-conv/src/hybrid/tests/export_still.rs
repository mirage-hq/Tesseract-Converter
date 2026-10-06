//! End-to-end native OpenEXR export regression; host render proof stays outside git.
use super::*;

const EXR: &[u8] = include_bytes!(
    "../../../../../crates/aftereffects_file/tests/fixtures/media_native_panel/media/red.exr"
);

fn exr_with_comments() -> Vec<u8> {
    let mut bytes = EXR.to_vec();
    let mut cursor = 8;
    loop {
        let name_end = cursor + bytes[cursor..].iter().position(|byte| *byte == 0).unwrap();
        if name_end == cursor {
            break;
        }
        cursor = name_end + 1;
        let kind_end = cursor + bytes[cursor..].iter().position(|byte| *byte == 0).unwrap();
        cursor = kind_end + 1;
        let length = usize::try_from(u32::from_le_bytes(
            bytes[cursor..cursor + 4].try_into().unwrap(),
        ))
        .unwrap();
        cursor += 4 + length;
    }
    let old_table = cursor + 1;
    let first_chunk = u64::from_le_bytes(bytes[old_table..old_table + 8].try_into().unwrap());
    let table_bytes = usize::try_from(first_chunk).unwrap() - old_table;
    assert_eq!(table_bytes % 8, 0);

    let comment = b"shoe photo retained for editable recovery";
    let mut attribute = b"comments\0string\0".to_vec();
    attribute.extend_from_slice(&u32::try_from(comment.len()).unwrap().to_le_bytes());
    attribute.extend_from_slice(comment);
    let added = attribute.len();
    bytes.splice(cursor..cursor, attribute);

    let table = old_table + added;
    for index in 0..table_bytes / 8 {
        let start = table + index * 8;
        let offset = u64::from_le_bytes(bytes[start..start + 8].try_into().unwrap());
        bytes[start..start + 8]
            .copy_from_slice(&(offset + u64::try_from(added).unwrap()).to_le_bytes());
    }
    bytes
}

#[test]
fn export_still_exr_stays_native_with_independent_picture_and_sound() {
    let parent = tempfile::tempdir().unwrap();
    let mut value = document_value();
    value["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .retain(|layer| layer["id"] == 20);
    value["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .insert(
            1,
            json!({
                "type":"Image", "id":30, "name":"EXR source picture",
                "activeRange":{"start":0,"duration":1000},
                "transform":{"anchorPoint":[0,0],"position":[0,0],"scale":[100,100],
                    "rotation":0,"opacity":100},
                "source":{"assetId":"source-image","fit":"contain",
                    "sourceRect":{"x":0,"y":0,"width":64,"height":48}}
            }),
        );
    value["duration"] = json!(1.001);
    let audio = value["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|layer| layer["id"] == 20)
        .unwrap();
    audio["playback"]["inputRange"]["start"] = json!(801);
    audio["playback"]["mapping"]["input"]["start"] = json!(801);

    let comments_exr = exr_with_comments();
    let source = parent.path().join("source.exr");
    fs::write(&source, &comments_exr).unwrap();
    let input = parent.path().join("input.tsrct");
    TesseractFileBuilder::from_project_json(&serde_json::to_vec(&value).unwrap())
        .unwrap()
        .add_asset("source-image", &source, AssetKind::Image)
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
    assert!(text.contains("./media/source.exr"));
    assert!(text.contains("<Title>source.exr</Title>"));
    assert!(text.contains("<CodecType>1281443650</CodecType>"));
    assert!(text.contains(">b0VYUgEBAA"));
    assert!(text.contains("./media/audio-mono.wav"));
    assert!(!text.contains(".aep"));
    assert!(!written
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message.contains("OpenEXR")
            && diagnostic.message.contains("omitted")));

    let packaged = written
        .artifacts
        .iter()
        .find(|artifact| {
            artifact
                .path
                .extension()
                .is_some_and(|extension| extension == "exr")
        })
        .expect("native Premiere package retains the original EXR");
    assert_eq!(fs::read(output.join(&packaged.path)).unwrap(), comments_exr);
    assert!(!written.artifacts.iter().any(|artifact| {
        artifact
            .path
            .extension()
            .is_some_and(|extension| extension == "aep")
    }));
    assert_eq!(
        fs::read(output.join("media/audio-mono.wav")).unwrap(),
        fs::read(fixture("audio-mono.wav")).unwrap()
    );
    assert_eq!(fs::read(input).unwrap(), original);
}
