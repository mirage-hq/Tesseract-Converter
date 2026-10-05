//! Supplemental metadata mutations of the pinned Adobe-derived video-format case.
//! Only descriptive atoms change; no new native rendering or fidelity claim.
use super::*;

fn atom(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let size = u32::try_from(payload.len() + 8).unwrap().to_be_bytes();
    [size.as_slice(), kind, payload].concat()
}

fn append_movie_atoms(original: &[u8], children: &[u8]) -> Vec<u8> {
    let mut offset = 0;
    while &original[offset + 4..offset + 8] != b"moov" {
        let size = u32::from_be_bytes(original[offset..offset + 4].try_into().unwrap());
        offset += usize::try_from(size).unwrap();
    }
    let size = u32::from_be_bytes(original[offset..offset + 4].try_into().unwrap());
    // This fixture's moov follows mdat, so adding tags moves no sample bytes
    // and changes no chunk offsets, packet identities or clocks.
    assert_eq!(offset + usize::try_from(size).unwrap(), original.len());
    let mut bytes = original.to_vec();
    let size = size
        .checked_add(u32::try_from(children.len()).unwrap())
        .unwrap();
    bytes[offset..offset + 4].copy_from_slice(&size.to_be_bytes());
    bytes.extend(children);
    bytes
}

fn descriptive_metadata() -> Vec<Vec<u8>> {
    let handler = atom(b"hdlr", &[&[0; 8][..], b"mdta", &[0; 14]].concat());
    let item = atom(b"\xa9nam", &atom(b"data", b"\0\0\0\x01"));
    let list = atom(b"ilst", &item);
    let mut oversized_list = list.clone();
    let length = u32::from_be_bytes(list[..4].try_into().unwrap()) + 1;
    oversized_list[..4].copy_from_slice(&length.to_be_bytes());
    vec![
        atom(b"meta", &[0; 2]),
        atom(b"meta", &[&handler[..], &list].concat()),
        atom(
            b"udta",
            &atom(b"meta", &[&[0; 4][..], &handler, &list].concat()),
        ),
        atom(b"meta", &[&handler[..], &oversized_list].concat()),
        atom(b"udta", b"uninterpreted descriptive payload"),
    ]
}

fn assert_editable_clips_and_bytes(root: &Path, expected_hevc: &[u8]) {
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
        let expected = if name == HEVC {
            expected_hevc.to_vec()
        } else {
            assert_eq!(name, H264);
            fs::read(fixture_path(H264)).unwrap()
        };
        assert_eq!(
            archive
                .asset(id)
                .unwrap()
                .read_verified_bytes(expected.len() as u64)
                .unwrap(),
            expected,
            "{name}"
        );
    }
}

#[test]
fn optional_movie_metadata_does_not_discard_native_editable_clips() {
    let original = fs::read(fixture_path(HEVC)).unwrap();
    for children in descriptive_metadata() {
        let directory = tempfile::tempdir().unwrap();
        stage(directory.path());
        let bytes = append_movie_atoms(&original, &children);
        fs::write(directory.path().join(HEVC), &bytes).unwrap();
        assert_editable_clips_and_bytes(directory.path(), &bytes);
    }
}

#[test]
fn optional_movie_metadata_count_does_not_cap_native_editable_clips() {
    let original = fs::read(fixture_path(HEVC)).unwrap();
    for count in [4100, 8220] {
        let directory = tempfile::tempdir().unwrap();
        stage(directory.path());
        let bytes = append_movie_atoms(&original, &atom(b"free", &[]).repeat(count));
        fs::write(directory.path().join(HEVC), &bytes).unwrap();
        assert_editable_clips_and_bytes(directory.path(), &bytes);
    }
}

#[test]
fn optional_movie_metadata_outer_framing_still_protects_the_movie() {
    let directory = tempfile::tempdir().unwrap();
    stage(directory.path());
    let original = fs::read(fixture_path(HEVC)).unwrap();
    let mut children = atom(b"meta", &[]);
    children[..4].copy_from_slice(&9_u32.to_be_bytes());
    let bytes = append_movie_atoms(&original, &children);
    fs::write(directory.path().join(HEVC), bytes).unwrap();
    for check in [true, false] {
        let output = directory.path().join(format!("invalid-{check}"));
        let error = premiere_to_tesseract(
            directory.path().join(PROJECT),
            &output,
            Some(SEQUENCE),
            check,
        )
        .unwrap_err();
        assert!(error.to_string().contains("exceeds its parent"), "{error}");
        assert!(!output.exists());
    }
}
