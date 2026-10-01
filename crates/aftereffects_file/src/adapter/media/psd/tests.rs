use super::{DecodeError, Selection, decode, plane};

const FIXTURE: &[u8] = include_bytes!("../../../../tests/fixtures/psd_import/two_layers_v2.psd");

#[test]
fn native_layer_pixels_crop_and_canvas() {
    let red = Selection::Layer { id: 101, index: 0 };
    let crop = decode(FIXTURE, red, [20, 16]).unwrap();
    assert_eq!(crop.get_pixel(0, 0).0, [255, 32, 16, 255]);
    assert_eq!(crop.get_pixel(19, 15).0, [255, 32, 16, 255]);
    let canvas = decode(FIXTURE, red, [64, 48]).unwrap();
    assert_eq!(canvas.get_pixel(9, 8).0, [0, 0, 0, 0]);
    assert_eq!(canvas.get_pixel(10, 8).0, [255, 32, 16, 255]);
    assert_eq!(canvas.get_pixel(29, 23).0, [255, 32, 16, 255]);
    assert_eq!(canvas.get_pixel(30, 23).0, [0, 0, 0, 0]);

    let blue = Selection::Layer { id: 202, index: 1 };
    let crop = decode(FIXTURE, blue, [32, 20]).unwrap();
    assert_eq!(crop.get_pixel(0, 0).0, [16, 64, 255, 128]);
    let canvas = decode(FIXTURE, blue, [64, 48]).unwrap();
    assert_eq!(canvas.get_pixel(23, 20).0, [0, 0, 0, 0]);
    assert_eq!(canvas.get_pixel(24, 20).0, [16, 64, 255, 128]);
    assert_eq!(canvas.get_pixel(55, 39).0, [16, 64, 255, 128]);
    assert_eq!(canvas.get_pixel(56, 39).0, [0, 0, 0, 0]);
}

#[test]
fn native_merged_pixels_are_stored_composite_not_layer_recomposition() {
    let merged = decode(FIXTURE, Selection::Merged, [64, 48]).unwrap();
    assert_eq!(merged.get_pixel(0, 0).0, [0, 0, 0, 0]);
    assert_eq!(merged.get_pixel(10, 8).0, [255, 32, 16, 255]);
    assert_eq!(merged.get_pixel(26, 22).0, [135, 48, 136, 255]);
    assert_eq!(merged.get_pixel(45, 30).0, [16, 64, 255, 128]);
}

#[test]
fn layer_identity_is_not_guessed_by_position_or_name() {
    assert!(matches!(
        decode(FIXTURE, Selection::Layer { id: 202, index: 0 }, [20, 16]),
        Err(DecodeError::Unsupported(_))
    ));
    assert!(matches!(
        decode(FIXTURE, Selection::Layer { id: 999, index: 0 }, [20, 16]),
        Err(DecodeError::Unsupported(_))
    ));
    assert!(matches!(
        decode(FIXTURE, Selection::Layer { id: 101, index: 2 }, [20, 16]),
        Err(DecodeError::Unsupported(_))
    ));
    // Second lyid is at native fixture offset 210; duplicate IDs are invalid.
    let mut duplicate = FIXTURE.to_vec();
    let position = duplicate
        .windows(8)
        .position(|v| v == b"lyid\0\0\0\x04")
        .unwrap();
    let next = duplicate[position + 8..]
        .windows(8)
        .position(|v| v == b"lyid\0\0\0\x04")
        .unwrap()
        + position
        + 8;
    duplicate[next + 8..next + 12].copy_from_slice(&101u32.to_be_bytes());
    assert!(matches!(
        decode(&duplicate, Selection::Layer { id: 101, index: 0 }, [20, 16]),
        Err(DecodeError::Malformed("duplicate layer IDs"))
    ));
}

#[test]
fn psd_unsupported_sibling_geometry_does_not_omit_selected_raster() {
    // Supplementary mutations of only Red's bounds: an empty rectangle or
    // an off-canvas rectangle. Blue's identity/channels remain untouched.
    for bounds in [[0_i32, 0, 0, 0], [-8, 10, 24, 30]] {
        let mut bytes = FIXTURE.to_vec();
        for (field, value) in bytes[44..60].chunks_exact_mut(4).zip(bounds) {
            field.copy_from_slice(&value.to_be_bytes());
        }
        let blue = decode(&bytes, Selection::Layer { id: 202, index: 1 }, [32, 20]).unwrap();
        assert!(blue.pixels().all(|pixel| pixel.0 == [16, 64, 255, 128]));
        assert!(decode(&bytes, Selection::Layer { id: 101, index: 0 }, [20, 16]).is_err());
    }
}

#[test]
fn rejects_truncation_and_hostile_dimensions_before_allocating() {
    for end in [0, 3, 25, 38, 100, 220, 4000] {
        assert!(
            decode(
                &FIXTURE[..end],
                Selection::Layer { id: 101, index: 0 },
                [20, 16]
            )
            .is_err(),
            "truncation at {end}"
        );
    }
    assert!(decode(&FIXTURE[..FIXTURE.len() - 1], Selection::Merged, [64, 48]).is_err());
    let mut huge = FIXTURE.to_vec();
    huge[14..18].copy_from_slice(&u32::MAX.to_be_bytes());
    assert!(matches!(
        decode(&huge, Selection::Merged, [64, 48]),
        Err(DecodeError::Bounds(_))
    ));
    let mut profile = FIXTURE.to_vec();
    profile[24..26].copy_from_slice(&4u16.to_be_bytes());
    assert!(matches!(
        decode(&profile, Selection::Merged, [64, 48]),
        Err(DecodeError::Unsupported(_))
    ));
    assert!(matches!(
        decode(FIXTURE, Selection::Layer { id: 101, index: 0 }, [19, 16]),
        Err(DecodeError::Unsupported(_))
    ));
}

fn with_resource(id: u16, payload: &[u8]) -> Vec<u8> {
    let mut resource = b"8BIM".to_vec();
    resource.extend(id.to_be_bytes());
    resource.extend([0, 0]); // Empty even-padded Pascal name.
    resource.extend(u32::try_from(payload.len()).unwrap().to_be_bytes());
    resource.extend(payload);
    if !payload.len().is_multiple_of(2) {
        resource.push(0);
    }
    let mut bytes = FIXTURE[..30].to_vec();
    bytes.extend(u32::try_from(resource.len()).unwrap().to_be_bytes());
    bytes.extend(resource);
    bytes.extend(&FIXTURE[34..]);
    bytes
}

#[test]
fn psd_resources_are_bounded_and_absent_merged_preview_is_not_substituted() {
    // Supplemental metadata cases; the adapter diagnoses omitted color management.
    let bytes = with_resource(1039, b"supplementary profile payload");
    assert_eq!(
        decode(&bytes, Selection::Merged, [64, 48]).unwrap(),
        decode(FIXTURE, Selection::Merged, [64, 48]).unwrap()
    );
    let no_preview = with_resource(1057, &[0, 0, 0, 1, 0]);
    assert!(matches!(
        decode(&no_preview, Selection::Merged, [64, 48]),
        Err(DecodeError::Unsupported("PSD has no real merged preview"))
    ));
    assert!(
        decode(
            &no_preview,
            Selection::Layer { id: 101, index: 0 },
            [20, 16]
        )
        .is_ok()
    );
    let invalid = with_resource(1057, &[0, 0, 0, 1]);
    assert!(decode(&invalid, Selection::Merged, [64, 48]).is_err());
}

#[test]
fn psd_selected_layer_unsupported_blend_preserves_plain_sibling() {
    let mut bytes = FIXTURE.to_vec();
    let blend = bytes
        .windows(8)
        .position(|value| value == b"8BIMnorm")
        .unwrap()
        + 4;
    bytes[blend..blend + 4].copy_from_slice(b"mul ");
    assert!(matches!(
        decode(&bytes, Selection::Layer { id: 101, index: 0 }, [20, 16]),
        Err(DecodeError::Unsupported(_))
    ));
    assert!(decode(&bytes, Selection::Layer { id: 202, index: 1 }, [32, 20]).is_ok());
}

#[test]
fn psd_rgb_composite_is_opaque_but_cannot_discard_a_transparency_marker() {
    let mut bytes = FIXTURE[..FIXTURE.len() - 64 * 48].to_vec();
    bytes[12..14].copy_from_slice(&3_u16.to_be_bytes());
    assert!(matches!(
        decode(&bytes, Selection::Merged, [64, 48]),
        Err(DecodeError::Malformed(
            "merged transparency channel is absent"
        ))
    ));
    bytes[42..44].copy_from_slice(&2_i16.to_be_bytes());
    let image = decode(&bytes, Selection::Merged, [64, 48]).unwrap();
    assert!(image.pixels().all(|pixel| pixel.0[3] == 255));
}

#[test]
fn psd_composite_packbits_uses_one_table_for_all_channels() {
    let layer_mask_len = u32::from_be_bytes(FIXTURE[34..38].try_into().unwrap()) as usize;
    let start = 38 + layer_mask_len;
    let raw = &FIXTURE[start + 2..];
    let mut encoded = FIXTURE[..start].to_vec();
    encoded.extend(1_u16.to_be_bytes());
    for _ in 0..4 * 48 {
        encoded.extend(65_u16.to_be_bytes());
    }
    for row in raw.chunks_exact(64) {
        encoded.push(63);
        encoded.extend(row);
    }
    assert_eq!(
        decode(&encoded, Selection::Merged, [64, 48]).unwrap(),
        decode(FIXTURE, Selection::Merged, [64, 48]).unwrap()
    );
    encoded.pop();
    assert!(decode(&encoded, Selection::Merged, [64, 48]).is_err());
}

#[test]
fn psd_dimensions_above_the_former_pixel_quota_are_valid() {
    assert_eq!(super::dimensions(4001, 4000).unwrap(), 16_004_000);
    // Enlarge only the canvas; the selected native layer's pixels stay unchanged.
    let mut source = FIXTURE.to_vec();
    source[14..18].copy_from_slice(&4000_u32.to_be_bytes());
    source[18..22].copy_from_slice(&4001_u32.to_be_bytes());
    let image = decode(&source, Selection::Layer { id: 101, index: 0 }, [20, 16]).unwrap();
    assert!(image.pixels().all(|pixel| pixel.0 == [255, 32, 16, 255]));
    assert!(super::dimensions(30_001, 1).is_err()); // PSD v1 wire constraint.
}

#[test]
fn psd_layer_inventory_has_no_arbitrary_record_or_channel_quota() {
    // Inventory parsing must not reject a selected raster because other records
    // have many channels. Selected-channel semantics are checked separately.
    for (count, channels) in [(4097_i16, 0_u16), (1, 9), (i16::MIN, 0)] {
        let mut info = count.to_be_bytes().to_vec();
        for _ in 0..count.unsigned_abs() {
            info.extend([0_u8; 16]); // Empty sibling rectangle.
            info.extend(channels.to_be_bytes());
            for id in 0..channels {
                info.extend(i16::try_from(id).unwrap().to_be_bytes());
                info.extend(2_u32.to_be_bytes()); // Raw, empty channel.
            }
            info.extend(b"8BIMnorm");
            info.extend([255, 0, 0, 0]);
            info.extend(12_u32.to_be_bytes());
            info.extend([0_u8; 12]); // Mask, blend ranges and padded name.
        }
        info.extend(vec![
            0;
            usize::from(count.unsigned_abs())
                * usize::from(channels)
                * 2
        ]);
        let mut section = u32::try_from(info.len()).unwrap().to_be_bytes().to_vec();
        section.extend(info);
        let layers = super::read_layers(&mut super::Cursor::new(&section)).unwrap();
        assert_eq!(layers.len(), usize::from(count.unsigned_abs()));
        assert_eq!(layers[0].channels.len(), usize::from(channels));
    }
}

#[test]
fn packbits_row_boundaries_and_exact_lengths() {
    // Two rows of width 4: literal 1,2; repeat 9,9; then a no-op and literal 3,4,5,6.
    let encoded = [0, 1, 0, 5, 0, 6, 1, 1, 2, 255, 9, 128, 3, 3, 4, 5, 6];
    assert_eq!(plane(&encoded, 4, 2).unwrap(), [1, 2, 9, 9, 3, 4, 5, 6]);
    for bad in [
        &encoded[..encoded.len() - 1],
        &[0, 1, 0, 2, 0, 0, 3, 1][..],   // literal needs four bytes
        &[0, 1, 0, 2, 0, 0, 251, 9][..], // repeat 6 exceeds row
        &[0, 1, 0, 2, 0, 0, 255, 9][..], // row decodes short
        &[0, 0, 1, 2][..],               // raw length mismatch
        &[0, 2, 0, 0][..],               // unsupported ZIP
    ] {
        assert!(
            plane(bad, 4, 2).is_err(),
            "accepted invalid channel: {bad:?}"
        );
    }
}
