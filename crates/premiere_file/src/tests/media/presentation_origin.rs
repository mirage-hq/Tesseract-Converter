//! Supplemental positive-CTTS/zero-edit clock tests; no Adobe render claim.
use super::*;

fn origin_bytes() -> Vec<u8> {
    let bytes = sample_grid_bytes(&[0, 1000, 2000, 2999, 3999, 4999, 5999], 30000);
    let bytes = with_composition_offsets(&bytes, &[1000, 3000, 0, 0, 2000, 0]);
    let bytes = patch_box(&bytes, MDHD, |table| write_u32(table, 16, 6000));
    let bytes = patch_box(&bytes, MVHD, |table| write_u32(table, 16, 6000));
    let bytes = patch_box(&bytes, TKHD, |table| write_u32(table, 20, 6000));
    patch_box(&bytes, ELST, |table| write_u32(table, 8, 6000))
}

fn interior() -> Vec<std::ops::Range<i64>> {
    std::iter::once(0..4 * FrameRate::Fps30.ticks_per_frame()).collect()
}

#[test]
fn presentation_origin_selected_zero_edit_retains_all_packet_identities() {
    let bytes = origin_bytes();
    let file = inspect_selected(&bytes, interior(), false).unwrap();
    assert!(file.timing.partial_timeline);
    let raw =
        media_transcode::inspect::inspect(Cursor::new(&bytes), bytes.len() as u64, true).unwrap();
    let displayed =
        media_transcode::inspect::inspect_presentation(Cursor::new(&bytes), bytes.len() as u64)
            .unwrap();
    assert_eq!(raw.packets.len(), 6);
    assert_eq!(displayed.packets.len(), 6);
    for (raw, displayed) in raw.packets.iter().zip(&displayed.packets) {
        assert_eq!(
            (raw.position, raw.size),
            (displayed.position, displayed.size)
        );
        assert_eq!(raw.pts.unwrap() - displayed.pts.unwrap(), 1000);
        assert_eq!(raw.dts.unwrap() - displayed.dts.unwrap(), 1000);
    }
}

#[test]
fn presentation_origin_non_affine_or_changed_packets_are_rejected() {
    let bytes = origin_bytes();
    let file = inspect_selected(&bytes, interior(), false).unwrap();
    let origin = file.timing.presentation_origin().unwrap();
    let raw =
        media_transcode::inspect::inspect(Cursor::new(&bytes), bytes.len() as u64, true).unwrap();
    let displayed =
        media_transcode::inspect::inspect_presentation(Cursor::new(&bytes), bytes.len() as u64)
            .unwrap();
    for change in 0..5 {
        let mut invalid = displayed.clone();
        match change {
            0 => invalid.packets[1].pts = invalid.packets[1].pts.map(|value| value + 1),
            1 => invalid.packets[1].dts = invalid.packets[1].dts.map(|value| value + 1),
            2 => {
                invalid.packets.pop();
            }
            3 => invalid.packets.swap(1, 2),
            _ => invalid.packets[1].position += 1,
        }
        assert!(
            origin
                .validate_packets(&raw, &invalid, &raw.streams[0], 30000)
                .is_err(),
            "{change}"
        );
    }
}

#[test]
fn presentation_origin_strict_whole_audio_and_unsafe_edits_are_rejected() {
    let bytes = origin_bytes();
    assert!(inspect(&bytes).is_err());
    assert!(inspect_selected(&bytes, interior(), true).is_err());
    for (offset, value) in [(12, 1), (12, u32::MAX), (16, 0), (8, 0)] {
        let bad = patch_box(&bytes, ELST, |table| write_u32(table, offset, value));
        assert!(
            inspect_selected(&bad, interior(), false).is_err(),
            "{offset}/{value}"
        );
    }
    let multiple = patch_box(&bytes, ELST, |table| {
        write_u32(table, 4, 2);
        table.extend([1000_u32, 0, 65536].into_iter().flat_map(u32::to_be_bytes));
    });
    assert!(inspect_selected(&multiple, interior(), false).is_err());
}

#[test]
fn presentation_origin_rounded_mapping_must_stay_inside_real_sample_coverage() {
    let bytes = origin_bytes();
    assert!(inspect_selected(
        &bytes,
        std::iter::once(0..crate::schema::TICKS / 10).collect(),
        false
    )
    .is_ok());
    // Native end4999/30000 lies on the final normalized PTS, but rounding the
    // offset forward makes edited end201 ms exceed physical last PTS199.967 ms.
    let end = 4999 * (crate::schema::TICKS / 30000);
    assert!(inspect_selected(&bytes, std::iter::once(0..end).collect(), false).is_err());
    assert!(inspect_selected(
        &bytes,
        std::iter::once(-1..crate::schema::TICKS / 10).collect(),
        false
    )
    .is_err());
}

#[test]
fn presentation_origin_public_write_uses_diagnosed_editable_source_trim() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("media")).unwrap();
    let bytes = origin_bytes();
    fs::write(root.path().join("media/source.mp4"), &bytes).unwrap();
    let step = FrameRate::Fps30.ticks_per_frame();
    let xml = include_str!("../../../tests/fixtures/one-clip.xml")
        .replace("2540160000000", &(6 * step).to_string())
        .replace("1270080000000", &(4 * step).to_string());
    let input = root.path().join("source.prproj");
    crate::test_support::write_prproj(&input, &xml);
    let output = root.path().join("import");
    let notes = crate::premiere_to_tesseract(&input, &output, Some("sequence-1"), false).unwrap();
    assert!(
        notes
            .iter()
            .any(|note| note.reason.contains("1000/30000") && note.reason.contains("34 ms")),
        "{notes:?}"
    );
    let archive = tesseract_file::TesseractFile::open(output.join("project.tsrct")).unwrap();
    let document = archive.project_json().unwrap();
    let video = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["type"] == "Video")
        .unwrap();
    assert_eq!(
        video["sourceRange"],
        serde_json::json!({"start":34,"duration":133})
    );
    let range = crate::test_support::layer_range(video);
    assert_eq!(range["start"], 0);
    assert_eq!(range["duration"], 133);
    let mapping = &video["playback"]["mapping"];
    assert_eq!(mapping["type"], "linear");
    assert_eq!(mapping["input"]["duration"], mapping["output"]["duration"]);
    assert_eq!(mapping["output"], video["sourceRange"]);
    let asset = video["source"]["assetId"].as_str().unwrap();
    assert_eq!(
        archive
            .asset(asset)
            .unwrap()
            .read_verified_bytes(bytes.len() as u64)
            .unwrap(),
        bytes
    );
    let raw =
        media_transcode::inspect::inspect(Cursor::new(&bytes), bytes.len() as u64, true).unwrap();
    let displayed =
        media_transcode::inspect::inspect_presentation(Cursor::new(&bytes), bytes.len() as u64)
            .unwrap();
    let select = |packets: &[media_transcode::inspect::PacketInfo], millis: i64| {
        packets
            .iter()
            .filter(|packet| packet.pts.unwrap() * 1_000_000 <= (millis * 1000 + 499) * 30000)
            .max_by_key(|packet| packet.pts.unwrap())
            .unwrap()
            .position
    };
    for time in [0, 33, 67, 100] {
        assert_eq!(
            select(&raw.packets, time + 34),
            select(&displayed.packets, time),
            "selected frame at {time}ms"
        );
    }
}
