//! Supplemental header mutations of the pinned Adobe-derived video-format case.
//! Encoded picture samples, packet offsets and source clocks remain unchanged.
use super::*;

fn configuration(bytes: &[u8]) -> (usize, usize, usize) {
    let start = bytes.windows(4).position(|kind| kind == b"avcC").unwrap() + 4;
    let size = u32::from_be_bytes(bytes[start - 8..start - 4].try_into().unwrap());
    let end = start - 8 + usize::try_from(size).unwrap();
    let mut cursor = start + 6;
    for _ in 0..bytes[start + 5] & 0x1f {
        let length = usize::from(u16::from_be_bytes([bytes[cursor], bytes[cursor + 1]]));
        cursor += 2 + length;
    }
    let pps = cursor;
    cursor += 1;
    for _ in 0..bytes[pps] {
        let length = usize::from(u16::from_be_bytes([bytes[cursor], bytes[cursor + 1]]));
        cursor += 2 + length;
    }
    assert!(cursor <= end);
    (start, pps, cursor)
}

fn assert_editable_videos(root: &Path, expected_h264: &[u8]) {
    let archive = convert(&root.join(PROJECT), Some(SEQUENCE), &root.join("import"));
    assert_eq!(video_layers(&archive), expected_layers());
    assert_eq!(archive.metadata().assets.len(), 2);
    let document = archive.project_json().unwrap();
    for layer in document["composition"]["layers"].as_array().unwrap() {
        if layer["type"] != "Video" {
            continue;
        }
        assert_eq!(layer["transform"]["opacity"].as_f64(), Some(100.0));
        let id = layer["source"]["assetId"].as_str().unwrap();
        let name = asset_name(&archive, id);
        let expected = if name == H264 {
            expected_h264.to_vec()
        } else {
            assert_eq!(name, HEVC);
            fs::read(fixture_path(HEVC)).unwrap()
        };
        let packaged = archive
            .asset(id)
            .unwrap()
            .read_verified_bytes(expected.len() as u64)
            .unwrap();
        assert_eq!(packaged, expected, "{name}");
    }
}

#[test]
fn avcc_reserved_header_bits_keep_native_editable_videos_and_original_bytes() {
    let original = fs::read(fixture_path(H264)).unwrap();
    let (start, _, _) = configuration(&original);
    for (length_reserved, sps_reserved) in
        [(0xfc, 0), (0, 0xe0), (0, 0), (0x80, 0x20), (0x24, 0x40)]
    {
        let directory = tempfile::tempdir().unwrap();
        stage(directory.path());
        let mut bytes = original.clone();
        bytes[start + 4] = length_reserved | (bytes[start + 4] & 3);
        bytes[start + 5] = sps_reserved | (bytes[start + 5] & 0x1f);
        fs::write(directory.path().join(H264), &bytes).unwrap();
        assert_editable_videos(directory.path(), &bytes);
    }
}

#[test]
fn avcc_reserved_extension_bits_keep_native_editable_videos_and_original_bytes() {
    let original = fs::read(fixture_path(H264)).unwrap();
    let (start, _, extension) = configuration(&original);
    assert_eq!(original[start + 1], 100);
    for reserved in [0, 0x40, 0xa0, 0xf8] {
        let directory = tempfile::tempdir().unwrap();
        stage(directory.path());
        let mut bytes = original.clone();
        bytes[extension] = (reserved & 0xfc) | (bytes[extension] & 3);
        bytes[extension + 1] = reserved | (bytes[extension + 1] & 7);
        bytes[extension + 2] = reserved | (bytes[extension + 2] & 7);
        fs::write(directory.path().join(H264), &bytes).unwrap();
        assert_editable_videos(directory.path(), &bytes);
    }
}

#[test]
fn avcc_required_framing_and_parameter_sets_still_reject_without_publication() {
    let original = fs::read(fixture_path(H264)).unwrap();
    let (start, pps, _) = configuration(&original);
    for defect in [
        "version",
        "NAL length",
        "SPS bounds",
        "PPS bounds",
        "empty SPS",
        "empty PPS",
    ] {
        let directory = tempfile::tempdir().unwrap();
        stage(directory.path());
        let mut bytes = original.clone();
        match defect {
            "version" => bytes[start] = 0,
            "NAL length" => bytes[start + 4] = (bytes[start + 4] & 0xfc) | 2,
            "SPS bounds" => bytes[start + 6..start + 8].copy_from_slice(&u16::MAX.to_be_bytes()),
            "PPS bounds" => bytes[pps + 1..pps + 3].copy_from_slice(&u16::MAX.to_be_bytes()),
            "empty SPS" => bytes[start + 6..start + 8].fill(0),
            "empty PPS" => bytes[pps + 1..pps + 3].fill(0),
            _ => unreachable!(),
        }
        fs::write(directory.path().join(H264), bytes).unwrap();
        for check in [true, false] {
            let output = directory.path().join(format!("invalid-{check}"));
            let error = premiere_to_tesseract(
                directory.path().join(PROJECT),
                &output,
                Some(SEQUENCE),
                check,
            )
            .unwrap_err();
            let message = error.to_string();
            assert!(
                message.contains("H.264 decoder configuration")
                    || message.contains("H.264 NAL length size")
                    || message.contains("empty H.264 sequence parameter set")
                    || message.contains("empty H.264 picture parameter set"),
                "{defect}: {message}"
            );
            assert!(!output.exists(), "{defect}");
        }
    }
}
