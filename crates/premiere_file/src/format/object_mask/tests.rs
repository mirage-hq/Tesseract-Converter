use super::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use sha2::{Digest, Sha256};

const INITIAL: &[u8] = include_bytes!("../../../tests/fixtures/object_mask/initial.prmf");
const PROPAGATION: &[u8] =
    include_bytes!("../../../tests/fixtures/object_mask/propagation-first-two.prmf");
const OPACITY: &str = include_str!("../../../tests/fixtures/object_mask/opacity.xml");
const CANVAS: [u32; 2] = [1280, 720];
const CADENCE: u64 = 8_511_237_907;

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn tracker_bytes() -> Vec<u8> {
    let xml = format!("<Root>{OPACITY}</Root>");
    let doc = roxmltree::Document::parse(&xml).unwrap();
    let tracker = doc
        .descendants()
        .find(|node| node.attribute("ObjectID") == Some("1237"))
        .unwrap();
    let value = tracker
        .children()
        .find(|node| node.has_tag_name("StartKeyframeValue"))
        .unwrap();
    STANDARD.decode(value.text().unwrap().trim()).unwrap()
}

fn assert_pixels(
    frame: &Frame<'_>,
    expected_crop: [u32; 4],
    expected_hash: &str,
    zeros: usize,
    full: usize,
) {
    assert_eq!(frame.crop, expected_crop);
    let pixels = frame.coverage().unwrap();
    assert_eq!(pixels.len(), 1280 * 720);
    let [x, y, width, height] = frame.crop.map(|value| value as usize);
    let mut crop = Vec::new();
    for row in 0..720 {
        for column in 0..1280 {
            let value = pixels[row * 1280 + column];
            if (y..y + height).contains(&row) && (x..x + width).contains(&column) {
                crop.push(value);
            } else {
                assert_eq!(value, 0, "outside native crop at {column},{row}");
            }
        }
    }
    // This hash covers every reconstructed row, including all compression tile
    // boundaries. Resetting the vertical predictor per tile does not pass.
    assert_eq!(digest(&crop), expected_hash);
    assert_eq!(crop.iter().filter(|&&pixel| pixel == 0).count(), zeros);
    assert_eq!(crop.iter().filter(|&&pixel| pixel == 255).count(), full);
    assert!(crop.iter().any(|&pixel| pixel > 0 && pixel < 255));
}

#[test]
fn object_mask_native_initial_pixels_and_source_bounds() {
    assert_eq!(INITIAL.len(), 18_512);
    assert_eq!(
        digest(INITIAL),
        "91a9d2d90d8bf7b0f88f313810de6c87f7e360afbb51355f12a5a210aac3498c"
    );
    let raster = Raster::decode_index(INITIAL, CANVAS, CADENCE, 1).unwrap();
    assert_eq!(raster.frames[0].timestamp_ticks, 0);
    assert_pixels(
        &raster.frames[0],
        [247, 50, 795, 670],
        "6a506361abf33a86c802a3ae08df0338df67ce73d7a534fb0cb3669a4885f303",
        229_397,
        286_001,
    );
    assert!(Raster::decode_index(INITIAL, [1920, 1080], CADENCE, 1).is_err());
    assert!(Raster::decode_index(INITIAL, [u32::MAX, u32::MAX], CADENCE, 1).is_err());
    assert!(Raster::decode_index(INITIAL, CANVAS, CADENCE, MAX_FRAMES as usize + 1).is_err());
}

#[test]
fn object_mask_native_propagation_excerpt_preserves_pixels_and_exact_clock() {
    assert_eq!(PROPAGATION.len(), 34_032);
    assert_eq!(
        digest(PROPAGATION),
        "5065e52b3ed94e57374a2918cef413ab606d180513678cd02bb84481ba9a3994"
    );
    let raster = Raster::decode_index(PROPAGATION, CANVAS, CADENCE, 2).unwrap();
    assert_eq!(
        raster
            .frames
            .iter()
            .map(|frame| frame.timestamp_ticks)
            .collect::<Vec<_>>(),
        [0, CADENCE]
    );
    assert_pixels(
        &raster.frames[0],
        [284, 50, 768, 670],
        "46812a580af45967d3ce44b2a7083affcc35f20a54fd4ab78c73de0174420312",
        210_787,
        285_826,
    );
    assert_pixels(
        &raster.frames[1],
        [280, 51, 759, 669],
        "0f18966209b2f6bfaafa7eba81148f1ca911629a02cc7e4d7749156e84e5847b",
        205_516,
        284_061,
    );
    assert!(Raster::decode_index(PROPAGATION, CANVAS, CADENCE + 1, 2).is_err());
    assert!(Raster::decode_index(PROPAGATION, CANVAS, CADENCE, 204).is_err());
}

#[test]
fn object_mask_tracker_resolves_only_referenced_propagation_and_checks_source() {
    let bytes = tracker_bytes();
    assert_eq!(bytes.len(), 304);
    let tracker = Tracker::decode(&bytes).unwrap();
    assert_eq!(
        tracker.initial.to_string(),
        "8abda722-8116-46fc-b431-9aeab7d80730"
    );
    assert_eq!(
        tracker.propagation.to_string(),
        "dd06d550-fb83-4fb0-b9e8-6d3d6fcdedf1"
    );
    assert_eq!(tracker.frame_ticks, CADENCE);
    assert_eq!(tracker.frame_count, 204);
    let source_rate = SourceFrameRate::from_ticks_per_frame(CADENCE as i64).unwrap();
    tracker
        .validate_source(source_rate, 1_736_292_533_028)
        .unwrap();
    assert!(tracker
        .validate_source(source_rate, 1_736_292_533_029)
        .is_err());
    assert!(tracker
        .validate_source(crate::schema::FrameRate::Fps30.into(), 1_736_292_533_028)
        .is_err());
    assert!(
        tracker.propagation(PROPAGATION, CANVAS).is_err(),
        "204-frame Tracker must not accept a two-frame excerpt"
    );

    let directory = tempfile::tempdir().unwrap();
    let decoy = directory
        .path()
        .join("25490698-2453-4e9d-a7d3-42bc54742782.prmf");
    std::fs::write(&decoy, INITIAL).unwrap();
    let resolved = tracker.propagation_path(directory.path());
    assert_eq!(
        resolved.file_name().unwrap(),
        "dd06d550-fb83-4fb0-b9e8-6d3d6fcdedf1.prmf"
    );
    assert!(
        !resolved.exists(),
        "a live unreferenced sidecar cannot satisfy the reference"
    );
    std::fs::write(&resolved, PROPAGATION).unwrap();
    // Supplementary count mutation for the explicitly bounded excerpt only.
    let mut excerpt_tracker = bytes;
    excerpt_tracker[228..232].copy_from_slice(&2u32.to_le_bytes());
    let excerpt_tracker = Tracker::decode(&excerpt_tracker).unwrap();
    let loaded = std::fs::read(excerpt_tracker.propagation_path(directory.path())).unwrap();
    assert_eq!(
        excerpt_tracker
            .propagation(&loaded, CANVAS)
            .unwrap()
            .frames
            .len(),
        2
    );
}

#[test]
fn object_mask_tracker_rejects_unknown_fields_bad_references_and_counts() {
    let original = tracker_bytes();
    // Native offsets are pinned by the unchanged Tracker fixture. It includes
    // a negative signed vtable displacement to a forward shared vtable.
    for (at, replacement) in [
        (59, vec![9]),                       // auxiliary type
        (192, 2u32.to_le_bytes().to_vec()),  // two propagations
        (192, 0u32.to_le_bytes().to_vec()),  // no propagation
        (212, 24u16.to_le_bytes().to_vec()), // previously absent slot 3
        (228, 0u32.to_le_bytes().to_vec()),  // zero count
        (228, 100_001u32.to_le_bytes().to_vec()),
        (232, (CADENCE + 1).to_le_bytes().to_vec()),
        (260, b"../x".to_vec()),                // reference is not a UUID
        (144, u32::MAX.to_le_bytes().to_vec()), // reference points outside buffer
    ] {
        let mut bytes = original.clone();
        bytes[at..at + replacement.len()].copy_from_slice(&replacement);
        assert!(Tracker::decode(&bytes).is_err(), "mutation at {at}");
    }
    for end in [0, 3, 20, 140, 252, 299] {
        assert!(
            Tracker::decode(&original[..end]).is_err(),
            "truncated at {end}"
        );
    }
}

fn u32_at(bytes: &[u8], at: usize) -> usize {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize
}

fn table_field(bytes: &[u8], table: usize, slot: usize) -> usize {
    let delta = i32::from_le_bytes(bytes[table..table + 4].try_into().unwrap());
    let vtable = (table as i64 - i64::from(delta)) as usize;
    table
        + usize::from(u16::from_le_bytes(
            bytes[vtable + 4 + slot * 2..vtable + 6 + slot * 2]
                .try_into()
                .unwrap(),
        ))
}

#[test]
fn object_mask_prmf_rejects_malformed_index_crop_payload_and_truncation() {
    let index = u64::from_le_bytes(INITIAL[8..16].try_into().unwrap()) as usize;
    let root = index + u32_at(INITIAL, index);
    let vector_field = table_field(INITIAL, root, 2);
    let vector = vector_field + u32_at(INITIAL, vector_field);
    let frame_element = vector + 4;
    let frame = frame_element + u32_at(INITIAL, frame_element);
    for (at, replacement) in [
        (0, b"xxxx".to_vec()),
        (4, 4u32.to_le_bytes().to_vec()),
        (8, u64::MAX.to_le_bytes().to_vec()),
        (16, u64::MAX.to_le_bytes().to_vec()),
        (24, 0u64.to_le_bytes().to_vec()),
        (index, u32::MAX.to_le_bytes().to_vec()),
        (vector, 100_001u32.to_le_bytes().to_vec()),
        (frame, i32::MIN.to_le_bytes().to_vec()),
        (
            table_field(INITIAL, frame, 3),
            u32::MAX.to_le_bytes().to_vec(),
        ),
        (
            table_field(INITIAL, frame, 3) + 8,
            0u32.to_le_bytes().to_vec(),
        ),
        (table_field(INITIAL, frame, 2), 0u64.to_le_bytes().to_vec()),
        (
            table_field(INITIAL, frame, 6),
            u32::MAX.to_le_bytes().to_vec(),
        ),
    ] {
        let mut bytes = INITIAL.to_vec();
        bytes[at..at + replacement.len()].copy_from_slice(&replacement);
        assert!(
            Raster::decode_index(&bytes, CANVAS, CADENCE, 1).is_err(),
            "mutation at {at}"
        );
    }
    for end in [0, 31, 32, index, INITIAL.len() - 1] {
        assert!(Raster::decode_index(&INITIAL[..end], CANVAS, CADENCE, 1).is_err());
    }
}

/// A full first tile taken from the native stream, with a one-tile stream
/// header. This exercises TileStream.h's zero-last-size rule with actual data.
fn full_tile_stream() -> Vec<u8> {
    let raster = Raster::decode_index(INITIAL, CANVAS, CADENCE, 1).unwrap();
    let payload = raster.frames[0].payload;
    let tiles = usize::from(u16::from_le_bytes(payload[2..4].try_into().unwrap()));
    let first_length = u32_at(payload, 12);
    let data = 8 + tiles * 4;
    let mut one = vec![4, 0xfb, 1, 0, 1, 0, 0, 0];
    one.extend_from_slice(&(first_length as u32).to_le_bytes());
    one.extend_from_slice(&payload[data..data + first_length]);
    one
}

#[test]
fn object_mask_gdeflate_full_last_tile_and_malformed_streams() {
    let full = full_tile_stream();
    gdeflate::decode(&full, &mut vec![0; 65_536]).unwrap();
    for (at, replacement) in [
        (0, vec![5]),
        (1, vec![0]),
        (2, 0u16.to_le_bytes().to_vec()),
        (2, u16::MAX.to_le_bytes().to_vec()),
        (4, 0u32.to_le_bytes().to_vec()),
        (4, 0x10_0001u32.to_le_bytes().to_vec()),
        (4, ((65_537u32 << 2) | 1).to_le_bytes().to_vec()),
        (8, u32::MAX.to_le_bytes().to_vec()),
    ] {
        let mut bytes = full.clone();
        bytes[at..at + replacement.len()].copy_from_slice(&replacement);
        assert!(
            gdeflate::decode(&bytes, &mut vec![0; 65_536]).is_err(),
            "mutation at {at}"
        );
    }
    for end in [0, 7, 8, 11, full.len() - 1] {
        assert!(gdeflate::decode(&full[..end], &mut vec![0; 65_536]).is_err());
    }
    let mut bad_compressed = vec![4, 0xfb, 1, 0, 1, 0, 0, 0, 4, 0, 0, 0];
    bad_compressed.extend_from_slice(&[0; 4]);
    let error = gdeflate::decode(&bad_compressed, &mut vec![0; 65_536]).unwrap_err();
    assert!(error.to_string().contains("tile failed"), "{error}");
    // Header promises a short crop, but the real compressed tile expands to
    // 64 KiB: the C decoder's status/actual-count gate must reject it.
    let mut short = full;
    short[4..8].copy_from_slice(&5u32.to_le_bytes());
    let error = gdeflate::decode(&short, &mut [0]).unwrap_err();
    assert!(error.to_string().contains("tile failed"), "{error}");
}

#[test]
fn object_mask_index_order_is_not_time_order_and_payloads_cannot_overlap() {
    let index = u64::from_le_bytes(PROPAGATION[8..16].try_into().unwrap()) as usize;
    let root = index + u32_at(PROPAGATION, index);
    let vector_field = table_field(PROPAGATION, root, 2);
    let vector = vector_field + u32_at(PROPAGATION, vector_field);
    let elements = [vector + 4, vector + 8];
    let frames = elements.map(|at| at + u32_at(PROPAGATION, at));
    let mut reversed = PROPAGATION.to_vec();
    for (at, target) in elements.into_iter().zip(frames.into_iter().rev()) {
        reversed[at..at + 4].copy_from_slice(&((target - at) as u32).to_le_bytes());
    }
    let raster = Raster::decode_index(&reversed, CANVAS, CADENCE, 2).unwrap();
    assert_eq!(raster.frames[0].timestamp_ticks, 0);
    assert_eq!(raster.frames[1].timestamp_ticks, CADENCE);
    assert_eq!(raster.frames[0].crop, [284, 50, 768, 670]);
    let mut overlap = PROPAGATION.to_vec();
    let offset = table_field(PROPAGATION, frames[1], 2);
    overlap[offset..offset + 8].copy_from_slice(&32u64.to_le_bytes());
    let error = Raster::decode_index(&overlap, CANVAS, CADENCE, 2).unwrap_err();
    assert!(error.to_string().contains("payloads overlap"), "{error}");
    let mut duplicate = PROPAGATION.to_vec();
    let timestamp = table_field(PROPAGATION, frames[1], 1);
    duplicate[timestamp..timestamp + 8].copy_from_slice(&0u64.to_le_bytes());
    assert!(Raster::decode_index(&duplicate, CANVAS, CADENCE, 2).is_err());
}

#[test]
fn object_mask_gdeflate_checks_intermediate_offsets_and_successful_short_output() {
    let raster = Raster::decode_index(INITIAL, CANVAS, CADENCE, 1).unwrap();
    let payload = raster.frames[0].payload;
    let crop_bytes = 795 * 670;
    for replacement in [0u32, u32::MAX] {
        let mut corrupt = payload.to_vec();
        corrupt[12..16].copy_from_slice(&replacement.to_le_bytes());
        assert!(gdeflate::decode(&corrupt, &mut vec![0; crop_bytes]).is_err());
    }
    // Actual native partial final tile, but a synthetic header promises one
    // additional output byte. The C API reports success with a short actual
    // count; checking status alone would silently accept a missing pixel.
    let tiles = usize::from(u16::from_le_bytes(payload[2..4].try_into().unwrap()));
    let data_start = 8 + tiles * 4;
    let final_start = u32_at(payload, 8 + (tiles - 1) * 4);
    let final_bytes = u32_at(payload, 8);
    let actual = crop_bytes - (tiles - 1) * 65_536;
    let flags = (((actual + 1) as u32) << 2) | 1;
    let mut short = vec![4, 0xfb, 1, 0];
    short.extend_from_slice(&flags.to_le_bytes());
    short.extend_from_slice(&(final_bytes as u32).to_le_bytes());
    short.extend_from_slice(&payload[data_start + final_start..]);
    let error = gdeflate::decode(&short, &mut vec![0; actual + 1]).unwrap_err();
    assert!(
        error
            .to_string()
            .contains(&format!("status 0, decoded {actual}")),
        "{error}"
    );
}

#[test]
fn object_mask_concurrent_native_decodes_preserve_pixels() {
    std::thread::scope(|scope| {
        for _ in 0..16 {
            scope.spawn(|| {
                let raster = Raster::decode_index(INITIAL, CANVAS, CADENCE, 1).unwrap();
                assert_pixels(
                    &raster.frames[0],
                    [247, 50, 795, 670],
                    "6a506361abf33a86c802a3ae08df0338df67ce73d7a534fb0cb3669a4885f303",
                    229_397,
                    286_001,
                );
            });
        }
    });
}
