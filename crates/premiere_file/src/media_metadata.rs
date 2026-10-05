//! Bounded ISO-BMFF metadata checks for facts FFmpeg normalizes or omits.
use crate::error::{ensure, unsupported, Result};
use crate::schema::records::PixelAspectRatio;
use h264_reader::{
    avcc::AvcDecoderConfigurationRecord,
    nal::{
        sps::{AspectRatioInfo, ChromaFormat, FrameMbsFlags, SeqParameterSet},
        Nal, RefNal, UnitType,
    },
};
use std::{
    collections::BTreeSet,
    io::{Read, Seek, SeekFrom},
    ops::Range,
};

pub(crate) type MediaBox = ([u8; 4], Range<u64>);

#[derive(Debug, Clone)]
pub(crate) struct EditEntry {
    pub(crate) segment_duration: u64,
    pub(crate) media_time: i64,
    pub(crate) media_rate: i16,
    pub(crate) media_rate_fraction: i16,
}

#[derive(Debug)]
pub(crate) struct TrackMetadata {
    pub(crate) handler: [u8; 4],
    pub(crate) timescale: u32,
    pub(crate) duration: u64,
    pub(crate) edit: Option<Vec<EditEntry>>,
    pub(crate) sample_description: Option<SampleDescription>,
    pub(crate) sample_timing: Option<SampleTiming>,
}

#[derive(Debug)]
pub(crate) struct SampleTiming {
    /// Unnormalized STTS counts/deltas, retained to detect demuxer clock repair.
    pub(crate) decode_runs: Vec<(u32, u32)>,
    pub(crate) sample_count: u32,
    pub(crate) media_end: u64,
    pub(crate) constant_duration: Option<u32>,
    pub(crate) final_duration: u32,
    pub(crate) zero_composition_offsets: bool,
    pub(crate) legacy_signed_ctts: bool,
}

#[derive(Debug)]
pub(crate) struct MovieMetadata {
    pub(crate) timescale: u32,
    pub(crate) tracks: Vec<TrackMetadata>,
}

pub(crate) fn read_movie_metadata(
    mut reader: impl Read + Seek,
    size: u64,
    validate_video: bool,
) -> Result<MovieMetadata> {
    read_movie_metadata_with_colour(&mut reader, size, validate_video, false)
}

/// Export can retain sRGB declarations for a foreign editable picture without
/// admitting them to the native Premiere writer or the Tesseract decoder.
pub(crate) fn read_export_movie_metadata(
    mut reader: impl Read + Seek,
    size: u64,
) -> Result<MovieMetadata> {
    read_movie_metadata_with_colour(&mut reader, size, true, true)
}

fn read_movie_metadata_with_colour(
    mut reader: impl Read + Seek,
    size: u64,
    validate_video: bool,
    export_colour: bool,
) -> Result<MovieMetadata> {
    let roots = boxes(&mut reader, 0..size)?;
    ensure!(
        !roots.iter().any(|(kind, _)| kind == b"moof"),
        "fragmented MP4 media is unsupported"
    );
    let moov = exactly_one(&roots, *b"moov", "movie")?;
    let moov_boxes = boxes(&mut reader, moov)?;
    validate_user_metadata(&mut reader, &moov_boxes)?;
    let mvhd = exactly_one(&moov_boxes, *b"mvhd", "movie header")?;
    let version = bytes::<1>(&mut reader, mvhd.start)?[0];
    let timescale_offset = match version {
        0 => 12,
        1 => 20,
        _ => return Err(unsupported("unsupported MP4 movie-header version")),
    };
    let timescale = be_u32(&mut reader, checked_add(mvhd.start, timescale_offset)?)?;
    let mut tracks = Vec::new();
    for (_, trak) in moov_boxes.iter().filter(|(kind, _)| kind == b"trak") {
        tracks.push(read_track(&mut reader, trak.clone())?);
    }
    if validate_video {
        ensure!(
            tracks
                .iter()
                .filter(|track| track.handler == *b"vide")
                .count()
                == 1,
            "source requires exactly one video stream"
        );
        // Classify handlers before interpreting sample entries: data tracks
        // mislabeled as a second picture must still fail the stream-count gate.
        for (track, (_, trak)) in tracks
            .iter_mut()
            .zip(moov_boxes.iter().filter(|(kind, _)| kind == b"trak"))
        {
            if track.handler == *b"vide" {
                let track_boxes = boxes(&mut reader, trak.clone())?;
                let (description, timing) =
                    read_video_description(&mut reader, &track_boxes, export_colour)?;
                track.sample_description = Some(description);
                track.sample_timing = Some(timing);
            }
        }
    }
    Ok(MovieMetadata { timescale, tracks })
}

// libavformat intentionally skips malformed descriptive tags. Preserve the
// existing admission rule that their nested box ranges and data headers are valid.
fn validate_user_metadata(reader: &mut (impl Read + Seek), movie: &[MediaBox]) -> Result<()> {
    let mut metadata = movie
        .iter()
        .filter(|(kind, _)| kind == b"meta")
        .cloned()
        .collect::<Vec<_>>();
    for (_, range) in movie.iter().filter(|(kind, _)| kind == b"udta") {
        metadata.extend(
            boxes(reader, range.clone())?
                .into_iter()
                .filter(|(kind, _)| kind == b"meta"),
        );
    }
    for (_, range) in metadata {
        ensure!(
            range.end - range.start >= 4,
            "truncated MP4 metadata header"
        );
        // An ISO full box stores its version and flags before the children. A
        // QuickTime metadata atom has neither and starts with its `hdlr` child.
        let quicktime =
            range.end - range.start >= 8 && bytes::<4>(reader, range.start + 4)? == *b"hdlr";
        let children = if quicktime {
            range
        } else {
            range.start + 4..range.end
        };
        for (_, list) in boxes(reader, children)?
            .into_iter()
            .filter(|(kind, _)| kind == b"ilst")
        {
            for (_, item) in boxes(reader, list)? {
                for (kind, data) in boxes(reader, item)? {
                    if kind == *b"data" {
                        ensure!(
                            data.end - data.start >= 8,
                            "truncated MP4 metadata data header"
                        );
                    }
                }
            }
        }
    }
    Ok(())
}

fn read_track(reader: &mut (impl Read + Seek), trak: Range<u64>) -> Result<TrackMetadata> {
    let track_boxes = boxes(reader, trak)?;
    let mdia = exactly_one(&track_boxes, *b"mdia", "media")?;
    let media_boxes = boxes(reader, mdia)?;
    let hdlr = exactly_one(&media_boxes, *b"hdlr", "handler")?;
    let handler = bytes::<4>(reader, checked_add(hdlr.start, 8)?)?;
    let mdhd = exactly_one(&media_boxes, *b"mdhd", "media header")?;
    let version = bytes::<1>(reader, mdhd.start)?[0];
    let (timescale_offset, duration_offset, wide) = match version {
        0 => (12, 16, false),
        1 => (20, 24, true),
        _ => return Err(unsupported("unsupported MP4 media-header version")),
    };
    let timescale = be_u32(reader, checked_add(mdhd.start, timescale_offset)?)?;
    let duration = if wide {
        be_u64(reader, checked_add(mdhd.start, duration_offset)?)?
    } else {
        u64::from(be_u32(reader, checked_add(mdhd.start, duration_offset)?)?)
    };
    let edit = read_edit(reader, &track_boxes)?;
    Ok(TrackMetadata {
        handler,
        timescale,
        duration,
        edit,
        sample_description: None,
        sample_timing: None,
    })
}

fn read_edit(
    reader: &mut (impl Read + Seek),
    track_boxes: &[MediaBox],
) -> Result<Option<Vec<EditEntry>>> {
    let Some((_, edts)) = track_boxes.iter().find(|(kind, _)| kind == b"edts") else {
        return Ok(None);
    };
    let entries = boxes(reader, edts.clone())?;
    let elst = exactly_one(&entries, *b"elst", "edit list")?;
    let header = bytes::<8>(reader, elst.start)?;
    let version = header[0];
    ensure!(
        version <= 1 && header[1..4] == [0; 3],
        "unsupported MP4 edit-list version"
    );
    let count = u32::from_be_bytes(header[4..8].try_into().expect("four bytes"));
    ensure!(count <= 4096, "invalid or excessive MP4 edit list");
    let entry_size = if version == 0 { 12 } else { 20 };
    let required = u64::from(count)
        .checked_mul(entry_size)
        .and_then(|n| n.checked_add(8))
        .ok_or_else(|| unsupported("MP4 edit list overflows"))?;
    ensure!(required == elst.end - elst.start, "truncated MP4 edit list");
    let mut result = Vec::new();
    result
        .try_reserve(
            usize::try_from(count)
                .map_err(|_| unsupported("MP4 edit list exceeds host address space"))?,
        )
        .map_err(|_| unsupported("MP4 edit list exceeds host address space"))?;
    let mut cursor = elst.start + 8;
    for _ in 0..count {
        let (segment_duration, media_time) = if version == 0 {
            (
                u64::from(be_u32(reader, cursor)?),
                i64::from(be_i32(reader, cursor + 4)?),
            )
        } else {
            (be_u64(reader, cursor)?, be_i64(reader, cursor + 8)?)
        };
        let rate_at = cursor + if version == 0 { 8 } else { 16 };
        result.push(EditEntry {
            segment_duration,
            media_time,
            media_rate: be_i16(reader, rate_at)?,
            media_rate_fraction: be_i16(reader, rate_at + 2)?,
        });
        cursor += entry_size;
    }
    Ok(Some(result))
}

fn read_video_description(
    reader: &mut (impl Read + Seek),
    track_boxes: &[MediaBox],
    export_colour: bool,
) -> Result<(SampleDescription, SampleTiming)> {
    let tkhd = exactly_one(track_boxes, *b"tkhd", "track header")?;
    let tkhd_version = bytes::<1>(reader, tkhd.start)?[0];
    let matrix_offset = match tkhd_version {
        0 => 40,
        1 => 52,
        _ => return Err(unsupported("unsupported MP4 track-header version")),
    };
    let matrix = bytes::<36>(reader, tkhd.start + matrix_offset)?;
    let display_width = be_u32(reader, tkhd.start + matrix_offset + 36)?;
    let display_height = be_u32(reader, tkhd.start + matrix_offset + 40)?;

    let mdia = exactly_one(track_boxes, *b"mdia", "media")?;
    let mdia_boxes = boxes(reader, mdia)?;
    let minf = exactly_one(&mdia_boxes, *b"minf", "media information")?;
    let minf_boxes = boxes(reader, minf)?;
    let stbl = exactly_one(&minf_boxes, *b"stbl", "sample table")?;
    let stbl_boxes = boxes(reader, stbl)?;
    let timing = validate_sample_tables(reader, &stbl_boxes)?;
    let stsd = exactly_one(&stbl_boxes, *b"stsd", "sample description")?;
    let header = bytes::<8>(reader, stsd.start)?;
    ensure!(
        header[..4] == [0; 4]
            && u32::from_be_bytes(header[4..].try_into().expect("four bytes")) == 1,
        "multiple or versioned MP4 sample descriptions unsupported"
    );
    let entries = boxes(reader, stsd.start + 8..stsd.end)?;
    let [(entry, payload)] = entries.as_slice() else {
        return Err(unsupported(
            "MP4 must contain one unambiguous visual sample description",
        ));
    };
    let mut payload = payload.clone();
    ensure!(
        payload.end - payload.start >= 78,
        "truncated MP4 sample description"
    );
    let dimensions = bytes::<28>(reader, payload.start)?;
    let width = u16::from_be_bytes([dimensions[24], dimensions[25]]);
    let height = u16::from_be_bytes([dimensions[26], dimensions[27]]);
    // The visual sample entry's depth field (24 opaque, 32 with alpha), which
    // ProRes 4444 writers set for an alpha picture.
    let depth = u16::from_be_bytes(bytes::<2>(reader, payload.start + 74)?);
    payload.start += 78;
    let children = boxes(reader, payload)?;
    let mut colour = None;
    let mut full_range = None;
    let mut pixel_aspect = None;
    let mut seen = BTreeSet::new();
    for (kind, payload) in children.iter().cloned() {
        if [*b"pasp", *b"colr", *b"clap", *b"fiel"].contains(&kind) && !seen.insert(kind) {
            return Err(unsupported("duplicate MP4 display metadata"));
        }
        match &kind {
            b"pasp" => {
                ensure!(
                    payload.end - payload.start == 8,
                    "invalid MP4 pixel aspect ratio"
                );
                let value = bytes::<8>(reader, payload.start)?;
                let x = u32::from_be_bytes(value[..4].try_into().expect("four bytes"));
                let y = u32::from_be_bytes(value[4..].try_into().expect("four bytes"));
                pixel_aspect = Some(PixelAspectRatio::new(u64::from(x), u64::from(y))?);
            }
            b"colr" => {
                let length = payload.end - payload.start;
                ensure!((10..=11).contains(&length), "unsupported MP4 color profile");
                let value = bytes::<10>(reader, payload.start)?;
                ensure!(
                    (value[..4] == *b"nclc" && length == 10)
                        || (value[..4] == *b"nclx" && length == 11),
                    "unsupported MP4 color profile"
                );
                colour = validate_color_for_use(
                    u16::from_be_bytes([value[4], value[5]]),
                    u16::from_be_bytes([value[6], value[7]]),
                    u16::from_be_bytes([value[8], value[9]]),
                    export_colour,
                )?;
                if length == 11 {
                    let flags = bytes::<1>(reader, payload.start + 10)?[0];
                    ensure!(flags & 0x7f == 0, "reserved MP4 color flags unsupported");
                    merge_video_range(&mut full_range, Some(flags & 0x80 != 0))?;
                }
            }
            b"clap" => {
                ensure!(
                    payload.end - payload.start == 32,
                    "invalid MP4 clean aperture"
                );
                let value = bytes::<32>(reader, payload.start)?;
                // Numerators and denominators of the aperture width and height
                // and of its horizontal and vertical offsets.
                let words: [u32; 8] = std::array::from_fn(|index| {
                    u32::from_be_bytes(
                        value[index * 4..index * 4 + 4]
                            .try_into()
                            .expect("four bytes"),
                    )
                });
                let [width_n, width_d, height_n, height_d, x_n, x_d, y_n, y_d] = words;
                ensure!(
                    width_d != 0 && height_d != 0 && x_d != 0 && y_d != 0,
                    "invalid MP4 clean aperture"
                );
                // The whole picture with zero offsets, as iPhone captures
                // declare it, crops nothing; any other aperture changes it.
                ensure!(
                    u64::from(width_n) == u64::from(width) * u64::from(width_d)
                        && u64::from(height_n) == u64::from(height) * u64::from(height_d)
                        && x_n == 0
                        && y_n == 0,
                    "MP4 clean-aperture cropping unsupported"
                );
            }
            b"fiel" => ensure!(
                payload.end - payload.start == 2 && bytes::<1>(reader, payload.start)?[0] == 1,
                "interlaced video is unsupported; the fiel box must declare one progressive field"
            ),
            _ => {}
        }
    }
    let orientation = video_orientation(matrix, width, height)?;
    Ok((
        SampleDescription {
            entry: *entry,
            width,
            height,
            depth,
            display_dimensions: [display_width, display_height],
            colour,
            orientation,
            full_range,
            pixel_aspect,
            children,
        },
        timing,
    ))
}

fn validate_sample_tables(
    reader: &mut (impl Read + Seek),
    boxes_: &[MediaBox],
) -> Result<SampleTiming> {
    let stts = exactly_one(boxes_, *b"stts", "sample timing")?;
    let header = bytes::<8>(reader, stts.start)?;
    let run_count = u32::from_be_bytes(header[4..].try_into().expect("four bytes"));
    ensure!(
        header[..4] == [0; 4]
            && run_count > 0
            && u64::from(run_count)
                .checked_mul(8)
                .and_then(|n| n.checked_add(8))
                == Some(stts.end - stts.start),
        "packaged MP4 has invalid sample-timing runs"
    );
    let mut decode_runs = Vec::new();
    decode_runs
        .try_reserve_exact(usize::try_from(run_count).unwrap_or(usize::MAX))
        .map_err(|_| unsupported("packaged MP4 has excessive sample-timing runs"))?;
    let mut sample_count = 0_u32;
    let mut media_end = 0_u64;
    let mut constant_duration = None;
    let mut constant = true;
    let mut final_duration = 0;
    for index in 0..run_count {
        let at = stts.start + 8 + u64::from(index) * 8;
        let samples = be_u32(reader, at)?;
        let duration = be_u32(reader, at + 4)?;
        ensure!(
            samples > 0 && duration > 0,
            "packaged MP4 has invalid sample-timing runs"
        );
        sample_count = sample_count
            .checked_add(samples)
            .ok_or_else(|| unsupported("packaged MP4 sample count overflows"))?;
        media_end = media_end
            .checked_add(u64::from(samples) * u64::from(duration))
            .ok_or_else(|| unsupported("packaged MP4 sample timeline overflows"))?;
        decode_runs.push((samples, duration));
        constant &= constant_duration.is_none_or(|value| value == duration);
        constant_duration.get_or_insert(duration);
        final_duration = duration;
    }
    let mut zero_composition_offsets = true;
    let mut legacy_signed_ctts = false;
    if let Some((_, ctts)) = boxes_.iter().find(|(kind, _)| kind == b"ctts") {
        let header = bytes::<8>(reader, ctts.start)?;
        let version = header[0];
        let run_count = u32::from_be_bytes(header[4..].try_into().expect("four bytes"));
        ensure!(
            version <= 1
                && header[1..4] == [0; 3]
                && run_count > 0
                && u64::from(run_count)
                    .checked_mul(8)
                    .and_then(|n| n.checked_add(8))
                    == Some(ctts.end - ctts.start),
            "MP4 composition offset table has invalid version, count or unsigned offset"
        );
        let mut covered = 0_u32;
        for index in 0..run_count {
            let at = ctts.start + 8 + u64::from(index) * 8;
            let samples = be_u32(reader, at)?;
            let raw_offset = be_u32(reader, at + 4)?;
            ensure!(
                samples > 0,
                "MP4 composition offset table has invalid version, count or unsigned offset"
            );
            covered = covered.checked_add(samples).ok_or_else(|| {
                unsupported("MP4 composition offset table sample count overflows")
            })?;
            // Legacy CTTSv0 files store signed offsets in the same bytes.
            // Admission still proves the complete physical presentation clock.
            legacy_signed_ctts |= version == 0 && i32::try_from(raw_offset).is_err();
            zero_composition_offsets &= raw_offset == 0;
        }
        ensure!(
            covered == sample_count,
            "MP4 composition offset table does not cover the exact sample count"
        );
    }
    if let (Some((_, stsc)), Some((_, stsz))) = (
        boxes_.iter().find(|(kind, _)| kind == b"stsc"),
        boxes_.iter().find(|(kind, _)| kind == b"stsz"),
    ) {
        let header = bytes::<8>(reader, stsc.start)?;
        let count = u32::from_be_bytes(header[4..].try_into().expect("four bytes"));
        ensure!(
            header[..4] == [0; 4]
                && count > 0
                && u64::from(count)
                    .checked_mul(12)
                    .and_then(|n| n.checked_add(8))
                    == Some(stsc.end - stsc.start),
            "packaged MP4 has invalid sample-to-chunk runs"
        );
        let mut runs = Vec::new();
        runs.try_reserve_exact(usize::try_from(count).unwrap_or(usize::MAX))
            .map_err(|_| unsupported("packaged MP4 has excessive sample-to-chunk runs"))?;
        let mut previous = 0;
        for index in 0..count {
            let at = stsc.start + 8 + u64::from(index) * 12;
            let first = be_u32(reader, at)?;
            let samples = be_u32(reader, at + 4)?;
            ensure!(
                first > previous && samples > 0 && (index != 0 || first == 1),
                "packaged MP4 has invalid sample-to-chunk runs"
            );
            runs.push((first, samples));
            previous = first;
        }
        let chunk_box = boxes_
            .iter()
            .find(|(kind, _)| kind == b"stco" || kind == b"co64")
            .ok_or_else(|| unsupported("packaged MP4 has no chunk offsets"))?;
        let chunk_header = bytes::<8>(reader, chunk_box.1.start)?;
        let chunk_count = u32::from_be_bytes(chunk_header[4..].try_into().expect("four bytes"));
        let entry_size = if chunk_box.0 == *b"stco" { 4 } else { 8 };
        ensure!(
            chunk_header[..4] == [0; 4]
                && u64::from(chunk_count)
                    .checked_mul(entry_size)
                    .and_then(|n| n.checked_add(8))
                    == Some(chunk_box.1.end - chunk_box.1.start),
            "packaged MP4 has invalid chunk offsets"
        );
        let terminal_chunk = chunk_count
            .checked_add(1)
            .ok_or_else(|| unsupported("packaged MP4 chunk count overflows"))?;
        let mut described_samples = 0_u64;
        for (index, &(first, samples)) in runs.iter().enumerate() {
            let end = runs
                .get(index + 1)
                .map_or(terminal_chunk, |&(next, _)| next);
            ensure!(
                first <= chunk_count && end <= terminal_chunk,
                "packaged MP4 sample-to-chunk run exceeds its chunk offsets"
            );
            described_samples = described_samples
                .checked_add(u64::from(end - first) * u64::from(samples))
                .ok_or_else(|| unsupported("packaged MP4 sample count overflows"))?;
        }
        let size_header = bytes::<12>(reader, stsz.start)?;
        let sample_size = u32::from_be_bytes(size_header[4..8].try_into().expect("four bytes"));
        let size_sample_count =
            u32::from_be_bytes(size_header[8..].try_into().expect("four bytes"));
        let expected_size = if sample_size == 0 {
            u64::from(size_sample_count)
                .checked_mul(4)
                .and_then(|n| n.checked_add(12))
        } else {
            Some(12)
        };
        ensure!(
            size_header[..4] == [0; 4]
                && expected_size == Some(stsz.end - stsz.start)
                && described_samples == u64::from(size_sample_count)
                && size_sample_count == sample_count,
            "packaged MP4 sample table does not cover every declared sample"
        );
    }
    Ok(SampleTiming {
        decode_runs,
        sample_count,
        media_end,
        constant_duration: constant.then_some(constant_duration.expect("nonempty timing runs")),
        final_duration,
        zero_composition_offsets,
        legacy_signed_ctts,
    })
}

/// The one visual sample entry, after the checks shared by codecs.
#[derive(Debug)]
pub(crate) struct SampleDescription {
    pub(crate) entry: [u8; 4],
    pub(crate) width: u16,
    pub(crate) height: u16,
    /// The visual sample entry's depth field: 24 for an opaque picture, 32
    /// when the picture carries alpha.
    pub(crate) depth: u16,
    /// Track-header dimensions in unsigned 16.16 pixels, before orientation.
    display_dimensions: [u32; 2],
    pub(crate) colour: Option<ColourDescription>,
    pub(crate) orientation: crate::schema::VideoOrientation,
    pub(crate) full_range: Option<bool>,
    pub(crate) pixel_aspect: Option<PixelAspectRatio>,
    pub(crate) children: Vec<MediaBox>,
}

impl SampleDescription {
    /// Check the track header only after container and codec PAR agree. A
    /// codec-only VUI declaration can explain aspect-adjusted tkhd dimensions.
    pub(crate) fn validate_display_dimensions(&self, aspect: PixelAspectRatio) -> Result<()> {
        let [display_width, display_height] = self.display_dimensions;
        let coded_width = u32::from(self.width) << 16;
        let (numerator, denominator) = aspect.terms();
        let declared = u128::from(display_width) * u128::from(denominator);
        let expected = u128::from(coded_width) * u128::from(numerator);
        // Allow only the rounding of tkhd's 16.16 representation.
        let aspect_width = declared.abs_diff(expected) <= u128::from(denominator) / 2;
        ensure!(
            (display_width == coded_width || aspect_width)
                && display_height == u32::from(self.height) << 16,
            "MP4 display dimensions must match the sample entry and pixel aspect"
        );
        Ok(())
    }
}

pub(crate) fn merge_pixel_aspect(
    current: &mut Option<PixelAspectRatio>,
    declared: Option<PixelAspectRatio>,
) -> Result<()> {
    let Some(declared) = declared else {
        return Ok(());
    };
    ensure!(
        current.is_none_or(|current| current.agrees(declared)),
        "conflicting video pixel aspect ratio declarations"
    );
    *current = Some(declared);
    Ok(())
}

pub(crate) fn validate_h264(
    description: &SampleDescription,
    extradata: &[u8],
) -> Result<(Option<ColourDescription>, PixelAspectRatio)> {
    let (_, colour, aspect) = inspect_h264(description, extradata, false)?;
    Ok((colour, aspect))
}

pub(crate) fn inspect_export_h264(
    description: &SampleDescription,
    extradata: &[u8],
) -> Result<(u8, Option<ColourDescription>, PixelAspectRatio)> {
    inspect_h264(description, extradata, true)
}

fn inspect_h264(
    description: &SampleDescription,
    extradata: &[u8],
    export_picture: bool,
) -> Result<(u8, Option<ColourDescription>, PixelAspectRatio)> {
    ensure!(description.entry == *b"avc1", "missing AVC sample entry");
    let configuration = AvcDecoderConfigurationRecord::try_from(extradata)
        .map_err(|e| unsupported(format!("invalid H.264 decoder configuration: {e:?}")))?;
    ensure!(
        configuration.length_size_minus_one() != 2,
        "unsupported H.264 NAL length size"
    );
    let mut cursor = 5usize;
    let count = usize::from(extradata[cursor] & 0x1f);
    cursor += 1;
    let mut colour = description.colour;
    let mut full_range = None;
    let mut pixel_aspect = description.pixel_aspect;
    let mut bit_depth = None;
    for _ in 0..count {
        ensure!(
            cursor + 2 <= extradata.len(),
            "invalid H.264 decoder configuration"
        );
        let length = usize::from(u16::from_be_bytes([
            extradata[cursor],
            extradata[cursor + 1],
        ]));
        cursor += 2;
        ensure!(
            length > 0
                && cursor
                    .checked_add(length)
                    .is_some_and(|end| end <= extradata.len()),
            "empty H.264 sequence parameter set"
        );
        let parameters = &extradata[cursor..cursor + length];
        cursor += length;
        let nal = RefNal::new(parameters, &[], true);
        ensure!(
            nal.header()
                .map_err(|e| unsupported(format!("invalid H.264 header: {e:?}")))?
                .nal_unit_type()
                == UnitType::SeqParameterSet,
            "AVC configuration contains a non-SPS parameter set"
        );
        let sps = SeqParameterSet::from_bits(nal.rbsp_bits())
            .map_err(|e| unsupported(format!("invalid H.264 sequence parameter set: {e:?}")))?;
        ensure!(
            sps.pixel_dimensions()
                .map_err(|e| unsupported(format!("invalid H.264 dimensions: {e:?}")))?
                == (u32::from(description.width), u32::from(description.height))
                && sps.chroma_info.chroma_format == ChromaFormat::YUV420
                && sps.chroma_info.bit_depth_luma_minus8 == sps.chroma_info.bit_depth_chroma_minus8
                && (sps.chroma_info.bit_depth_luma_minus8 == 0
                    || export_picture && sps.chroma_info.bit_depth_luma_minus8 == 2)
                && !sps.chroma_info.separate_colour_plane_flag
                && sps.frame_mbs_flags == FrameMbsFlags::Frames,
            "H.264 must be progressive 8-bit 4:2:0 with matching dimensions"
        );
        let depth = sps.chroma_info.bit_depth_luma_minus8 + 8;
        ensure!(
            bit_depth.is_none_or(|first| first == depth),
            "H.264 parameter sets declare conflicting bit depths"
        );
        bit_depth = Some(depth);
        if let Some(vui) = sps.vui_parameters {
            if let Some(ratio) = vui.aspect_ratio_info {
                if !matches!(ratio, AspectRatioInfo::Unspecified) {
                    let (x, y) = ratio
                        .get()
                        .ok_or_else(|| unsupported("reserved H.264 pixel aspect ratio"))?;
                    merge_pixel_aspect(
                        &mut pixel_aspect,
                        Some(PixelAspectRatio::new(u64::from(x), u64::from(y))?),
                    )?;
                }
            }
            if let Some(signal) = vui.video_signal_type {
                merge_video_range(&mut full_range, Some(signal.video_full_range_flag))?;
                if let Some(c) = signal.colour_description {
                    ColourDescription::merge(
                        &mut colour,
                        validate_color_for_use(
                            u16::from(c.colour_primaries),
                            u16::from(c.transfer_characteristics),
                            u16::from(c.matrix_coefficients),
                            export_picture,
                        )?,
                    )?;
                }
            }
        }
    }
    let bit_depth =
        bit_depth.ok_or_else(|| unsupported("missing H.264 sequence parameter sets"))?;
    // The record constructor bounds every PPS, but its iterators require a
    // nonempty NAL. Guard that precondition before asking it to interpret PPS
    // syntax and SPS references; successful metadata demux is not that proof.
    let pps_count = extradata[cursor];
    cursor += 1;
    ensure!(pps_count > 0, "missing H.264 picture parameter sets");
    for _ in 0..pps_count {
        let length = usize::from(u16::from_be_bytes([
            extradata[cursor],
            extradata[cursor + 1],
        ]));
        cursor += 2;
        ensure!(length > 0, "empty H.264 picture parameter set");
        cursor += length;
    }
    if cursor < extradata.len() {
        // High-profile extension fields are optional in existing records. When
        // present, require the complete bounded form and agreement with SPS.
        // Auxiliary SPS extensions have no established interpretation here.
        let extension = &extradata[cursor..];
        ensure!(
            matches!(extradata[1], 100 | 110 | 122 | 144)
                && extension.len() == 4
                && extension[0] & 3 == 1
                && extension[1] & 7 == bit_depth - 8
                && extension[2] & 7 == bit_depth - 8
                && extension[3] == 0,
            "invalid or unsupported H.264 decoder configuration extension"
        );
    }
    configuration
        .create_context()
        .map_err(|e| unsupported(format!("invalid H.264 parameter sets: {e:?}")))?;
    validate_video_range(description.full_range, full_range, bit_depth, colour)?;
    Ok((bit_depth, colour, pixel_aspect.unwrap_or_default()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ColourDescription {
    primaries: u16,
    transfer: u16,
    matrix: u16,
}
const UNSPECIFIED: u16 = 2;
const PRIMARIES: [(u16, &str); 4] = [
    (1, "BT.709"),
    (2, "unspecified"),
    (9, "BT.2020"),
    (12, "P3"),
];
const TRANSFERS: [(u16, &str); 5] = [
    (1, "BT.709"),
    (2, "unspecified"),
    (13, "sRGB"),
    (16, "PQ"),
    (18, "HLG"),
];
const MATRICES: [(u16, &str); 3] = [(1, "BT.709"), (2, "unspecified"), (9, "BT.2020nc")];
impl ColourDescription {
    pub(crate) fn codes(self) -> (u16, u16, u16) {
        (self.primaries, self.transfer, self.matrix)
    }
    pub(crate) fn merge(current: &mut Option<Self>, declared: Option<Self>) -> Result<()> {
        let Some(later) = declared else { return Ok(()) };
        let Some(first) = *current else {
            *current = Some(later);
            return Ok(());
        };
        let reconcile = |a, b| match (a, b) {
            (UNSPECIFIED, c) | (c, UNSPECIFIED) => Some(c),
            (a, b) if a == b => Some(a),
            _ => None,
        };
        let Some(((primaries, transfer), matrix)) = reconcile(first.primaries, later.primaries)
            .zip(reconcile(first.transfer, later.transfer))
            .zip(reconcile(first.matrix, later.matrix))
        else {
            return Err(unsupported(format!(
                "media declares conflicting colour metadata {first} and {later}"
            )));
        };
        *current = Some(Self {
            primaries,
            transfer,
            matrix,
        });
        Ok(())
    }
    pub(crate) fn passes_through(&self) -> bool {
        [self.primaries, self.transfer, self.matrix]
            .iter()
            .any(|c| ![1, UNSPECIFIED].contains(c))
    }
    pub(crate) fn passthrough_warning(&self) -> String {
        format!("video colour {self} passes through unchanged; display depends on the player")
    }
}
impl std::fmt::Display for ColourDescription {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = |table: &[(u16, &'static str)], code| {
            table
                .iter()
                .find(|(c, _)| *c == code)
                .map(|(_, n)| *n)
                .expect("validated colour code")
        };
        write!(
            f,
            "{}/{}/{}",
            name(&PRIMARIES, self.primaries),
            name(&TRANSFERS, self.transfer),
            name(&MATRICES, self.matrix)
        )
    }
}
pub(crate) fn validate_color(
    primaries: u16,
    transfer: u16,
    matrix: u16,
) -> Result<Option<ColourDescription>> {
    validate_color_for_use(primaries, transfer, matrix, false)
}

pub(crate) fn validate_export_color(
    primaries: u16,
    transfer: u16,
    matrix: u16,
) -> Result<Option<ColourDescription>> {
    validate_color_for_use(primaries, transfer, matrix, true)
}

fn validate_color_for_use(
    primaries: u16,
    transfer: u16,
    matrix: u16,
    export_picture: bool,
) -> Result<Option<ColourDescription>> {
    let listed = |t: &[(u16, &str)], c| t.iter().any(|(x, _)| *x == c);
    ensure!(listed(&PRIMARIES,primaries)&&listed(&TRANSFERS,transfer)&&listed(&MATRICES,matrix)
        && (transfer != 13 || export_picture && primaries == 1 && [1, 2].contains(&matrix)),
        "explicit media colour metadata {primaries}/{transfer}/{matrix} is unsupported; conversion passes through BT.709, BT.2020 PQ/HLG and P3 declarations");
    Ok((![primaries, transfer, matrix]
        .iter()
        .all(|c| *c == UNSPECIFIED))
    .then_some(ColourDescription {
        primaries,
        transfer,
        matrix,
    }))
}

fn exactly_one(boxes: &[MediaBox], kind: [u8; 4], name: &str) -> Result<Range<u64>> {
    let mut found = boxes.iter().filter(|(candidate, _)| *candidate == kind);
    let Some((_, range)) = found.next() else {
        return Err(unsupported(format!("missing MP4 {name}")));
    };
    ensure!(
        found.next().is_none(),
        "MP4 must contain one unambiguous {name}"
    );
    Ok(range.clone())
}
fn checked_add(base: u64, offset: u64) -> Result<u64> {
    base.checked_add(offset)
        .ok_or_else(|| unsupported("MP4 metadata offset overflows"))
}
fn bytes<const N: usize>(reader: &mut (impl Read + Seek), position: u64) -> Result<[u8; N]> {
    reader.seek(SeekFrom::Start(position))?;
    let mut value = [0; N];
    reader.read_exact(&mut value)?;
    Ok(value)
}
fn be_u32(r: &mut (impl Read + Seek), p: u64) -> Result<u32> {
    Ok(u32::from_be_bytes(bytes(r, p)?))
}
fn be_i32(r: &mut (impl Read + Seek), p: u64) -> Result<i32> {
    Ok(i32::from_be_bytes(bytes(r, p)?))
}
fn be_u64(r: &mut (impl Read + Seek), p: u64) -> Result<u64> {
    Ok(u64::from_be_bytes(bytes(r, p)?))
}
fn be_i64(r: &mut (impl Read + Seek), p: u64) -> Result<i64> {
    Ok(i64::from_be_bytes(bytes(r, p)?))
}
fn be_i16(r: &mut (impl Read + Seek), p: u64) -> Result<i16> {
    Ok(i16::from_be_bytes(bytes(r, p)?))
}
fn boxes(reader: &mut (impl Read + Seek), range: Range<u64>) -> Result<Vec<MediaBox>> {
    let mut cursor = range.start;
    let mut result = Vec::new();
    ensure!(cursor <= range.end, "truncated MP4 sample description");
    while cursor < range.end {
        if range.end - cursor == 4 && bytes::<4>(reader, cursor)? == [0; 4] {
            break;
        }
        ensure!(
            range.end - cursor >= 8 && result.len() < 4096,
            "invalid or excessive MP4 metadata boxes"
        );
        let header = bytes::<8>(reader, cursor)?;
        let short = u32::from_be_bytes(header[..4].try_into().expect("four bytes"));
        let (size, header_size) = match short {
            0 => (range.end - cursor, 8),
            1 if range.end - cursor >= 16 => (be_u64(reader, cursor + 8)?, 16),
            1 => return Err(unsupported("truncated extended MP4 box")),
            n => (u64::from(n), 8),
        };
        ensure!(
            size >= header_size && size <= range.end - cursor,
            "MP4 metadata box exceeds its parent"
        );
        result.push((
            header[4..].try_into().expect("four bytes"),
            cursor + header_size..cursor + size,
        ));
        cursor += size;
    }
    Ok(result)
}

fn video_orientation(
    matrix: [u8; 36],
    width: u16,
    height: u16,
) -> Result<crate::schema::VideoOrientation> {
    use crate::schema::VideoOrientation::*;
    // One and minus one in the 16.16 fixed point of the rotation entries.
    const ONE: i32 = 1 << 16;
    const MINUS_ONE: i32 = -ONE;
    let entries: [i32; 9] = std::array::from_fn(|index| {
        i32::from_be_bytes(
            matrix[index * 4..index * 4 + 4]
                .try_into()
                .expect("four bytes"),
        )
    });
    let [a, b, u, c, d, v, x, y, w] = entries;
    let width = i64::from(width) * i64::from(ONE);
    let height = i64::from(height) * i64::from(ONE);
    let not_quarter_turn =
        || unsupported("MP4 display transform must be an unmirrored quarter turn");
    if u != 0 || v != 0 || w != 1 << 30 {
        return Err(not_quarter_turn());
    }
    let (orientation, translation) = match [a, b, c, d] {
        [ONE, 0, 0, ONE] => (Identity, [0, 0]),
        [0, ONE, MINUS_ONE, 0] => (Clockwise, [height, 0]),
        [MINUS_ONE, 0, 0, MINUS_ONE] => (HalfTurn, [width, height]),
        [0, MINUS_ONE, ONE, 0] => (CounterClockwise, [0, width]),
        _ => return Err(not_quarter_turn()),
    };
    let actual = [i64::from(x), i64::from(y)];
    if actual != [0, 0] && actual != translation {
        return Err(unsupported(
            "MP4 display transform has an unsupported translation",
        ));
    }
    Ok(orientation)
}

/// Reconcile explicit range flags; absence does not assert limited range.
pub(crate) fn merge_video_range(current: &mut Option<bool>, declared: Option<bool>) -> Result<()> {
    if let Some(declared) = declared {
        if current.is_some_and(|current| current != declared) {
            return Err(unsupported(
                "media declares conflicting full-range and limited-range flags",
            ));
        }
        *current = Some(declared);
    }
    Ok(())
}

/// Reconciles the `container` range flag of the `colr` box with the
/// `bitstream` flag of the parameter sets, and checks that a full-range
/// result is supported. The decoder derives range from the bitstream, so a
/// full-range container needs an explicit full-range bitstream signal.
/// Full-range decode has independent pixel evidence only for basic 8-bit SDR.
pub(crate) fn validate_video_range(
    container: Option<bool>,
    bitstream: Option<bool>,
    bit_depth: u8,
    colour: Option<ColourDescription>,
) -> Result<()> {
    if container == Some(true) && bitstream != Some(true) {
        return Err(unsupported(
            "full-range container requires an explicit coherent full-range bitstream signal",
        ));
    }
    let mut full_range = bitstream;
    merge_video_range(&mut full_range, container)?;
    if full_range == Some(true)
        && (bit_depth != 8 || colour.is_some_and(|colour| colour.passes_through()))
    {
        return Err(unsupported(
            "full-range video requires supported 8-bit BT.709 or unspecified SDR colour",
        ));
    }
    Ok(())
}
