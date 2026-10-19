//! Supplemental timing-table edits, not an independent Adobe oracle.
use super::*;

fn terminal_sample_bytes() -> Vec<u8> {
    sample_grid_bytes(&[0, 1000, 2000, 3000, 4000, 5000, 5000], 30000)
}

#[test]
fn terminal_zero_sample_import_keeps_positive_clock_and_editable_trim() {
    let bytes = terminal_sample_bytes();
    let ranges = std::iter::once(0..4 * FrameRate::Fps30.ticks_per_frame()).collect();
    let media = inspect_selected(&bytes, ranges, false).unwrap();
    assert_eq!(media.timing.sample_count, 5);
    assert_eq!(media.timing.supported().unwrap().0, FrameRate::Fps30);
    // Neither whole-source export nor an audio clock is inferred from this picture repair.
    assert!(inspect(&bytes).is_err());
    let ranges = std::iter::once(0..4 * FrameRate::Fps30.ticks_per_frame()).collect();
    assert!(inspect_selected(&bytes, ranges, true).is_err());
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("media")).unwrap();
    fs::write(root.path().join("media/source.mp4"), &bytes).unwrap();
    let xml = include_str!("../../../tests/fixtures/one-clip.xml")
        .replace(
            "2540160000000",
            &(5 * FrameRate::Fps30.ticks_per_frame()).to_string(),
        )
        .replace(
            "1270080000000",
            &(4 * FrameRate::Fps30.ticks_per_frame()).to_string(),
        );
    let input = root.path().join("source.prproj");
    crate::test_support::write_prproj(&input, &xml);
    let output = root.path().join("import");
    crate::premiere_to_tesseract(&input, &output, Some("sequence-1"), false).unwrap();
    let archive = tesseract_file::TesseractFile::open(output.join("project.tsrct")).unwrap();
    let document = archive.project_json().unwrap();
    let video = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["type"] == "Video")
        .unwrap();
    assert_eq!(crate::test_support::layer_range(video)["duration"], 133);
    assert_eq!(
        video["sourceRange"],
        serde_json::json!({"start":0,"duration":133})
    );
    let asset = video["source"]["assetId"].as_str().unwrap();
    assert_eq!(
        archive
            .asset(asset)
            .unwrap()
            .read_verified_bytes(bytes.len() as u64)
            .unwrap(),
        bytes
    );
}

#[test]
fn terminal_zero_sample_rejects_interior_and_multiple_zero_samples() {
    for starts in [
        [0, 1000, 2000, 2000, 3000, 4000, 5000],
        [0, 1000, 2000, 3000, 4000, 4000, 4000],
    ] {
        let bytes = sample_grid_bytes(&starts, 30000);
        let ranges = std::iter::once(0..FrameRate::Fps30.ticks_per_frame()).collect();
        assert!(inspect_selected(&bytes, ranges, false).is_err());
    }
}
