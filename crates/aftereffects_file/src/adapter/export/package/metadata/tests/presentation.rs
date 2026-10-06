use super::*;

// A decode-order table whose presentation order is 0, 2, 3, 1, then the tail.
// The full unit-rate edit removes only the common composition-time origin.
#[test]
fn b_frame_presentation_origin_keeps_all_samples_and_exact_native_rate() {
    let movie = presentation_movie(2_000, &[(1, 2_000), (1, 4_000), (2, 1_000), (20, 2_000)]);
    let metadata = quicktime(&movie).unwrap();
    assert_eq!(metadata.dimensions, [1920, 1080]);
    assert_eq!(metadata.duration_millis, 1_000);
    assert_eq!(metadata.frame_rate.integer, 24);
    assert_eq!(metadata.frame_rate.fractional, 0);
}

#[test]
fn presentation_origin_edit_does_not_admit_shifted_or_missing_samples() {
    let valid = [(1, 2_000), (1, 4_000), (2, 1_000), (20, 2_000)];
    for origin in [0, 1_000, 3_000, -1] {
        assert!(quicktime(&presentation_movie(origin, &valid)).is_err());
    }
    for offsets in [
        vec![(1, 2_000), (1, 3_000), (2, 1_000), (20, 2_000)],
        vec![(1, 2_000), (1, 5_000), (2, 1_000), (20, 2_000)],
        vec![(1, 2_000), (1, 4_000), (2, 1_000), (19, 2_000)],
        vec![(1, 2_000), (1, 4_000), (2, 1_000), (21, 2_000)],
        vec![(0, 2_000), (24, 2_000)],
    ] {
        assert!(quicktime(&presentation_movie(2_000, &offsets)).is_err());
    }
}

#[test]
fn composition_offsets_are_checked_even_with_a_zero_origin_edit() {
    let metadata = quicktime(&presentation_movie(0, &[(24, 0)])).unwrap();
    assert_eq!(metadata.duration_millis, 1_000);
    assert!(quicktime(&presentation_movie(0, &[(12, 0), (12, 1_000)])).is_err());
}

#[test]
fn composition_offset_versions_counts_and_duplicates_remain_checked() {
    let timing = Timing {
        sample_count: 24,
        sample_delta: 1_000,
        duration: 24_000,
    };
    for payload in [
        vec![0_u8; 8],
        [0_u32.to_be_bytes(), u32::MAX.to_be_bytes()].concat(),
        [
            1_u32.to_be_bytes(),
            1_u32.to_be_bytes(),
            24_u32.to_be_bytes(),
            0_u32.to_be_bytes(),
        ]
        .concat(),
        [
            0x0100_0000_u32.to_be_bytes(),
            1_u32.to_be_bytes(),
            24_u32.to_be_bytes(),
            0_u32.to_be_bytes(),
        ]
        .concat(),
    ] {
        let atom = Atom {
            kind: *b"ctts",
            payload: &payload,
        };
        assert!(super::super::presentation::origin(&[atom], timing).is_err());
    }
    let payload = [
        0_u32.to_be_bytes(),
        1_u32.to_be_bytes(),
        24_u32.to_be_bytes(),
        0_u32.to_be_bytes(),
    ]
    .concat();
    let atoms = [
        Atom {
            kind: *b"ctts",
            payload: &payload,
        },
        Atom {
            kind: *b"ctts",
            payload: &payload,
        },
    ];
    assert!(super::super::presentation::origin(&atoms, timing).is_err());
}

fn presentation_movie(origin: i32, offsets: &[(u32, u32)]) -> Vec<u8> {
    let mut edits = vec![0_u8; 4];
    edits.extend_from_slice(&1_u32.to_be_bytes());
    edits.extend_from_slice(&1_000_u32.to_be_bytes());
    edits.extend_from_slice(&origin.to_be_bytes());
    edits.extend_from_slice(&0x0001_0000_u32.to_be_bytes());
    let mut composition = vec![0_u8; 4];
    composition.extend_from_slice(&u32::try_from(offsets.len()).unwrap().to_be_bytes());
    for &(count, offset) in offsets {
        composition.extend_from_slice(&count.to_be_bytes());
        composition.extend_from_slice(&offset.to_be_bytes());
    }
    let sample_table = atom(
        *b"stbl",
        &[
            atom(*b"stsd", &video_descriptions()),
            atom(*b"stts", &timing(24, 1_000)),
            atom(*b"ctts", &composition),
        ]
        .concat(),
    );
    let media = atom(
        *b"mdia",
        &[
            atom(*b"mdhd", &media_header(24_000, 24_000)),
            atom(*b"hdlr", &handler(*b"vide")),
            atom(*b"minf", &sample_table),
        ]
        .concat(),
    );
    let track = atom(
        *b"trak",
        &[
            atom(*b"tkhd", &track_header(1_000)),
            atom(*b"edts", &atom(*b"elst", &edits)),
            media,
        ]
        .concat(),
    );
    [
        atom(*b"ftyp", &[b"qt  ".as_slice(), &[0, 0, 0, 0]].concat()),
        atom(
            *b"moov",
            &[atom(*b"mvhd", &movie_header(1_000)), track].concat(),
        ),
    ]
    .concat()
}
