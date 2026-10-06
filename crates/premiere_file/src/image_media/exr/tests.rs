use super::*;
use crate::{
    image_media::{inspect_export_image_media, inspect_image_media, ImageFormat},
    media::MediaFacts,
};
use std::io::Cursor;

const EXR: &[u8] =
    include_bytes!("../../../../aftereffects_file/tests/fixtures/media_native_panel/media/red.exr");

fn native(bytes: &[u8]) -> ValidatedImage {
    match inspect(Cursor::new(bytes), Some(bytes.len() as u64)).unwrap() {
        Inspection::Native(image) => image,
        Inspection::PremiereUnsupported { .. } => panic!("expected Premiere-compatible OpenEXR"),
    }
}

fn half_rgba() -> Vec<u8> {
    half_rgba_with_metadata(true, false)
}

fn half_rgba_with_metadata(pixel_aspect: bool, comments: bool) -> Vec<u8> {
    fn attribute(bytes: &mut Vec<u8>, name: &[u8], kind: &[u8], value: &[u8]) {
        bytes.extend_from_slice(name);
        bytes.push(0);
        bytes.extend_from_slice(kind);
        bytes.push(0);
        bytes.extend_from_slice(&(value.len() as u32).to_le_bytes());
        bytes.extend_from_slice(value);
    }
    let mut bytes = vec![0x76, 0x2f, 0x31, 0x01, 2, 0, 0, 0];
    let mut channels = Vec::new();
    for name in b"ABGR" {
        channels.extend_from_slice(&[*name, 0]);
        channels.extend_from_slice(&1_u32.to_le_bytes());
        channels.extend_from_slice(&[0; 4]);
        channels.extend_from_slice(&1_u32.to_le_bytes());
        channels.extend_from_slice(&1_u32.to_le_bytes());
    }
    channels.push(0);
    attribute(&mut bytes, b"channels", b"chlist", &channels);
    attribute(&mut bytes, b"compression", b"compression", &[0]);
    let window = [0_i32, 0, 1, 0]
        .into_iter()
        .flat_map(i32::to_le_bytes)
        .collect::<Vec<_>>();
    attribute(&mut bytes, b"dataWindow", b"box2i", &window);
    attribute(&mut bytes, b"displayWindow", b"box2i", &window);
    attribute(&mut bytes, b"lineOrder", b"lineOrder", &[0]);
    if pixel_aspect {
        attribute(
            &mut bytes,
            b"pixelAspectRatio",
            b"float",
            &1_f32.to_le_bytes(),
        );
    } else {
        attribute(&mut bytes, b"writer", b"string", b"lavc");
        attribute(
            &mut bytes,
            b"framesPerSecond",
            b"rational",
            &[25, 0, 0, 0, 1, 0, 0, 0],
        );
        attribute(&mut bytes, b"gamma", b"float", &1_f32.to_le_bytes());
        attribute(
            &mut bytes,
            b"pixelAspectRatioRational",
            b"rational",
            &[4, 0, 0, 0, 3, 0, 0, 0],
        );
    }
    if comments {
        attribute(
            &mut bytes,
            b"comments",
            b"string",
            b"shoe photo retained for editable recovery",
        );
    }
    attribute(&mut bytes, b"screenWindowCenter", b"v2f", &[0; 8]);
    attribute(
        &mut bytes,
        b"screenWindowWidth",
        b"float",
        &1_f32.to_le_bytes(),
    );
    bytes.push(0);
    let chunk = bytes.len() as u64 + 8;
    bytes.extend_from_slice(&chunk.to_le_bytes());
    bytes.extend_from_slice(&0_i32.to_le_bytes());
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    // Planar HALF samples A=.5, B=0, G=0, R=1; two pixels each.
    for sample in [0x3800_u16, 0, 0, 0x3c00] {
        for _ in 0..2 {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
    }
    bytes
}

fn display_window(bytes: &mut [u8], values: [i32; 4]) {
    let name = b"displayWindow\0box2i\0";
    let start = bytes
        .windows(name.len())
        .position(|window| window == name)
        .unwrap()
        + name.len()
        + 4;
    for (index, value) in values.into_iter().enumerate() {
        bytes[start + index * 4..start + (index + 1) * 4].copy_from_slice(&value.to_le_bytes());
    }
}

#[test]
fn native_rgb_openexr_is_an_editable_premiere_still() {
    assert_eq!(
        crate::hash::hash_reader(Cursor::new(EXR)).unwrap(),
        "78ee9059289beb78da9e87bc21196b176646f67bb82b39d8cf1396605e78fa56"
    );
    let facts = inspect_export_image_media(Cursor::new(EXR), EXR.len() as u64).unwrap();
    let MediaFacts::Still(image) = facts else {
        panic!("OpenEXR must be native still media")
    };
    assert_eq!(image.format, ImageFormat::OpenExr);
    assert_eq!((image.width, image.height), (64, 48));
    assert!(!image.alpha);
    assert!(image.pixel_aspect.is_square());
    assert_eq!(
        image.open_exr_channels,
        Some(crate::schema::OpenExrChannels::Rgb {
            red: true,
            green: true,
            blue: true,
        })
    );
    assert!(crate::audio_media::PictureClock::of(&MediaFacts::Still(image)).is_none());
    assert_eq!(inspect_image_media(Cursor::new(EXR)).unwrap(), image);
}

#[test]
fn rgba_openexr_retains_float_alpha_and_ancillary_metadata() {
    let bytes = half_rgba_with_metadata(false, true);
    let image = native(&bytes);
    assert_eq!(image.format, ImageFormat::OpenExr);
    assert_eq!((image.width, image.height), (2, 1));
    assert!(image.alpha);
    assert_eq!(image.pixel_aspect.terms(), (4, 3));
    assert_eq!(
        image.open_exr_channels,
        Some(crate::schema::OpenExrChannels::Rgb {
            red: true,
            green: true,
            blue: true,
        })
    );

    let decoded = image::ImageReader::with_format(Cursor::new(&bytes), image::ImageFormat::OpenExr)
        .decode()
        .unwrap()
        .to_rgba32f();
    assert_eq!(decoded.get_pixel(0, 0).0, [1.0, 0.0, 0.0, 0.5]);
}

#[test]
fn differing_and_large_display_windows_do_not_trigger_pixel_allocation_guards() {
    let mut bytes = half_rgba();
    display_window(&mut bytes, [-25, -10, 100_000_000, 20]);
    let image = native(&bytes);
    assert_eq!((image.width, image.height), (100_000_026, 31));
    assert!(image.alpha);
}

#[test]
fn damaged_chunk_offset_table_recovers_from_sequential_chunks() {
    let mut bytes = half_rgba();
    let offset_table = bytes.len() - 8 - 8 - 16;
    bytes[offset_table..offset_table + 8].copy_from_slice(&u64::MAX.to_le_bytes());
    let image = native(&bytes);
    assert_eq!((image.width, image.height), (2, 1));
    assert!(image.alpha);
}

#[test]
fn only_deep_tiled_parts_use_the_editable_fallback() {
    let reader = Reader::read_from_buffered(Cursor::new(EXR), false).unwrap();
    let mut header = reader.headers()[0].clone();
    header.deep = true;
    header.blocks = BlockDescription::Tiles(exr::meta::attribute::TileDescription {
        tile_size: exr::math::Vec2(64, 48),
        level_mode: exr::meta::attribute::LevelMode::Singular,
        rounding_mode: exr::math::RoundingMode::Down,
    });
    assert!(!premiere_compatible(&header));

    let mut later = reader.headers()[0].clone();
    later.own_attributes.layer_name = Some(exr::meta::attribute::Text::from("beauty"));
    let prefixed = image_facts(&[(1, &later)], &header, 2).unwrap();
    assert!(!prefixed.alpha);
    assert_eq!(
        prefixed.open_exr_channels,
        Some(crate::schema::OpenExrChannels::Rgb {
            red: false,
            green: false,
            blue: false,
        })
    );
    later.own_attributes.layer_name = None;
    let unprefixed = image_facts(&[(1, &later)], &header, 2).unwrap();
    assert!(!unprefixed.alpha);
    assert_eq!(
        unprefixed.open_exr_channels,
        Some(crate::schema::OpenExrChannels::Rgb {
            red: true,
            green: true,
            blue: true,
        })
    );

    header.deep = false;
    assert!(premiere_compatible(&header));
}

#[test]
fn truncated_or_invalid_structure_is_not_published_as_a_valid_source() {
    let bytes = half_rgba();
    let mut bad_version = bytes.clone();
    bad_version[4..8].copy_from_slice(&0x1002_u32.to_le_bytes());
    let mut bad_chunk_size = bytes.clone();
    let chunk_size = bad_chunk_size.len() - 16 - 4;
    bad_chunk_size[chunk_size..chunk_size + 4].copy_from_slice(&u32::MAX.to_le_bytes());

    for broken in [
        bad_version,
        bad_chunk_size,
        bytes[..bytes.len() - 1].to_vec(),
    ] {
        assert!(inspect(Cursor::new(&broken), Some(broken.len() as u64)).is_err());
    }
    assert!(inspect(Cursor::new(&bytes), Some(bytes.len() as u64 + 1)).is_err());
}
