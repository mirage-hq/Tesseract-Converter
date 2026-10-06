use serde_json::{json, Value};
use std::{fs, io::Write, path::Path};
use tesseract_file::{AssetKind, TesseractFileBuilder};

/// Adobe ticks per second. Integration tests cannot see the crate constant.
pub(crate) const TICKS: i64 = 254_016_000_000;

pub(crate) fn editable_document() -> Value {
    serde_json::from_str(include_str!("../fixtures/editable-video.json")).unwrap()
}

/// An authored affine mapping whose visible window selects `source`.
pub(crate) fn linear_playback(window: Value, source: Value) -> Value {
    json!({
        "type": "windowed", "inputRange": window,
        "mapping": {"type": "linear", "input": window, "output": source},
        "inputOffsetMs": 0
    })
}

/// The visible range from each layer type's canonical timing field.
pub(crate) fn layer_range(layer: &Value) -> &Value {
    let range = match layer["type"].as_str().unwrap() {
        "Video" | "Audio" | "Group" => &layer["playback"]["inputRange"],
        "Media" | "Image" | "Text" | "Rect" | "Shape" | "BooleanOperation" | "Pag" | "AiEdit"
        | "Adjustment" => &layer["activeRange"],
        unexpected => panic!("unexpected fixture layer type {unexpected}"),
    };
    assert!(range.is_object(), "missing canonical layer range: {layer}");
    range
}

pub(crate) fn write_prproj(path: &Path, xml: &str) {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(xml.as_bytes()).unwrap();
    fs::write(path, encoder.finish().unwrap()).unwrap();
}

/// Package the single video asset referenced by the editable document fixture.
pub(crate) fn write_archive(path: &Path, document: &Value, media: &Path) {
    TesseractFileBuilder::from_project_json(&serde_json::to_vec(document).unwrap())
        .unwrap()
        .add_asset("premiere-video-1", media, AssetKind::Video)
        .unwrap()
        .write(path)
        .unwrap();
}
