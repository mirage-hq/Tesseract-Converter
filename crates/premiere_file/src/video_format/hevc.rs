//! HEVC decoder configuration (`hvcC`), Dolby Vision configuration
//! (`dvcC`/`dvvC`) and parameter-set checks.
//!
//! `hvc1` stores every parameter set in the sample entry, so these checks see
//! the stream configuration that the decoder uses. The accepted subset is the
//! H.264 one widened by bit depth and colour: single-layer, progressive 8- or
//! 10-bit 4:2:0 pictures with square pixels and limited range, in BT.709 or a
//! colour that [`validate_color`] passes through. A Dolby Vision configuration
//! on the `hvc1` entry (the iPhone form) is accepted only when its base layer is
//! plain HEVC that any player shows without the Dolby metadata: one layer
//! (`el_present_flag` 0, `bl_present_flag` 1) whose
//! `dv_bl_signal_compatibility_id` is not 0 (profile 5's IPT-PQ-c2 picture
//! needs a Dolby Vision decoder). The RPU units in the samples pass through
//! with the bytes.
//!
//! The VUI checks are only as sound as the sequence parameter set fields read
//! before them, so the reader parses only syntax that a captured encoder stream
//! exercises (listed in `src/tests/video_format.rs`). Sub-layer profile or level
//! fields, PCM, predicted reference picture sets, and long-term reference
//! pictures reject: no local encoder produces them, and a misread would shift
//! every later field.
//!
//! The `hvcC` header must declare Main or Main 10 at 8- or 10-bit 4:2:0.
//! Decoders take the chroma format and bit depths from the sequence parameter
//! set, so each set must declare the same values as the header. Each set also supplies what neither
//! the header nor the sample-entry boxes reliably carry: the interlaced-source
//! flag (the header keeps it only when every parameter set sets it), the
//! decoded size, and the VUI pixel aspect, range, color, field coding, and
//! display window. The `colr` and `pasp` boxes are optional: a captured
//! iPhone HEVC sample has neither and declares Display-P3 primaries only in its VUI, which
//! box-only checks would accept as BT.709.

use crate::{
    error::{ensure, unsupported, BuildError, Result},
    media_metadata::{validate_color, ColourDescription, SampleDescription},
};
use h264_reader::rbsp::{BitRead, BitReader, BitReaderError, ByteReader};
use std::{
    io::{Read, Seek, SeekFrom},
    num::NonZeroUsize,
};

const VIDEO_PARAMETER_SET: u8 = 32;
const SEQUENCE_PARAMETER_SET: u8 = 33;
const PICTURE_PARAMETER_SET: u8 = 34;
const NAL_HEADER_BYTES: NonZeroUsize = match NonZeroUsize::new(2) {
    Some(bytes) => bytes,
    None => panic!("an HEVC NAL header has two bytes"),
};

/// Checks the one `hvcC` configuration of an HEVC sample entry, and its Dolby
/// Vision configuration when it has one. Returns the bit depth and the colour
/// declaration reconciled across the `colr` box and every SPS, if any.
pub(super) fn validate_configuration(
    description: &SampleDescription,
    reader: &mut (impl Read + Seek),
) -> Result<(
    u8,
    Option<ColourDescription>,
    crate::schema::records::PixelAspectRatio,
)> {
    let mut configurations = Vec::new();
    for (kind, payload) in &description.children {
        match kind {
            b"dvcC" | b"dvvC" => {
                let length = payload.end - payload.start;
                ensure!(
                    length == DOLBY_VISION_CONFIGURATION_BYTES,
                    "invalid Dolby Vision configuration size {length}"
                );
                reader.seek(SeekFrom::Start(payload.start))?;
                let mut record = [0; DOLBY_VISION_CONFIGURATION_BYTES as usize];
                reader.read_exact(&mut record)?;
                validate_dolby_vision(&record)?;
            }
            b"dvwC" => return Err(unsupported("Dolby Vision in a wrapper is unsupported")),
            b"lhvC" => return Err(unsupported("layered HEVC is unsupported")),
            b"hvcC" => configurations.push(payload.clone()),
            _ => {}
        }
    }
    let [payload] = &configurations[..] else {
        return Err(unsupported(
            "HEVC sample entry must contain one hvcC configuration",
        ));
    };
    let record = read_configuration_record(reader, payload.start..payload.end)?;
    let configuration = Configuration::read(&record)?;
    configuration.validate()?;
    for unit in configuration.units(VIDEO_PARAMETER_SET) {
        ensure!(
            max_layers_minus1(unit).map_err(parameter_set_error)? == 0,
            "multi-layer HEVC (alpha or multiview) is unsupported"
        );
    }
    let declared = (
        u32::from(configuration.chroma_format),
        u32::from(configuration.bit_depth_luma_minus8),
        u32::from(configuration.bit_depth_chroma_minus8),
    );
    let mut colour = description.colour;
    let mut full_range = None;
    let mut pixel_aspect = description.pixel_aspect;
    for unit in configuration.units(SEQUENCE_PARAMETER_SET) {
        let parameters = SequenceParameters::read(unit).map_err(parameter_set_error)?;
        let (chroma_format, luma_minus8, chroma_minus8) = parameters.format;
        ensure!(
            parameters.format == declared,
            "HEVC sequence parameter set declares chroma format {chroma_format} at {}/{} bits, unlike its hvcC header",
            luma_minus8 + 8,
            chroma_minus8 + 8
        );
        crate::media_metadata::merge_video_range(
            &mut full_range,
            parameters.usability.as_ref().and_then(|vui| vui.full_range),
        )?;
        let vui_colour = parameters.validate(description.width, description.height)?;
        let vui_aspect = parameters
            .usability
            .as_ref()
            .map(VideoUsability::pixel_aspect)
            .transpose()?
            .flatten();
        crate::media_metadata::merge_pixel_aspect(&mut pixel_aspect, vui_aspect)?;
        ColourDescription::merge(&mut colour, vui_colour)?;
    }
    crate::media_metadata::validate_video_range(
        description.full_range,
        full_range,
        configuration.bit_depth_luma_minus8 + 8,
        colour,
    )?;
    Ok((
        configuration.bit_depth_luma_minus8 + 8,
        colour,
        pixel_aspect.unwrap_or_default(),
    ))
}

/// Read only the bytes declared by the native hvcC fields. An extended MP4
/// box can claim far more bytes than its configuration contains; never reserve
/// its declared size before inspecting the array and NAL lengths.
fn read_configuration_record(
    reader: &mut (impl Read + Seek),
    payload: std::ops::Range<u64>,
) -> Result<Vec<u8>> {
    let length = payload.end - payload.start;
    ensure!(
        length >= 23,
        "invalid HEVC decoder configuration size {length}"
    );
    reader.seek(SeekFrom::Start(payload.start))?;
    let mut remaining = length;
    let mut record = Vec::new();
    read_part(reader, &mut record, &mut remaining, 23)?;
    let array_count = record[22];
    for _ in 0..array_count {
        read_part(reader, &mut record, &mut remaining, 3)?;
        let header = record.len() - 3;
        let unit_count = u16::from_be_bytes([record[header + 1], record[header + 2]]);
        for _ in 0..unit_count {
            read_part(reader, &mut record, &mut remaining, 2)?;
            let size = record.len() - 2;
            let unit_length = u16::from_be_bytes([record[size], record[size + 1]]);
            read_part(
                reader,
                &mut record,
                &mut remaining,
                usize::from(unit_length),
            )?;
        }
    }
    ensure!(
        remaining == 0,
        "HEVC decoder configuration has trailing bytes"
    );
    Ok(record)
}

fn read_part(
    reader: &mut impl Read,
    record: &mut Vec<u8>,
    remaining: &mut u64,
    bytes: usize,
) -> Result<()> {
    if u64::try_from(bytes).map_or(true, |bytes| bytes > *remaining) {
        return Err(unsupported("truncated HEVC decoder configuration"));
    }
    let start = record.len();
    record
        .try_reserve(bytes)
        .map_err(|_| unsupported("HEVC decoder configuration exceeds host address space"))?;
    record.resize(start + bytes, 0);
    reader.read_exact(&mut record[start..])?;
    *remaining -= bytes as u64;
    Ok(())
}

#[cfg(test)]
mod read_tests {
    use super::*;

    #[test]
    fn oversized_declared_box_fails_without_reserving_its_declared_length() {
        let mut header = vec![0; 23];
        header[22] = 1;
        let mut reader = std::io::Cursor::new(header);
        let error = read_configuration_record(&mut reader, 0..(1_u64 << 40)).unwrap_err();
        assert!(matches!(error, BuildError::Io(_)), "{error}");
    }

    #[test]
    fn trailing_bytes_do_not_expand_the_configuration_record() {
        let mut header = vec![0; 24];
        header[22] = 0;
        let mut reader = std::io::Cursor::new(&mut header);
        let error = read_configuration_record(&mut reader, 0..24).unwrap_err();
        assert!(error.to_string().contains("trailing bytes"), "{error}");
    }
}

/// A `DolbyVisionConfigurationRecord` is 24 bytes.
const DOLBY_VISION_CONFIGURATION_BYTES: u64 = 24;

/// Checks that the Dolby Vision stream's base layer is plain HEVC. The record
/// layout: `dv_version_major(8) dv_version_minor(8) dv_profile(7) dv_level(6)
/// rpu_present_flag(1) el_present_flag(1) bl_present_flag(1)
/// dv_bl_signal_compatibility_id(4)`, then reserved bits.
fn validate_dolby_vision(record: &[u8; 24]) -> Result<()> {
    let profile = record[2] >> 1;
    let el_present = record[3] & 0b10 != 0;
    let bl_present = record[3] & 0b1 != 0;
    let compatibility_id = record[4] >> 4;
    ensure!(
        bl_present && !el_present,
        "Dolby Vision profile {profile} with an enhancement layer or without a base layer is unsupported"
    );
    ensure!(
        compatibility_id != 0,
        "Dolby Vision profile {profile} has no cross-compatible base layer; it needs a Dolby Vision decoder"
    );
    Ok(())
}

/// The fields of an `HEVCDecoderConfigurationRecord` that conversion checks.
struct Configuration<'a> {
    version: u8,
    profile_space: u8,
    profile_idc: u8,
    compatibility: u32,
    chroma_format: u8,
    bit_depth_luma_minus8: u8,
    bit_depth_chroma_minus8: u8,
    length_size_minus_one: u8,
    arrays: Vec<ParameterSetArray<'a>>,
}

struct ParameterSetArray<'a> {
    complete: bool,
    nal_type: u8,
    units: Vec<&'a [u8]>,
}

impl<'a> Configuration<'a> {
    fn read(record: &'a [u8]) -> Result<Self> {
        let truncated = || unsupported("truncated HEVC decoder configuration");
        if record.len() < 23 {
            return Err(truncated());
        }
        let mut cursor = 23;
        let mut take = |length: usize| -> Result<&'a [u8]> {
            let bytes = record.get(cursor..cursor + length).ok_or_else(truncated)?;
            cursor += length;
            Ok(bytes)
        };
        let array_count = record[22];
        let mut arrays = Vec::with_capacity(array_count.into());
        for _ in 0..array_count {
            let header = take(3)?;
            let unit_count = u16::from_be_bytes([header[1], header[2]]);
            let units = (0..unit_count)
                .map(|_| {
                    let length = take(2)?;
                    take(usize::from(u16::from_be_bytes([length[0], length[1]])))
                })
                .collect::<Result<_>>()?;
            arrays.push(ParameterSetArray {
                complete: header[0] & 0x80 != 0,
                nal_type: header[0] & 0x3f,
                units,
            });
        }
        Ok(Self {
            version: record[0],
            profile_space: record[1] >> 6,
            profile_idc: record[1] & 0x1f,
            compatibility: u32::from_be_bytes([record[2], record[3], record[4], record[5]]),
            chroma_format: record[16] & 0b11,
            bit_depth_luma_minus8: record[17] & 0b111,
            bit_depth_chroma_minus8: record[18] & 0b111,
            length_size_minus_one: record[21] & 0b11,
            arrays,
        })
    }

    fn validate(&self) -> Result<()> {
        ensure!(
            self.version == 1,
            "unsupported HEVC decoder configuration version {}",
            self.version
        );
        // Compatibility flags 1 and 2 mark a Main- or Main 10-conforming stream
        // of another profile.
        let main = matches!(self.profile_idc, 1 | 2) || self.compatibility & (0b11 << 29) != 0;
        ensure!(
            self.profile_space == 0 && main,
            "HEVC profile {} is unsupported; conversion accepts HEVC Main and Main 10",
            self.profile_idc
        );
        ensure!(
            self.chroma_format == 1
                && matches!(self.bit_depth_luma_minus8, 0 | 2)
                && self.bit_depth_chroma_minus8 == self.bit_depth_luma_minus8,
            "HEVC must be 8- or 10-bit 4:2:0; found chroma format {} at {}/{} bits",
            self.chroma_format,
            self.bit_depth_luma_minus8 + 8,
            self.bit_depth_chroma_minus8 + 8
        );
        ensure!(
            self.length_size_minus_one != 2,
            "invalid HEVC NAL length size"
        );
        for nal_type in [
            VIDEO_PARAMETER_SET,
            SEQUENCE_PARAMETER_SET,
            PICTURE_PARAMETER_SET,
        ] {
            let arrays: Vec<_> = self
                .arrays
                .iter()
                .filter(|array| array.nal_type == nal_type)
                .collect();
            // Completeness guarantees that no parameter set of this type is in-band.
            ensure!(
                arrays.iter().all(|array| array.complete)
                    && arrays.iter().any(|array| !array.units.is_empty()),
                "HEVC sample entry must store complete type-{nal_type} parameter sets in hvcC"
            );
        }
        for array in &self.arrays {
            for unit in &array.units {
                ensure!(
                    unit.len() > 2
                        && unit[0] & 0x80 == 0
                        && (unit[0] >> 1) & 0x3f == array.nal_type
                        && (unit[0] & 1) == 0
                        && unit[1] >> 3 == 0,
                    "hvcC contains an invalid or non-base-layer type-{} NAL unit",
                    array.nal_type
                );
            }
        }
        Ok(())
    }

    fn units(&self, nal_type: u8) -> impl Iterator<Item = &'a [u8]> + '_ {
        self.arrays
            .iter()
            .filter(move |array| array.nal_type == nal_type)
            .flat_map(|array| array.units.iter().copied())
    }
}

#[derive(Debug)]
enum ParameterSetError {
    Bits(BitReaderError),
    OutOfRange(&'static str),
    /// The complete rejection message for syntax that no captured stream proves.
    Unproven(&'static str),
}

impl From<BitReaderError> for ParameterSetError {
    fn from(error: BitReaderError) -> Self {
        Self::Bits(error)
    }
}

impl std::fmt::Display for ParameterSetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bits(error) => write!(f, "{error:?}"),
            Self::OutOfRange(field) => write!(f, "{field} is out of range"),
            Self::Unproven(message) => f.write_str(message),
        }
    }
}

fn parameter_set_error(error: ParameterSetError) -> BuildError {
    if matches!(error, ParameterSetError::Unproven(_)) {
        unsupported(error.to_string())
    } else {
        unsupported(format!("invalid HEVC parameter set: {error}"))
    }
}

fn bounded(
    value: u32,
    max: u32,
    name: &'static str,
) -> std::result::Result<u32, ParameterSetError> {
    if value <= max {
        Ok(value)
    } else {
        Err(ParameterSetError::OutOfRange(name))
    }
}

fn rbsp(unit: &[u8]) -> BitReader<ByteReader<&[u8]>> {
    BitReader::new(ByteReader::skipping_bytes(unit, NAL_HEADER_BYTES))
}

/// Reads `vps_max_layers_minus1`; a second layer carries alpha or another view.
fn max_layers_minus1(unit: &[u8]) -> std::result::Result<u8, ParameterSetError> {
    let mut bits = rbsp(unit);
    bits.skip(6, "vps_video_parameter_set_id and base layer flags")?;
    Ok(bits.read(6, "vps_max_layers_minus1")?)
}

/// The sequence parameter set fields that decide the decoded picture.
struct SequenceParameters {
    /// `chroma_format_idc`, `bit_depth_luma_minus8`, `bit_depth_chroma_minus8`.
    format: (u32, u32, u32),
    interlaced_source: bool,
    width: u64,
    height: u64,
    usability: Option<VideoUsability>,
}

/// The VUI fields before timing information.
struct VideoUsability {
    aspect_ratio_idc: Option<u8>,
    sample_aspect_ratio: (u16, u16),
    full_range: Option<bool>,
    colour: Option<(u8, u8, u8)>,
    field_seq: bool,
    frame_field_info: bool,
    display_window: bool,
}

impl VideoUsability {
    fn pixel_aspect(&self) -> Result<Option<crate::schema::records::PixelAspectRatio>> {
        use crate::schema::records::PixelAspectRatio;
        // H.265 Table E.1: aspect_ratio_idc values 1 through 16.
        const RATIOS: [(u16, u16); 16] = [
            (1, 1),
            (12, 11),
            (10, 11),
            (16, 11),
            (40, 33),
            (24, 11),
            (20, 11),
            (32, 11),
            (80, 33),
            (18, 11),
            (15, 11),
            (64, 33),
            (160, 99),
            (4, 3),
            (3, 2),
            (2, 1),
        ];
        let (width, height) = match self.aspect_ratio_idc {
            None | Some(0) => return Ok(None),
            Some(id @ 1..=16) => RATIOS[usize::from(id - 1)],
            Some(255) => self.sample_aspect_ratio,
            Some(_) => return Err(unsupported("reserved HEVC pixel aspect ratio")),
        };
        PixelAspectRatio::new(u64::from(width), u64::from(height)).map(Some)
    }
}

impl SequenceParameters {
    /// Parses `seq_parameter_set_rbsp` (H.265 7.3.2.2) through the VUI fields
    /// that decide the decoded picture.
    fn read(unit: &[u8]) -> std::result::Result<Self, ParameterSetError> {
        let mut bits = rbsp(unit);
        let r = &mut bits;
        r.skip(4, "sps_video_parameter_set_id")?;
        let max_sub_layers_minus1: u8 = r.read(3, "sps_max_sub_layers_minus1")?;
        if max_sub_layers_minus1 > 6 {
            return Err(ParameterSetError::OutOfRange("sps_max_sub_layers_minus1"));
        }
        r.skip(1, "sps_temporal_id_nesting_flag")?;
        let interlaced_source = read_profile_tier_level(r, max_sub_layers_minus1)?;
        bounded(
            r.read_ue("sps_seq_parameter_set_id")?,
            15,
            "sps_seq_parameter_set_id",
        )?;
        let chroma_format_idc = bounded(r.read_ue("chroma_format_idc")?, 3, "chroma_format_idc")?;
        if chroma_format_idc == 3 {
            r.skip(1, "separate_colour_plane_flag")?;
        }
        let coded_width = u64::from(r.read_ue("pic_width_in_luma_samples")?);
        let coded_height = u64::from(r.read_ue("pic_height_in_luma_samples")?);
        let (mut crop_width, mut crop_height) = (0, 0);
        if r.read_bool("conformance_window_flag")? {
            let (unit_width, unit_height) = match chroma_format_idc {
                1 => (2, 2),
                2 => (2, 1),
                _ => (1, 1),
            };
            crop_width = unit_width
                * (u64::from(r.read_ue("conf_win_left_offset")?)
                    + u64::from(r.read_ue("conf_win_right_offset")?));
            crop_height = unit_height
                * (u64::from(r.read_ue("conf_win_top_offset")?)
                    + u64::from(r.read_ue("conf_win_bottom_offset")?));
        }
        let width = coded_width
            .checked_sub(crop_width)
            .filter(|width| *width > 0)
            .ok_or(ParameterSetError::OutOfRange("conformance window width"))?;
        let height = coded_height
            .checked_sub(crop_height)
            .filter(|height| *height > 0)
            .ok_or(ParameterSetError::OutOfRange("conformance window height"))?;
        let luma_minus8 = bounded(
            r.read_ue("bit_depth_luma_minus8")?,
            8,
            "bit_depth_luma_minus8",
        )?;
        let chroma_minus8 = bounded(
            r.read_ue("bit_depth_chroma_minus8")?,
            8,
            "bit_depth_chroma_minus8",
        )?;
        bounded(
            r.read_ue("log2_max_pic_order_cnt_lsb_minus4")?,
            12,
            "log2_max_pic_order_cnt_lsb_minus4",
        )?;
        let first_ordered_layer = if r.read_bool("sps_sub_layer_ordering_info_present_flag")? {
            0
        } else {
            max_sub_layers_minus1
        };
        for _ in first_ordered_layer..=max_sub_layers_minus1 {
            r.read_ue("sps_max_dec_pic_buffering_minus1")?;
            r.read_ue("sps_max_num_reorder_pics")?;
            r.read_ue("sps_max_latency_increase_plus1")?;
        }
        for name in [
            "log2_min_luma_coding_block_size_minus3",
            "log2_diff_max_min_luma_coding_block_size",
            "log2_min_luma_transform_block_size_minus2",
            "log2_diff_max_min_luma_transform_block_size",
            "max_transform_hierarchy_depth_inter",
            "max_transform_hierarchy_depth_intra",
        ] {
            r.read_ue(name)?;
        }
        if r.read_bool("scaling_list_enabled_flag")?
            && r.read_bool("sps_scaling_list_data_present_flag")?
        {
            skip_scaling_list_data(r)?;
        }
        r.skip(
            2,
            "amp_enabled_flag and sample_adaptive_offset_enabled_flag",
        )?;
        if r.read_bool("pcm_enabled_flag")? {
            return Err(ParameterSetError::Unproven(
                "HEVC PCM coding (pcm_enabled_flag) is unsupported",
            ));
        }
        let set_count = bounded(
            r.read_ue("num_short_term_ref_pic_sets")?,
            64,
            "num_short_term_ref_pic_sets",
        )?;
        for index in 0..set_count {
            skip_short_term_ref_pic_set(r, index)?;
        }
        if r.read_bool("long_term_ref_pics_present_flag")? {
            return Err(ParameterSetError::Unproven(
                "HEVC long-term reference pictures (long_term_ref_pics_present_flag) are unsupported",
            ));
        }
        r.skip(
            2,
            "sps_temporal_mvp_enabled_flag and strong_intra_smoothing_enabled_flag",
        )?;
        let usability = if r.read_bool("vui_parameters_present_flag")? {
            Some(read_video_usability(r)?)
        } else {
            None
        };
        Ok(Self {
            format: (chroma_format_idc, luma_minus8, chroma_minus8),
            interlaced_source,
            width,
            height,
            usability,
        })
    }

    /// Checks the decoded picture against the sample entry and returns the
    /// VUI colour description that passes through, if any.
    fn validate(&self, width: u16, height: u16) -> Result<Option<ColourDescription>> {
        // Scan checks come first: a field-coded picture has half the frame height.
        // An unknown source scan type still decodes as frames.
        ensure!(
            !self.interlaced_source,
            "HEVC declares interlaced source pictures; interlaced video is unsupported"
        );
        // Progressive streams may also set frame_field_info_present_flag to carry
        // picture-timing SEI. Rejecting them too is deliberate: conversion does
        // not read the SEI that tells them apart from field-timed pictures.
        ensure!(
            self.usability
                .as_ref()
                .is_none_or(|usability| !usability.field_seq && !usability.frame_field_info),
            "field-coded or field-timed HEVC is unsupported"
        );
        ensure!(
            (self.width, self.height) == (u64::from(width), u64::from(height)),
            "HEVC picture size {}x{} differs from the sample entry {width}x{height}",
            self.width,
            self.height
        );
        let Some(usability) = &self.usability else {
            return Ok(None);
        };
        ensure!(
            !usability.display_window,
            "HEVC default display window cropping is unsupported"
        );
        match usability.colour {
            Some((primaries, transfer, matrix)) => {
                validate_color(primaries.into(), transfer.into(), matrix.into())
            }
            None => Ok(None),
        }
    }
}

/// Reads `profile_tier_level(1, max_sub_layers_minus1)` and returns
/// `general_interlaced_source_flag`. Sub-layers are read only when they carry
/// no profile or level fields, as in the captured x265 temporal-layer stream.
fn read_profile_tier_level(
    r: &mut impl BitRead,
    max_sub_layers_minus1: u8,
) -> std::result::Result<bool, ParameterSetError> {
    r.skip(
        8 + 32 + 1,
        "general profile, compatibility, and progressive source flags",
    )?;
    let interlaced = r.read_bool("general_interlaced_source_flag")?;
    r.skip(46 + 8, "general constraint flags and general_level_idc")?;
    for _ in 0..max_sub_layers_minus1 {
        let profile_present = r.read_bool("sub_layer_profile_present_flag")?;
        let level_present = r.read_bool("sub_layer_level_present_flag")?;
        if profile_present || level_present {
            return Err(ParameterSetError::Unproven(
                "HEVC sub-layer profile or level information (sub_layer_profile_present_flag, sub_layer_level_present_flag) is unsupported",
            ));
        }
    }
    if max_sub_layers_minus1 > 0 {
        r.skip(
            2 * (8 - u32::from(max_sub_layers_minus1)),
            "reserved_zero_2bits",
        )?;
    }
    Ok(interlaced)
}

/// Skips `scaling_list_data()` (H.265 7.3.4).
fn skip_scaling_list_data(r: &mut impl BitRead) -> std::result::Result<(), ParameterSetError> {
    for size_id in 0..4_u32 {
        let matrices = if size_id == 3 { 2 } else { 6 };
        for _ in 0..matrices {
            if r.read_bool("scaling_list_pred_mode_flag")? {
                if size_id > 1 {
                    r.read_se("scaling_list_dc_coef_minus8")?;
                }
                for _ in 0..64.min(1 << (4 + (size_id << 1))) {
                    r.read_se("scaling_list_delta_coef")?;
                }
            } else {
                r.read_ue("scaling_list_pred_matrix_id_delta")?;
            }
        }
    }
    Ok(())
}

/// Skips one `st_ref_pic_set(stRpsIdx)` of a sequence parameter set (H.265
/// 7.3.7). Only explicitly coded sets are read, as in the captured
/// VideoToolbox stream; a set predicted from the previous set rejects.
fn skip_short_term_ref_pic_set(
    r: &mut impl BitRead,
    index: u32,
) -> std::result::Result<(), ParameterSetError> {
    if index > 0 && r.read_bool("inter_ref_pic_set_prediction_flag")? {
        return Err(ParameterSetError::Unproven(
            "HEVC predicted short-term reference picture sets (inter_ref_pic_set_prediction_flag) are unsupported",
        ));
    }
    let negative_count = bounded(r.read_ue("num_negative_pics")?, 16, "num_negative_pics")?;
    let positive_count = bounded(r.read_ue("num_positive_pics")?, 16, "num_positive_pics")?;
    for _ in 0..negative_count + positive_count {
        bounded(r.read_ue("delta_poc_minus1")?, 32_767, "delta_poc_minus1")?;
        r.skip(1, "used_by_curr_pic_flag")?;
    }
    Ok(())
}

/// Reads `vui_parameters()` (H.265 E.2.1) through the default display window.
fn read_video_usability(
    r: &mut impl BitRead,
) -> std::result::Result<VideoUsability, ParameterSetError> {
    let mut usability = VideoUsability {
        aspect_ratio_idc: None,
        sample_aspect_ratio: (0, 0),
        full_range: None,
        colour: None,
        field_seq: false,
        frame_field_info: false,
        display_window: false,
    };
    if r.read_bool("aspect_ratio_info_present_flag")? {
        let idc: u8 = r.read(8, "aspect_ratio_idc")?;
        if idc == 255 {
            usability.sample_aspect_ratio = (r.read(16, "sar_width")?, r.read(16, "sar_height")?);
        }
        usability.aspect_ratio_idc = Some(idc);
    }
    if r.read_bool("overscan_info_present_flag")? {
        r.skip(1, "overscan_appropriate_flag")?;
    }
    if r.read_bool("video_signal_type_present_flag")? {
        r.skip(3, "video_format")?;
        usability.full_range = Some(r.read_bool("video_full_range_flag")?);
        if r.read_bool("colour_description_present_flag")? {
            usability.colour = Some((
                r.read(8, "colour_primaries")?,
                r.read(8, "transfer_characteristics")?,
                r.read(8, "matrix_coeffs")?,
            ));
        }
    }
    if r.read_bool("chroma_loc_info_present_flag")? {
        r.read_ue("chroma_sample_loc_type_top_field")?;
        r.read_ue("chroma_sample_loc_type_bottom_field")?;
    }
    r.skip(1, "neutral_chroma_indication_flag")?;
    usability.field_seq = r.read_bool("field_seq_flag")?;
    usability.frame_field_info = r.read_bool("frame_field_info_present_flag")?;
    if r.read_bool("default_display_window_flag")? {
        for name in [
            "def_disp_win_left_offset",
            "def_disp_win_right_offset",
            "def_disp_win_top_offset",
            "def_disp_win_bottom_offset",
        ] {
            usability.display_window |= r.read_ue(name)? != 0;
        }
    }
    Ok(usability)
}
