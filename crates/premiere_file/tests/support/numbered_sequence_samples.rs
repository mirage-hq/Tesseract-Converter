//! Native source 12eb4142d353317629664f32d746b4e468c7b862a94361b88dc14c6c97de1668.
//! Saved 30000/1001 source, 30fps sequence; only media paths are relocated.
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub fn import(root: &Path) -> PathBuf {
    import_with(root, |xml| xml)
}

pub fn import_with(root: &Path, edit: impl FnOnce(String) -> String) -> PathBuf {
    let frames: [&[u8]; 6] = [
        include_bytes!("../fixtures/numbered-sequence-samples/frame000.png"),
        include_bytes!("../fixtures/numbered-sequence-samples/frame001.png"),
        include_bytes!("../fixtures/numbered-sequence-samples/frame002.png"),
        include_bytes!("../fixtures/numbered-sequence-samples/frame003.png"),
        include_bytes!("../fixtures/numbered-sequence-samples/frame004.png"),
        include_bytes!("../fixtures/numbered-sequence-samples/frame005.png"),
    ];
    for (index, bytes) in frames.iter().enumerate() {
        fs::write(root.join(format!("frame{index:03}.png")), bytes).unwrap();
    }
    let mut xml = String::new();
    flate2::read::GzDecoder::new(
        &include_bytes!("../fixtures/numbered-sequence-samples/source.prproj")[..],
    )
    .read_to_string(&mut xml)
    .unwrap();
    for tag in ["FilePath", "ActualMediaFilePath"] {
        let start = xml.find(&format!("<{tag}>")).unwrap();
        let end = start + xml[start..].find(&format!("</{tag}>")).unwrap() + tag.len() + 3;
        xml.replace_range(start..end, "");
    }
    xml = edit(xml.replace("../inputs/frame000.png", "frame000.png"));
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(xml.as_bytes()).unwrap();
    let input = root.join("source.prproj");
    fs::write(&input, encoder.finish().unwrap()).unwrap();
    let output = root.join("import");
    let omissions = premiere_file::premiere_to_tesseract(
        &input,
        &output,
        Some("0d68ee84-a06d-40be-8926-ebb0e48826ea"),
        false,
    )
    .unwrap();
    eprintln!("{omissions:?}");
    output.join("project.tsrct")
}

pub fn swap(document: &mut serde_json::Value) {
    let groups: Vec<_> = document["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .filter(|layer| layer["type"] == "Group")
        .collect();
    assert_eq!(groups.len(), 2);
    for group in groups {
        let start = group["playback"]["inputRange"]["start"].as_u64().unwrap();
        assert!(start == 0 || start == 100);
        let other = 100 - start;
        group["playback"]["inputRange"]["start"] = other.into();
        group["playback"]["mapping"]["input"]["start"] = other.into();
    }
}
