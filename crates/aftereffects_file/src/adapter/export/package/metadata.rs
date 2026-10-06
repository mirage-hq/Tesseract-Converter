//! Bounded metadata extraction for the source-backed QuickTime package profile.

use std::io::{self, Read, Seek, SeekFrom};

use crate::{
    media::MediaDuration,
    writer::footage::{NativeFrameRate, SOURCE_TICKS_PER_SECOND},
};

mod mp4;
pub(super) use mp4::from_reader as mp4_from_reader;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum MetadataError {
    /// The bytes are structurally valid, but their native interpretation is
    /// outside the deliberately narrow profile emitted by this converter.
    Unsupported(&'static str),
    /// Required structure is absent, contradictory, or out of bounds.
    Malformed(&'static str),
}

#[derive(Debug)]
pub(super) enum MetadataReadError {
    Profile(MetadataError),
    Io(io::Error),
}

impl From<MetadataError> for MetadataReadError {
    fn from(error: MetadataError) -> Self {
        Self::Profile(error)
    }
}

impl From<io::Error> for MetadataReadError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct QuickTimeMetadata {
    pub(super) dimensions: [u16; 2],
    pub(super) duration_millis: u64,
    pub(super) duration_millis_floor: u64,
    pub(super) duration_native_ticks: Option<u64>,
    pub(super) frame_rate: NativeFrameRate,
    pub(super) native_duration: Option<MediaDuration>,
    pub(super) video_codec: [u8; 4],
    pub(super) audio_sample_rate: f64,
}

#[derive(Clone, Copy)]
struct Atom<'a> {
    kind: [u8; 4],
    payload: &'a [u8],
}

struct AtomBudget {
    remaining: usize,
}

#[derive(Clone, Copy)]
struct MovieHeader {
    timescale: u32,
    duration: u64,
}

#[derive(Clone, Copy)]
struct TrackHeader {
    duration: u64,
    dimensions: [u16; 2],
}

#[derive(Clone, Copy)]
struct MediaHeader {
    timescale: u32,
    duration: u64,
}

#[derive(Clone, Copy)]
struct Timing {
    sample_count: u32,
    sample_delta: u32,
    duration: u64,
}

#[derive(Clone, Copy)]
struct VideoTrack {
    dimensions: [u16; 2],
    duration: u64,
    timescale: u32,
    frame_rate: NativeFrameRate,
    native_duration: Option<MediaDuration>,
    codec: [u8; 4],
}

#[derive(Clone, Copy)]
struct AudioTrack {
    duration: u64,
    timescale: u32,
    sample_rate: u32,
}

pub(super) fn quicktime_from_reader(
    reader: &mut (impl Read + Seek),
    file_length: u64,
) -> Result<QuickTimeMetadata, MetadataReadError> {
    let retained = container_metadata_from_reader(reader, file_length)?;
    quicktime_bytes_with_budget(&retained, usize::MAX).map_err(Into::into)
}

// Shared streaming atom reader: retain only container metadata, seek over media.
// MOV and MP4 validation use the same structural bounds and no resource quotas.
fn container_metadata_from_reader(
    reader: &mut (impl Read + Seek),
    file_length: u64,
) -> Result<Vec<u8>, MetadataReadError> {
    let mut cursor = 0_u64;
    let mut metadata_bytes = 0_u64;
    let mut retained = Vec::new();
    while cursor < file_length {
        let header_end = checked_end(cursor, 8, file_length, "QuickTime atom header is truncated")?;
        let mut header = [0_u8; 16];
        read_exact_at(reader, cursor, &mut header[..8])?;
        charge_metadata(&mut metadata_bytes, 8)?;
        let short_size = u32::from_be_bytes(
            header[0..4]
                .try_into()
                .map_err(|_| MetadataError::Malformed("QuickTime atom size is malformed"))?,
        );
        let kind: [u8; 4] = header[4..8]
            .try_into()
            .map_err(|_| MetadataError::Malformed("QuickTime atom type is malformed"))?;
        let (header_size, atom_size) = match short_size {
            0 => (8_u64, file_length - cursor),
            1 => {
                checked_end(
                    header_end,
                    8,
                    file_length,
                    "QuickTime extended atom header is truncated",
                )?;
                read_exact_at(reader, header_end, &mut header[8..16])?;
                charge_metadata(&mut metadata_bytes, 8)?;
                let size = u64::from_be_bytes(header[8..16].try_into().map_err(|_| {
                    MetadataError::Malformed("QuickTime extended atom size is malformed")
                })?);
                (16, size)
            }
            value => (8, u64::from(value)),
        };
        if atom_size < header_size {
            return Err(
                MetadataError::Malformed("QuickTime atom is smaller than its header").into(),
            );
        }
        let atom_end = checked_end(
            cursor,
            atom_size,
            file_length,
            "QuickTime atom extends beyond the file",
        )?;
        if kind == *b"ftyp" || kind == *b"moov" {
            let payload_length = atom_size - header_size;
            charge_metadata(&mut metadata_bytes, payload_length)?;
            retained.extend_from_slice(
                &header[..usize::try_from(header_size).map_err(|_| {
                    MetadataError::Malformed("QuickTime atom header size is too large")
                })?],
            );
            let old_length = retained.len();
            let payload_length = usize::try_from(payload_length)
                .map_err(|_| MetadataError::Malformed("QuickTime atom payload is too large"))?;
            let retained_length =
                old_length
                    .checked_add(payload_length)
                    .ok_or(MetadataError::Malformed(
                        "QuickTime retained metadata size overflows",
                    ))?;
            retained.resize(retained_length, 0);
            read_exact_at(reader, cursor + header_size, &mut retained[old_length..])?;
        }
        cursor = atom_end;
    }
    Ok(retained)
}

#[cfg(test)]
fn quicktime(bytes: &[u8]) -> Result<QuickTimeMetadata, MetadataError> {
    quicktime_bytes_with_budget(bytes, usize::MAX)
}

fn quicktime_bytes_with_budget(
    bytes: &[u8],
    remaining_atoms: usize,
) -> Result<QuickTimeMetadata, MetadataError> {
    let mut budget = AtomBudget {
        remaining: remaining_atoms,
    };
    let root = atoms(bytes, &mut budget)?;
    let mut file_types = root.iter().filter(|atom| atom.kind == *b"ftyp");
    let file_type = file_types
        .next()
        .copied()
        .ok_or(MetadataError::Unsupported(
            "movie does not declare the required QuickTime qt brand",
        ))?;
    if file_types.next().is_some() {
        return Err(MetadataError::Malformed(
            "QuickTime file has duplicate ftyp atoms",
        ));
    }
    require_quicktime_brand(file_type.payload)?;
    let movie = exactly_one(&root, *b"moov", "QuickTime file has no unique moov atom")?;
    let movie_atoms = atoms(movie.payload, &mut budget)?;
    let movie_header = parse_movie_header(
        exactly_one(
            &movie_atoms,
            *b"mvhd",
            "QuickTime movie has no unique mvhd atom",
        )?
        .payload,
    )?;

    let tracks: Vec<_> = movie_atoms
        .iter()
        .filter(|atom| atom.kind == *b"trak")
        .copied()
        .collect();
    if tracks.is_empty() {
        return Err(MetadataError::Malformed(
            "QuickTime movie has an invalid track count",
        ));
    }

    let mut video = None;
    let mut audio = None;
    let mut timecode = false;
    for track in tracks {
        match parse_track(track.payload, movie_header, &mut budget)? {
            ParsedTrack::Video(value) if video.replace(value).is_some() => {
                return Err(MetadataError::Unsupported(
                    "QuickTime profile has more than one video track",
                ));
            }
            ParsedTrack::Audio(value) if audio.replace(value).is_some() => {
                return Err(MetadataError::Unsupported(
                    "QuickTime profile has more than one audio track",
                ));
            }
            ParsedTrack::Timecode if timecode => {
                return Err(MetadataError::Unsupported(
                    "QuickTime profile has more than one timecode track",
                ));
            }
            ParsedTrack::Timecode => timecode = true,
            ParsedTrack::Video(_) | ParsedTrack::Audio(_) => {}
        }
    }
    let video = video.ok_or(MetadataError::Unsupported(
        "QuickTime profile has no supported video track",
    ))?;
    if let Some(audio) = audio {
        let video_duration = u128::from(video.duration)
            .checked_mul(u128::from(audio.timescale))
            .ok_or(MetadataError::Malformed("QuickTime duration overflows"))?;
        let audio_duration = u128::from(audio.duration)
            .checked_mul(u128::from(video.timescale))
            .ok_or(MetadataError::Malformed("QuickTime duration overflows"))?;
        if video_duration != audio_duration {
            return Err(MetadataError::Unsupported(
                "QuickTime audio and video track durations differ",
            ));
        }
    }

    let duration_numerator = video
        .duration
        .checked_mul(1_000)
        .ok_or(MetadataError::Malformed("QuickTime duration overflows"))?;
    let duration_millis_floor = duration_numerator / u64::from(video.timescale);
    let duration_millis = duration_numerator.div_ceil(u64::from(video.timescale));
    if duration_millis == 0 {
        return Err(MetadataError::Malformed(
            "QuickTime video duration is empty",
        ));
    }
    Ok(QuickTimeMetadata {
        dimensions: video.dimensions,
        duration_millis,
        duration_millis_floor,
        duration_native_ticks: {
            let numerator = u128::from(video.duration) * u128::from(SOURCE_TICKS_PER_SECOND);
            let denominator = u128::from(video.timescale);
            numerator
                .is_multiple_of(denominator)
                .then(|| u64::try_from(numerator / denominator).ok())
                .flatten()
        },
        frame_rate: video.frame_rate,
        native_duration: video.native_duration,
        video_codec: video.codec,
        audio_sample_rate: audio.map_or(0.0, |track| f64::from(track.sample_rate)),
    })
}

enum ParsedTrack {
    Video(VideoTrack),
    Audio(AudioTrack),
    Timecode,
}

mod ancillary;
mod presentation;

fn parse_track(
    payload: &[u8],
    movie: MovieHeader,
    budget: &mut AtomBudget,
) -> Result<ParsedTrack, MetadataError> {
    let track_atoms = atoms(payload, budget)?;
    let edits = optional_one(
        &track_atoms,
        *b"edts",
        "QuickTime track has duplicate edts atoms",
    )?;
    let track_header = parse_track_header(
        exactly_one(
            &track_atoms,
            *b"tkhd",
            "QuickTime track has no unique tkhd atom",
        )?
        .payload,
    )?;
    let media = exactly_one(
        &track_atoms,
        *b"mdia",
        "QuickTime track has no unique mdia atom",
    )?;
    let media_atoms = atoms(media.payload, budget)?;
    let media_header = parse_media_header(
        exactly_one(
            &media_atoms,
            *b"mdhd",
            "QuickTime media has no unique mdhd atom",
        )?
        .payload,
    )?;
    require_track_duration(track_header.duration, movie, media_header)?;
    let handler = parse_handler(
        exactly_one(
            &media_atoms,
            *b"hdlr",
            "QuickTime media has no unique hdlr atom",
        )?
        .payload,
    )?;
    let media_info = exactly_one(
        &media_atoms,
        *b"minf",
        "QuickTime media has no unique minf atom",
    )?;
    let media_info_atoms = atoms(media_info.payload, budget)?;
    let sample_table = exactly_one(
        &media_info_atoms,
        *b"stbl",
        "QuickTime media has no unique stbl atom",
    )?;
    let sample_atoms = atoms(sample_table.payload, budget)?;
    let timing = parse_timing(
        exactly_one(
            &sample_atoms,
            *b"stts",
            "QuickTime media has no unique stts atom",
        )?
        .payload,
    )?;
    if timing.duration != media_header.duration {
        return Err(MetadataError::Malformed(
            "QuickTime stts and mdhd durations disagree",
        ));
    }
    let presentation_origin = if handler == *b"vide" {
        presentation::origin(&sample_atoms, timing)?
    } else {
        0
    };
    if let Some(edits) = edits {
        require_identity_edit(
            edits.payload,
            track_header.duration,
            presentation_origin,
            budget,
        )?;
    } else if presentation_origin != 0 {
        return Err(MetadataError::Unsupported(
            "QuickTime nonzero presentation origin requires a full-span identity edit",
        ));
    }
    let descriptions = exactly_one(
        &sample_atoms,
        *b"stsd",
        "QuickTime media has no unique stsd atom",
    )?;

    match handler {
        [b'v', b'i', b'd', b'e'] => {
            let (dimensions, codec) = parse_video_description(descriptions.payload, budget)?;
            if track_header.dimensions != dimensions {
                return Err(MetadataError::Unsupported(
                    "QuickTime encoded and display dimensions differ",
                ));
            }
            if timing.sample_count == 0 {
                return Err(MetadataError::Malformed("QuickTime video timing is empty"));
            }
            let frame_rate = native_frame_rate(media_header.timescale, timing.sample_delta)?;
            Ok(ParsedTrack::Video(VideoTrack {
                dimensions,
                duration: timing.duration,
                timescale: media_header.timescale,
                frame_rate,
                native_duration: if is_ntsc_2997(media_header.timescale, timing.sample_delta) {
                    // AE interprets this exact source ratio as 29.97 fps. The
                    // independent native fixture stores samples / 29.97, not
                    // MOV duration or a clock reconstructed from rounded ms.
                    Some(MediaDuration {
                        numerator: timing.sample_count.checked_mul(100).ok_or(
                            MetadataError::Unsupported(
                                "QuickTime native duration exceeds the native range",
                            ),
                        )?,
                        denominator: 2997,
                    })
                } else {
                    None
                },
                codec,
            }))
        }
        [b's', b'o', b'u', b'n'] => {
            if track_header.dimensions != [0, 0] {
                return Err(MetadataError::Malformed(
                    "QuickTime audio track has visual dimensions",
                ));
            }
            let sample_rate = parse_audio_description(descriptions.payload, budget)?;
            if sample_rate != media_header.timescale {
                return Err(MetadataError::Unsupported(
                    "QuickTime audio sample rate and media timescale differ",
                ));
            }
            Ok(ParsedTrack::Audio(AudioTrack {
                duration: timing.duration,
                timescale: media_header.timescale,
                sample_rate,
            }))
        }
        [b't', b'm', b'c', b'd'] => {
            if track_header.dimensions != [0, 0] {
                return Err(MetadataError::Malformed(
                    "QuickTime timecode track has visual dimensions",
                ));
            }
            ancillary::require_timecode_description(
                descriptions.payload,
                media_header,
                timing,
                budget,
            )?;
            Ok(ParsedTrack::Timecode)
        }
        _ => Err(MetadataError::Unsupported(
            "QuickTime profile contains an unknown track handler",
        )),
    }
}

fn require_identity_edit(
    payload: &[u8],
    track_duration: u64,
    presentation_origin: u64,
    budget: &mut AtomBudget,
) -> Result<(), MetadataError> {
    const UNSUPPORTED: &str =
        "QuickTime edit lists are not representable by the native source profile";
    let edits = atoms(payload, budget)?;
    if edits.len() != 1 || edits[0].kind != *b"elst" {
        return Err(MetadataError::Unsupported(UNSUPPORTED));
    }
    let list = edits[0].payload;
    if list.len() < 8 {
        return Err(MetadataError::Malformed(
            "QuickTime edit list header is truncated",
        ));
    }
    if list[..4] != [0; 4] || read_u32(list, 4, "QuickTime edit count is truncated")? != 1 {
        return Err(MetadataError::Unsupported(UNSUPPORTED));
    }
    if list.len() != 20 {
        return Err(MetadataError::Malformed(
            "QuickTime edit entry length is invalid",
        ));
    }
    let duration = u64::from(read_u32(list, 8, "QuickTime edit duration is truncated")?);
    let media_time = read_i32(list, 12, "QuickTime edit media time is truncated")?;
    let rate = read_u32(list, 16, "QuickTime edit rate is truncated")?;
    // A B-frame track may start its complete presentation grid after decode
    // time zero. Its unit-rate edit removes that origin, not authored footage.
    if duration != track_duration
        || u64::try_from(media_time).ok() != Some(presentation_origin)
        || rate != 0x0001_0000
    {
        return Err(MetadataError::Unsupported(UNSUPPORTED));
    }
    Ok(())
}

fn is_ntsc_2997(timescale: u32, sample_delta: u32) -> bool {
    u64::from(timescale) * 1001 == u64::from(sample_delta) * 30_000
}

fn native_frame_rate(timescale: u32, sample_delta: u32) -> Result<NativeFrameRate, MetadataError> {
    const FRACTION_SCALE: u64 = 1 << 16;

    if timescale == 0 || sample_delta == 0 {
        return Err(MetadataError::Malformed("QuickTime video timing is empty"));
    }
    if is_ntsc_2997(timescale, sample_delta) {
        // Independently observed AE26.5x89 encoding, not nearest 16.16
        // rounding of 30000/1001; no other NTSC ratios are inferred.
        return Ok(NativeFrameRate {
            integer: 29,
            fractional: 63570,
        });
    }
    let scaled_numerator = u64::from(timescale) * FRACTION_SCALE;
    let denominator = u64::from(sample_delta);
    if !scaled_numerator.is_multiple_of(denominator) {
        return Err(MetadataError::Unsupported(
            "QuickTime video frame rate cannot be represented exactly",
        ));
    }
    let fixed_rate = scaled_numerator / denominator;
    let integer = u32::try_from(fixed_rate / FRACTION_SCALE).map_err(|_| {
        MetadataError::Unsupported("QuickTime video frame rate exceeds the native range")
    })?;
    let fractional = u16::try_from(fixed_rate % FRACTION_SCALE).map_err(|_| {
        MetadataError::Unsupported("QuickTime video frame rate exceeds the native range")
    })?;
    Ok(NativeFrameRate {
        integer,
        fractional,
    })
}

fn atoms<'a>(bytes: &'a [u8], budget: &mut AtomBudget) -> Result<Vec<Atom<'a>>, MetadataError> {
    let mut result = Vec::new();
    let mut cursor = 0_usize;
    while cursor < bytes.len() {
        budget.remaining = budget
            .remaining
            .checked_sub(1)
            .ok_or(MetadataError::Malformed(
                "QuickTime atom count overflows host address space",
            ))?;
        let header = bytes
            .get(
                cursor
                    ..cursor
                        .checked_add(8)
                        .ok_or(MetadataError::Malformed("QuickTime atom offset overflows"))?,
            )
            .ok_or(MetadataError::Malformed(
                "QuickTime atom header is truncated",
            ))?;
        let short_size = u32::from_be_bytes(
            header[0..4]
                .try_into()
                .map_err(|_| MetadataError::Malformed("QuickTime atom size is malformed"))?,
        );
        let kind: [u8; 4] = header[4..8]
            .try_into()
            .map_err(|_| MetadataError::Malformed("QuickTime atom type is malformed"))?;
        let (header_size, atom_size) = match short_size {
            0 => (8_usize, bytes.len() - cursor),
            1 => {
                let extended = bytes
                    .get(
                        cursor + 8
                            ..cursor.checked_add(16).ok_or(MetadataError::Malformed(
                                "QuickTime extended atom offset overflows",
                            ))?,
                    )
                    .ok_or(MetadataError::Malformed(
                        "QuickTime extended atom header is truncated",
                    ))?;
                let size = u64::from_be_bytes(extended.try_into().map_err(|_| {
                    MetadataError::Malformed("QuickTime extended atom size is malformed")
                })?);
                let size = usize::try_from(size).map_err(|_| {
                    MetadataError::Malformed("QuickTime extended atom size is too large")
                })?;
                (16, size)
            }
            value => (
                8,
                usize::try_from(value)
                    .map_err(|_| MetadataError::Malformed("QuickTime atom size is too large"))?,
            ),
        };
        if atom_size < header_size {
            return Err(MetadataError::Malformed(
                "QuickTime atom is smaller than its header",
            ));
        }
        let end = cursor
            .checked_add(atom_size)
            .ok_or(MetadataError::Malformed("QuickTime atom end overflows"))?;
        if end > bytes.len() {
            return Err(MetadataError::Malformed(
                "QuickTime atom extends beyond its parent",
            ));
        }
        let payload_start = cursor
            .checked_add(header_size)
            .ok_or(MetadataError::Malformed("QuickTime atom payload overflows"))?;
        result.push(Atom {
            kind,
            payload: &bytes[payload_start..end],
        });
        cursor = end;
    }
    Ok(result)
}

fn exactly_one<'data>(
    atoms: &[Atom<'data>],
    kind: [u8; 4],
    error: &'static str,
) -> Result<Atom<'data>, MetadataError> {
    let mut matches = atoms.iter().filter(|atom| atom.kind == kind);
    let value = matches
        .next()
        .copied()
        .ok_or(MetadataError::Malformed(error))?;
    if matches.next().is_some() {
        return Err(MetadataError::Malformed(error));
    }
    Ok(value)
}

fn require_quicktime_brand(payload: &[u8]) -> Result<(), MetadataError> {
    if payload.len() < 8 || !(payload.len() - 8).is_multiple_of(4) {
        return Err(MetadataError::Malformed("QuickTime ftyp atom is malformed"));
    }
    let major = payload
        .get(0..4)
        .ok_or(MetadataError::Malformed("QuickTime ftyp atom is truncated"))?;
    let compatible = payload[8..].chunks_exact(4);
    if major != b"qt  " && !compatible.into_iter().any(|brand| brand == b"qt  ") {
        return Err(MetadataError::Unsupported(
            "movie bytes do not declare the QuickTime qt brand",
        ));
    }
    Ok(())
}

fn parse_movie_header(payload: &[u8]) -> Result<MovieHeader, MetadataError> {
    let version = *payload
        .first()
        .ok_or(MetadataError::Malformed("QuickTime mvhd atom is truncated"))?;
    let (timescale_offset, duration_offset) = match version {
        0 => (12, 16),
        1 => (20, 24),
        _ => {
            return Err(MetadataError::Unsupported(
                "QuickTime mvhd version is unsupported",
            ));
        }
    };
    let timescale = read_u32(
        payload,
        timescale_offset,
        "QuickTime mvhd atom is truncated",
    )?;
    let duration = if version == 0 {
        u64::from(read_u32(
            payload,
            duration_offset,
            "QuickTime mvhd duration is truncated",
        )?)
    } else {
        read_u64(
            payload,
            duration_offset,
            "QuickTime mvhd duration is truncated",
        )?
    };
    if timescale == 0 || duration == 0 {
        return Err(MetadataError::Malformed("QuickTime movie timing is empty"));
    }
    Ok(MovieHeader {
        timescale,
        duration,
    })
}

fn parse_track_header(payload: &[u8]) -> Result<TrackHeader, MetadataError> {
    let version = *payload
        .first()
        .ok_or(MetadataError::Malformed("QuickTime tkhd atom is truncated"))?;
    let flags = read_u24(payload, 1, "QuickTime tkhd flags are truncated")?;
    if flags & 0x03 != 0x03 {
        return Err(MetadataError::Unsupported(
            "QuickTime track is not enabled in the movie",
        ));
    }
    let (duration_offset, matrix_offset, width_offset): (usize, usize, usize) = match version {
        0 => (20, 40, 76),
        1 => (28, 52, 88),
        _ => {
            return Err(MetadataError::Unsupported(
                "QuickTime tkhd version is unsupported",
            ));
        }
    };
    let duration = if version == 0 {
        u64::from(read_u32(
            payload,
            duration_offset,
            "QuickTime tkhd duration is truncated",
        )?)
    } else {
        read_u64(
            payload,
            duration_offset,
            "QuickTime tkhd duration is truncated",
        )?
    };
    let expected_matrix = [0x0001_0000_u32, 0, 0, 0, 0x0001_0000, 0, 0, 0, 0x4000_0000];
    for (index, expected) in expected_matrix.into_iter().enumerate() {
        let offset = matrix_offset
            .checked_add(index.checked_mul(4).ok_or(MetadataError::Malformed(
                "QuickTime matrix offset overflows",
            ))?)
            .ok_or(MetadataError::Malformed(
                "QuickTime matrix offset overflows",
            ))?;
        if read_u32(payload, offset, "QuickTime tkhd matrix is truncated")? != expected {
            return Err(MetadataError::Unsupported(
                "QuickTime rotated or transformed display matrices are unsupported",
            ));
        }
    }
    let width = fixed_dimension(payload, width_offset)?;
    let height = fixed_dimension(payload, width_offset + 4)?;
    Ok(TrackHeader {
        duration,
        dimensions: [width, height],
    })
}

fn parse_media_header(payload: &[u8]) -> Result<MediaHeader, MetadataError> {
    let version = *payload
        .first()
        .ok_or(MetadataError::Malformed("QuickTime mdhd atom is truncated"))?;
    let (timescale_offset, duration_offset) = match version {
        0 => (12, 16),
        1 => (20, 24),
        _ => {
            return Err(MetadataError::Unsupported(
                "QuickTime mdhd version is unsupported",
            ));
        }
    };
    let timescale = read_u32(
        payload,
        timescale_offset,
        "QuickTime mdhd atom is truncated",
    )?;
    let duration = if version == 0 {
        u64::from(read_u32(
            payload,
            duration_offset,
            "QuickTime mdhd duration is truncated",
        )?)
    } else {
        read_u64(
            payload,
            duration_offset,
            "QuickTime mdhd duration is truncated",
        )?
    };
    if timescale == 0 || duration == 0 {
        return Err(MetadataError::Malformed("QuickTime media timing is empty"));
    }
    Ok(MediaHeader {
        timescale,
        duration,
    })
}

fn require_track_duration(
    track_duration: u64,
    movie: MovieHeader,
    media: MediaHeader,
) -> Result<(), MetadataError> {
    if track_duration == 0 {
        return Err(MetadataError::Malformed(
            "QuickTime track duration is empty",
        ));
    }
    if track_duration != movie.duration {
        return Err(MetadataError::Unsupported(
            "QuickTime track and movie durations differ without an edit list",
        ));
    }
    // A whole media duration may require a partial movie tick. Only its exact
    // upward integer representation is admissible, not a floating tolerance.
    let media_duration = u128::from(media.duration)
        .checked_mul(u128::from(movie.timescale))
        .ok_or(MetadataError::Malformed("QuickTime duration overflows"))?
        .div_ceil(u128::from(media.timescale));
    if u128::from(track_duration) != media_duration {
        return Err(MetadataError::Unsupported(
            "QuickTime track and media durations differ without an edit list",
        ));
    }
    Ok(())
}

fn parse_handler(payload: &[u8]) -> Result<[u8; 4], MetadataError> {
    if payload.first().copied() != Some(0) {
        return Err(MetadataError::Unsupported(
            "QuickTime hdlr version is unsupported",
        ));
    }
    payload
        .get(8..12)
        .ok_or(MetadataError::Malformed("QuickTime hdlr atom is truncated"))?
        .try_into()
        .map_err(|_| MetadataError::Malformed("QuickTime handler type is malformed"))
}

fn parse_timing(payload: &[u8]) -> Result<Timing, MetadataError> {
    if payload.get(0..4) != Some(&[0, 0, 0, 0]) {
        return Err(MetadataError::Unsupported(
            "QuickTime stts version or flags are unsupported",
        ));
    }
    let count = read_u32(payload, 4, "QuickTime stts atom is truncated")?;
    if count == 0 {
        return Err(MetadataError::Malformed(
            "QuickTime stts entry count is invalid",
        ));
    }
    let entries_len = usize::try_from(count)
        .ok()
        .and_then(|value| value.checked_mul(8))
        .ok_or(MetadataError::Malformed(
            "QuickTime stts entry bytes overflow",
        ))?;
    let expected_len = 8_usize
        .checked_add(entries_len)
        .ok_or(MetadataError::Malformed("QuickTime stts length overflows"))?;
    if payload.len() != expected_len {
        return Err(MetadataError::Malformed(
            "QuickTime stts entry count does not match its payload",
        ));
    }
    if count != 1 {
        return Err(MetadataError::Unsupported(
            "QuickTime variable-rate sample timing is unsupported",
        ));
    }
    let sample_count = read_u32(payload, 8, "QuickTime stts sample count is truncated")?;
    let sample_delta = read_u32(payload, 12, "QuickTime stts sample delta is truncated")?;
    if sample_count == 0 || sample_delta == 0 {
        return Err(MetadataError::Malformed("QuickTime stts timing is empty"));
    }
    let duration = u64::from(sample_count)
        .checked_mul(u64::from(sample_delta))
        .ok_or(MetadataError::Malformed(
            "QuickTime stts duration overflows",
        ))?;
    Ok(Timing {
        sample_count,
        sample_delta,
        duration,
    })
}

fn parse_video_description(
    payload: &[u8],
    budget: &mut AtomBudget,
) -> Result<([u16; 2], [u8; 4]), MetadataError> {
    let entry = sample_description(payload, budget)?;
    // These sample entries are already produced/consumed by this repository's
    // source-media paths. They only gate the container profile; the AEP record
    // continues to use the independently pinned `MOoV` native source code.
    if entry.kind != *b"avc1" && entry.kind != *b"ap4h" {
        return Err(MetadataError::Unsupported(
            "QuickTime video sample-entry codec is outside the supported native profile",
        ));
    }
    if entry.payload.len() < 78 {
        return Err(MetadataError::Malformed(
            "QuickTime visual sample entry is truncated",
        ));
    }
    let width = read_u16(
        entry.payload,
        24,
        "QuickTime visual sample width is truncated",
    )?;
    let height = read_u16(
        entry.payload,
        26,
        "QuickTime visual sample height is truncated",
    )?;
    if width == 0 || height == 0 {
        return Err(MetadataError::Malformed(
            "QuickTime visual sample dimensions are empty",
        ));
    }
    let children = atoms(&entry.payload[78..], budget)?;
    if let Some(pixel_aspect) = optional_one(
        &children,
        *b"pasp",
        "QuickTime visual sample has duplicate pasp atoms",
    )? {
        require_square_pixels(pixel_aspect.payload)?;
    }
    if let Some(clean_aperture) = optional_one(
        &children,
        *b"clap",
        "QuickTime visual sample has duplicate clap atoms",
    )? {
        require_full_clean_aperture(clean_aperture.payload, [width, height])?;
    }
    Ok(([width, height], entry.kind))
}

fn parse_audio_description(payload: &[u8], budget: &mut AtomBudget) -> Result<u32, MetadataError> {
    let entry = sample_description(payload, budget)?;
    let pcm = matches!(&entry.kind, b"sowt" | b"twos");
    if entry.kind != *b"mp4a" && !pcm {
        return Err(MetadataError::Unsupported(
            "QuickTime audio sample-entry codec is outside the supported native profile",
        ));
    }
    if entry.payload.len() < 28 {
        return Err(MetadataError::Malformed(
            "QuickTime audio sample entry is truncated",
        ));
    }
    let version = read_u16(
        entry.payload,
        8,
        "QuickTime audio sample-entry version is truncated",
    )?;
    if version != 0 && !(pcm && version == 1) {
        return Err(MetadataError::Unsupported(
            "QuickTime extended audio sample entries are unsupported",
        ));
    }
    let channels = read_u16(
        entry.payload,
        16,
        "QuickTime audio channel count is truncated",
    )?;
    if !matches!(channels, 1 | 2) {
        return Err(MetadataError::Unsupported(
            "QuickTime audio must have an explicit mono or stereo layout",
        ));
    }
    if pcm {
        ancillary::require_pcm_description(entry.payload, version, channels, budget)?;
    }
    let sample_rate_fixed = read_u32(
        entry.payload,
        24,
        "QuickTime audio sample rate is truncated",
    )?;
    if sample_rate_fixed & 0xffff != 0 {
        return Err(MetadataError::Unsupported(
            "QuickTime fractional audio sample rates are unsupported",
        ));
    }
    let sample_rate = sample_rate_fixed >> 16;
    if sample_rate == 0 {
        return Err(MetadataError::Malformed(
            "QuickTime audio sample rate is zero",
        ));
    }
    Ok(sample_rate)
}

fn sample_description<'a>(
    payload: &'a [u8],
    budget: &mut AtomBudget,
) -> Result<Atom<'a>, MetadataError> {
    if payload.get(0..4) != Some(&[0, 0, 0, 0]) {
        return Err(MetadataError::Unsupported(
            "QuickTime stsd version or flags are unsupported",
        ));
    }
    let count = read_u32(payload, 4, "QuickTime stsd atom is truncated")?;
    if count == 0 {
        return Err(MetadataError::Malformed(
            "QuickTime stsd entry count is invalid",
        ));
    }
    let entries = atoms(
        payload
            .get(8..)
            .ok_or(MetadataError::Malformed("QuickTime stsd atom is truncated"))?,
        budget,
    )?;
    if usize::try_from(count).ok() != Some(entries.len()) {
        return Err(MetadataError::Malformed(
            "QuickTime stsd entry count does not match its payload",
        ));
    }
    if entries.len() != 1 {
        return Err(MetadataError::Unsupported(
            "QuickTime media must use exactly one sample description",
        ));
    }
    Ok(entries[0])
}

fn optional_one<'data>(
    atoms: &[Atom<'data>],
    kind: [u8; 4],
    duplicate_error: &'static str,
) -> Result<Option<Atom<'data>>, MetadataError> {
    let mut matches = atoms.iter().filter(|atom| atom.kind == kind);
    let value = matches.next().copied();
    if matches.next().is_some() {
        return Err(MetadataError::Malformed(duplicate_error));
    }
    Ok(value)
}

fn require_square_pixels(payload: &[u8]) -> Result<(), MetadataError> {
    if payload.len() != 8 {
        return Err(MetadataError::Malformed("QuickTime pasp atom is malformed"));
    }
    let horizontal = read_u32(payload, 0, "QuickTime pasp atom is truncated")?;
    let vertical = read_u32(payload, 4, "QuickTime pasp atom is truncated")?;
    if horizontal == 0 || vertical == 0 {
        return Err(MetadataError::Malformed(
            "QuickTime pixel aspect ratio is zero",
        ));
    }
    if horizontal != vertical {
        return Err(MetadataError::Unsupported(
            "QuickTime non-square pixels are unsupported",
        ));
    }
    Ok(())
}

fn require_full_clean_aperture(payload: &[u8], dimensions: [u16; 2]) -> Result<(), MetadataError> {
    if payload.len() != 32 {
        return Err(MetadataError::Malformed("QuickTime clap atom is malformed"));
    }
    let width_n = read_u32(payload, 0, "QuickTime clap atom is truncated")?;
    let width_d = read_u32(payload, 4, "QuickTime clap atom is truncated")?;
    let height_n = read_u32(payload, 8, "QuickTime clap atom is truncated")?;
    let height_d = read_u32(payload, 12, "QuickTime clap atom is truncated")?;
    let horizontal_n = read_i32(payload, 16, "QuickTime clap atom is truncated")?;
    let horizontal_d = read_u32(payload, 20, "QuickTime clap atom is truncated")?;
    let vertical_n = read_i32(payload, 24, "QuickTime clap atom is truncated")?;
    let vertical_d = read_u32(payload, 28, "QuickTime clap atom is truncated")?;
    if width_d == 0 || height_d == 0 || horizontal_d == 0 || vertical_d == 0 {
        return Err(MetadataError::Malformed(
            "QuickTime clean-aperture denominator is zero",
        ));
    }
    let full_width = u64::from(dimensions[0])
        .checked_mul(u64::from(width_d))
        .ok_or(MetadataError::Malformed(
            "QuickTime clean-aperture width overflows",
        ))?;
    let full_height = u64::from(dimensions[1])
        .checked_mul(u64::from(height_d))
        .ok_or(MetadataError::Malformed(
            "QuickTime clean-aperture height overflows",
        ))?;
    if u64::from(width_n) != full_width
        || u64::from(height_n) != full_height
        || horizontal_n != 0
        || vertical_n != 0
    {
        return Err(MetadataError::Unsupported(
            "QuickTime cropped or offset clean apertures are unsupported",
        ));
    }
    Ok(())
}

fn fixed_dimension(payload: &[u8], offset: usize) -> Result<u16, MetadataError> {
    let fixed = read_u32(payload, offset, "QuickTime tkhd dimensions are truncated")?;
    if fixed & 0xffff != 0 {
        return Err(MetadataError::Unsupported(
            "QuickTime fractional display dimensions are unsupported",
        ));
    }
    u16::try_from(fixed >> 16)
        .map_err(|_| MetadataError::Unsupported("QuickTime display dimensions exceed AEP limits"))
}

fn read_u16(bytes: &[u8], offset: usize, error: &'static str) -> Result<u16, MetadataError> {
    let end = offset
        .checked_add(2)
        .ok_or(MetadataError::Malformed(error))?;
    let value = bytes
        .get(offset..end)
        .ok_or(MetadataError::Malformed(error))?;
    Ok(u16::from_be_bytes(
        value
            .try_into()
            .map_err(|_| MetadataError::Malformed(error))?,
    ))
}

fn read_u24(bytes: &[u8], offset: usize, error: &'static str) -> Result<u32, MetadataError> {
    let end = offset
        .checked_add(3)
        .ok_or(MetadataError::Malformed(error))?;
    let value = bytes
        .get(offset..end)
        .ok_or(MetadataError::Malformed(error))?;
    Ok((u32::from(value[0]) << 16) | (u32::from(value[1]) << 8) | u32::from(value[2]))
}

fn read_u32(bytes: &[u8], offset: usize, error: &'static str) -> Result<u32, MetadataError> {
    let end = offset
        .checked_add(4)
        .ok_or(MetadataError::Malformed(error))?;
    let value = bytes
        .get(offset..end)
        .ok_or(MetadataError::Malformed(error))?;
    Ok(u32::from_be_bytes(
        value
            .try_into()
            .map_err(|_| MetadataError::Malformed(error))?,
    ))
}

fn read_i32(bytes: &[u8], offset: usize, error: &'static str) -> Result<i32, MetadataError> {
    let end = offset
        .checked_add(4)
        .ok_or(MetadataError::Malformed(error))?;
    let value = bytes
        .get(offset..end)
        .ok_or(MetadataError::Malformed(error))?;
    Ok(i32::from_be_bytes(
        value
            .try_into()
            .map_err(|_| MetadataError::Malformed(error))?,
    ))
}

fn read_u64(bytes: &[u8], offset: usize, error: &'static str) -> Result<u64, MetadataError> {
    let end = offset
        .checked_add(8)
        .ok_or(MetadataError::Malformed(error))?;
    let value = bytes
        .get(offset..end)
        .ok_or(MetadataError::Malformed(error))?;
    Ok(u64::from_be_bytes(
        value
            .try_into()
            .map_err(|_| MetadataError::Malformed(error))?,
    ))
}

fn checked_end(
    start: u64,
    length: u64,
    file_length: u64,
    error: &'static str,
) -> Result<u64, MetadataError> {
    let end = start
        .checked_add(length)
        .ok_or(MetadataError::Malformed(error))?;
    if end > file_length {
        return Err(MetadataError::Malformed(error));
    }
    Ok(end)
}

fn read_exact_at(
    reader: &mut (impl Read + Seek),
    offset: u64,
    bytes: &mut [u8],
) -> Result<(), io::Error> {
    reader.seek(SeekFrom::Start(offset))?;
    reader.read_exact(bytes)
}

fn charge_metadata(total: &mut u64, length: u64) -> Result<(), MetadataError> {
    *total = total.checked_add(length).ok_or(MetadataError::Malformed(
        "parsed metadata byte count overflows",
    ))?;
    Ok(())
}

fn read_c_string(
    reader: &mut (impl Read + Seek),
    file_length: u64,
    cursor: &mut u64,
    metadata_bytes: &mut u64,
    error: &'static str,
) -> Result<Vec<u8>, MetadataReadError> {
    let mut value = Vec::new();
    loop {
        checked_end(*cursor, 1, file_length, error)?;
        charge_metadata(metadata_bytes, 1)?;
        let mut byte = [0_u8; 1];
        read_exact_at(reader, *cursor, &mut byte)?;
        *cursor += 1;
        if byte[0] == 0 {
            return Ok(value);
        }
        // OpenEXR's long-names flag permits at most 255 bytes, excluding NUL.
        // This is a native format constraint, not a converter resource quota.
        if value.len() == 255 {
            return Err(MetadataError::Malformed(error).into());
        }
        value.push(byte[0]);
    }
}

pub(super) fn open_exr_dimensions(
    reader: &mut (impl Read + Seek),
    file_length: u64,
) -> Result<[u16; 2], MetadataReadError> {
    const MAGIC: [u8; 4] = 20_000_630_u32.to_le_bytes();

    checked_end(0, 8, file_length, "OpenEXR container header is truncated")?;
    let mut metadata_bytes = 8;
    let mut header = [0_u8; 8];
    read_exact_at(reader, 0, &mut header)?;
    if header[..4] != MAGIC[..] {
        return Err(MetadataError::Malformed(
            "OpenEXR archive asset has malformed or missing dataWindow metadata",
        )
        .into());
    }
    let mut cursor = 8_u64;
    let mut dimensions = None;
    loop {
        let name = read_c_string(
            reader,
            file_length,
            &mut cursor,
            &mut metadata_bytes,
            "OpenEXR attribute name is malformed or unterminated",
        )?;
        if name.is_empty() {
            return dimensions.ok_or(MetadataReadError::Profile(MetadataError::Malformed(
                "OpenEXR archive asset has malformed or missing dataWindow metadata",
            )));
        }
        let value_type = read_c_string(
            reader,
            file_length,
            &mut cursor,
            &mut metadata_bytes,
            "OpenEXR attribute type is malformed or unterminated",
        )?;
        if value_type.is_empty() {
            return Err(MetadataError::Malformed("OpenEXR attribute type is empty").into());
        }
        let size_end = checked_end(
            cursor,
            4,
            file_length,
            "OpenEXR attribute size is truncated",
        )?;
        charge_metadata(&mut metadata_bytes, 4)?;
        let mut size_bytes = [0_u8; 4];
        read_exact_at(reader, cursor, &mut size_bytes)?;
        let size = u64::from(u32::from_le_bytes(size_bytes));
        cursor = size_end;
        let payload_end = checked_end(
            cursor,
            size,
            file_length,
            "OpenEXR attribute extends beyond the asset",
        )?;
        charge_metadata(&mut metadata_bytes, size)?;
        if name == b"dataWindow" {
            if value_type != b"box2i" || size != 16 || dimensions.is_some() {
                return Err(MetadataError::Malformed(
                    "OpenEXR dataWindow metadata is malformed or duplicated",
                )
                .into());
            }
            let mut payload = [0_u8; 16];
            read_exact_at(reader, cursor, &mut payload)?;
            let x_min = i32::from_le_bytes(payload[0..4].try_into().map_err(|_| {
                MetadataError::Malformed("OpenEXR dataWindow metadata is malformed")
            })?);
            let y_min = i32::from_le_bytes(payload[4..8].try_into().map_err(|_| {
                MetadataError::Malformed("OpenEXR dataWindow metadata is malformed")
            })?);
            let x_max = i32::from_le_bytes(payload[8..12].try_into().map_err(|_| {
                MetadataError::Malformed("OpenEXR dataWindow metadata is malformed")
            })?);
            let y_max = i32::from_le_bytes(payload[12..16].try_into().map_err(|_| {
                MetadataError::Malformed("OpenEXR dataWindow metadata is malformed")
            })?);
            let width = x_max
                .checked_sub(x_min)
                .and_then(|value| value.checked_add(1))
                .and_then(|value| u16::try_from(value).ok())
                .ok_or(MetadataError::Unsupported(
                    "OpenEXR dataWindow width exceeds the native source profile",
                ))?;
            let height = y_max
                .checked_sub(y_min)
                .and_then(|value| value.checked_add(1))
                .and_then(|value| u16::try_from(value).ok())
                .ok_or(MetadataError::Unsupported(
                    "OpenEXR dataWindow height exceeds the native source profile",
                ))?;
            dimensions = Some([width, height]);
        }
        cursor = payload_end;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct WaveMetadata {
    pub(super) sample_rate: u32,
    pub(super) duration_millis: u64,
    pub(super) sample_frames: u32,
    pub(super) file_length: u32,
}

pub(super) fn wave_from_reader(
    reader: &mut (impl Read + Seek),
    file_length: u64,
) -> Result<WaveMetadata, MetadataReadError> {
    checked_end(
        0,
        12,
        file_length,
        "RIFF/WAVE container header is truncated",
    )?;
    let mut metadata_bytes = 12_u64;
    let mut header = [0_u8; 12];
    read_exact_at(reader, 0, &mut header)?;
    if &header[0..4] == b"RF64" && &header[8..12] == b"WAVE" {
        return Err(MetadataError::Unsupported(
            "RF64/WAVE is outside the source-backed RIFF/WAVE profile",
        )
        .into());
    }
    if &header[0..4] != b"RIFF" || &header[8..12] != b"WAVE" {
        return Err(
            MetadataError::Malformed("RIFF/WAVE asset has a malformed container header").into(),
        );
    }
    let declared_size =
        u64::from(u32::from_le_bytes(header[4..8].try_into().map_err(
            |_| MetadataError::Malformed("RIFF size is malformed"),
        )?))
        .checked_add(8)
        .ok_or(MetadataError::Malformed("RIFF size overflows"))?;
    if declared_size != file_length {
        return Err(MetadataError::Malformed("RIFF size does not match the archive asset").into());
    }

    let mut cursor = 12_u64;
    let mut format = None;
    let mut data_length = None;
    while cursor < file_length {
        let header_end = checked_end(cursor, 8, file_length, "RIFF chunk header is truncated")?;
        charge_metadata(&mut metadata_bytes, 8)?;
        let mut chunk_header = [0_u8; 8];
        read_exact_at(reader, cursor, &mut chunk_header)?;
        let kind: [u8; 4] = chunk_header[0..4]
            .try_into()
            .map_err(|_| MetadataError::Malformed("RIFF chunk kind is malformed"))?;
        let length =
            u64::from(u32::from_le_bytes(chunk_header[4..8].try_into().map_err(
                |_| MetadataError::Malformed("RIFF chunk size is malformed"),
            )?));
        let payload_end = checked_end(
            header_end,
            length,
            file_length,
            "RIFF chunk extends beyond the asset",
        )?;
        let padded_end = checked_end(
            payload_end,
            length & 1,
            file_length,
            "RIFF odd-sized chunk is missing its padding byte",
        )?;
        if kind != *b"data" {
            charge_metadata(&mut metadata_bytes, length)?;
        }
        if kind == *b"fmt " {
            if format.is_some() || length < 16 {
                return Err(MetadataError::Malformed(
                    "RIFF/WAVE has malformed or duplicate format metadata",
                )
                .into());
            }
            let mut payload = [0_u8; 16];
            read_exact_at(reader, header_end, &mut payload)?;
            let encoding = u16::from_le_bytes(
                payload[0..2]
                    .try_into()
                    .map_err(|_| MetadataError::Malformed("WAVE encoding is malformed"))?,
            );
            if !matches!(encoding, 1 | 3) {
                return Err(MetadataError::Unsupported(
                    "WAVE encoding is outside the source-backed PCM/float profile",
                )
                .into());
            }
            let channels = u16::from_le_bytes(
                payload[2..4]
                    .try_into()
                    .map_err(|_| MetadataError::Malformed("WAVE channels are malformed"))?,
            );
            if !matches!(channels, 1 | 2) {
                return Err(MetadataError::Unsupported(
                    "WAVE audio must have an explicit mono or stereo layout",
                )
                .into());
            }
            let sample_rate = u32::from_le_bytes(
                payload[4..8]
                    .try_into()
                    .map_err(|_| MetadataError::Malformed("WAVE sample rate is malformed"))?,
            );
            let byte_rate = u32::from_le_bytes(
                payload[8..12]
                    .try_into()
                    .map_err(|_| MetadataError::Malformed("WAVE byte rate is malformed"))?,
            );
            let block_align = u16::from_le_bytes(
                payload[12..14]
                    .try_into()
                    .map_err(|_| MetadataError::Malformed("WAVE block alignment is malformed"))?,
            );
            let bits_per_sample = u16::from_le_bytes(
                payload[14..16]
                    .try_into()
                    .map_err(|_| MetadataError::Malformed("WAVE sample size is malformed"))?,
            );
            let supported_bits = if encoding == 1 {
                matches!(bits_per_sample, 8 | 16 | 24 | 32)
            } else {
                matches!(bits_per_sample, 32 | 64)
            };
            if !supported_bits {
                return Err(MetadataError::Unsupported(
                    "WAVE sample size is outside the source-backed PCM/float profile",
                )
                .into());
            }
            let bytes_per_sample = bits_per_sample / 8;
            let expected_align = channels
                .checked_mul(bytes_per_sample)
                .ok_or(MetadataError::Malformed("WAVE block alignment overflows"))?;
            let expected_rate = sample_rate
                .checked_mul(u32::from(expected_align))
                .ok_or(MetadataError::Malformed("WAVE byte rate overflows"))?;
            if sample_rate == 0
                || block_align == 0
                || block_align != expected_align
                || byte_rate != expected_rate
            {
                return Err(MetadataError::Malformed(
                    "WAVE rate and block metadata are inconsistent",
                )
                .into());
            }
            format = Some((sample_rate, block_align));
        } else if kind == *b"data" && data_length.replace(length).is_some() {
            return Err(MetadataError::Malformed("RIFF/WAVE has duplicate data chunks").into());
        }
        cursor = padded_end;
    }
    let (sample_rate, block_align) =
        format.ok_or(MetadataError::Malformed("RIFF/WAVE has no format metadata"))?;
    let data_length = data_length.ok_or(MetadataError::Malformed(
        "RIFF/WAVE has no audio data chunk",
    ))?;
    if data_length == 0 || data_length % u64::from(block_align) != 0 {
        return Err(
            MetadataError::Malformed("WAVE audio data is empty or not sample-aligned").into(),
        );
    }
    let sample_frames = data_length / u64::from(block_align);
    let duration_millis = sample_frames
        .checked_mul(1_000)
        .ok_or(MetadataError::Malformed("WAVE duration overflows"))?
        .div_ceil(u64::from(sample_rate));
    let sample_frames = u32::try_from(sample_frames).map_err(|_| {
        MetadataError::Unsupported("WAVE sample count exceeds the native 32-bit field")
    })?;
    let file_length = u32::try_from(file_length).map_err(|_| {
        MetadataError::Unsupported("WAVE file length exceeds the native 32-bit field")
    })?;
    Ok(WaveMetadata {
        sample_rate,
        duration_millis,
        sample_frames,
        file_length,
    })
}

#[cfg(test)]
mod tests {
    mod pcm_timecode;
    mod presentation;

    use std::io::{Cursor, Write};

    use super::*;

    struct ReadLimit<R> {
        inner: R,
        bytes_read: usize,
        limit: usize,
    }

    impl<R> ReadLimit<R> {
        fn new(inner: R, limit: usize) -> Self {
            Self {
                inner,
                bytes_read: 0,
                limit,
            }
        }
    }

    impl<R: Read> Read for ReadLimit<R> {
        fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
            if bytes.len() > self.limit.saturating_sub(self.bytes_read) {
                return Err(io::Error::other("metadata parser exceeded test read limit"));
            }
            let read = self.inner.read(bytes)?;
            self.bytes_read += read;
            Ok(read)
        }
    }

    impl<R: Seek> Seek for ReadLimit<R> {
        fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
            self.inner.seek(position)
        }
    }

    #[test]
    fn native_ntsc_movie_clock_is_admitted() {
        let movie = include_bytes!("../../../../tests/fixtures/ntsc_media_clock/movie.mov");
        let metadata = quicktime(movie).expect("native-supported NTSC MOV must be admitted");
        assert_eq!(metadata.dimensions, [160, 90]);
        assert_eq!(metadata.duration_millis, 1_001);
        assert_eq!(metadata.duration_millis_floor, 1_001);
        assert_eq!(
            metadata.frame_rate,
            NativeFrameRate {
                integer: 29,
                fractional: 63570
            }
        );
        assert_eq!(
            metadata.native_duration,
            Some(MediaDuration {
                numerator: 3000,
                denominator: 2997
            })
        );
    }

    #[test]
    fn ntsc_source_clock_accepts_only_exact_equivalent_ratios() {
        for (timescale, delta) in [(30_000, 1001), (60_000, 2002), (90_000, 3003)] {
            let metadata = quicktime(&movie_with_video_timing(1001, timescale, 30, delta)).unwrap();
            assert_eq!(
                metadata.native_duration,
                Some(MediaDuration {
                    numerator: 3000,
                    denominator: 2997
                })
            );
            assert_eq!(
                metadata.frame_rate,
                NativeFrameRate {
                    integer: 29,
                    fractional: 63570
                }
            );
        }
        for (timescale, delta) in [
            (29_999, 1001),
            (30_001, 1001),
            (30_000, 1000 + 2),
            (24_000, 1001),
            (60_000, 1001),
        ] {
            assert!(matches!(
                native_frame_rate(timescale, delta),
                Err(MetadataError::Unsupported(_))
            ));
        }
    }

    #[test]
    fn ntsc_native_duration_uses_sample_count_not_rounded_millis() {
        let metadata = quicktime(&movie_with_video_timing(34, 30_000, 1, 1001)).unwrap();
        assert_eq!(metadata.duration_millis, 34);
        assert_eq!(
            metadata.native_duration,
            Some(MediaDuration {
                numerator: 100,
                denominator: 2997
            })
        );
    }

    #[test]
    fn reads_constant_rate_quicktime_metadata() {
        let movie = valid_movie();
        assert_eq!(
            quicktime(&movie),
            Ok(QuickTimeMetadata {
                dimensions: [1920, 1080],
                duration_millis: 1_000,
                duration_millis_floor: 1_000,
                duration_native_ticks: Some(24_576),
                frame_rate: NativeFrameRate::integer(24),
                native_duration: None,
                video_codec: *b"avc1",
                audio_sample_rate: 0.0,
            })
        );
    }

    #[test]
    fn quicktime_sample_entry_codec_is_preserved_for_native_import_options() {
        let mut descriptions = video_descriptions();
        descriptions[12..16].copy_from_slice(b"ap4h");
        assert_eq!(
            parse_video_description(&descriptions, &mut AtomBudget { remaining: 8 }),
            Ok(([1920, 1080], *b"ap4h"))
        );
    }

    #[test]
    fn identity_quicktime_edit_list_keeps_the_full_track_without_retiming() {
        let movie = movie_with_video_edit(1_000, 0, 0x0001_0000);
        assert_eq!(quicktime(&movie), quicktime(&valid_movie()));
    }

    #[test]
    fn shifted_or_shortened_quicktime_edit_list_remains_unsupported() {
        for movie in [
            movie_with_video_edit(900, 0, 0x0001_0000),
            movie_with_video_edit(1_000, 1, 0x0001_0000),
            movie_with_video_edit(1_000, -1, 0x0001_0000),
            movie_with_video_edit(1_000, 0, 0),
        ] {
            assert!(matches!(
                quicktime(&movie),
                Err(MetadataError::Unsupported(_))
            ));
        }
    }

    #[test]
    fn quicktime_keeps_exact_submillisecond_source_duration() {
        use super::super::{InterpretRequest, MediaRequest, interpret};
        use crate::writer::footage::{FootageKind, NativeSourceFormat};

        for (codec, format) in [
            (*b"avc1", NativeSourceFormat::QuickTime),
            (*b"ap4h", NativeSourceFormat::QuickTimeProRes4444),
        ] {
            let mut movie = movie_with_video_timing(241, 12_288, 241, 512);
            let position = movie
                .windows(4)
                .position(|window| window == b"mvhd")
                .unwrap();
            movie[position + 16..position + 20].copy_from_slice(&24_u32.to_be_bytes());
            let position = movie
                .windows(4)
                .position(|window| window == b"avc1")
                .unwrap();
            movie[position..position + 4].copy_from_slice(&codec);
            let parsed = quicktime(&movie).unwrap();
            assert_eq!(parsed.duration_millis, 10_042);
            assert_eq!(parsed.duration_native_ticks, Some(246_784));
            assert_eq!(parsed.duration_millis_floor, 10_041);
            assert_eq!(parsed.video_codec, codec);

            let mut file = tempfile::NamedTempFile::new().unwrap();
            file.write_all(&movie).unwrap();
            let request = MediaRequest {
                layer_id: fx_schema::LayerId::new(1),
                asset_id: fx_schema::AssetId::new("movie").unwrap(),
                kind: FootageKind::Video,
                preferred_name: "movie".into(),
            };
            let source = match interpret(InterpretRequest {
                request: &request,
                requested_kinds: super::super::VIDEO_KIND,
                asset_kind: tesseract_file::AssetKind::Video,
                archive_path: "movie.mov",
                content_type: "video/quicktime",
                materialized_path: file.path(),
                byte_length: movie.len() as u64,
                ordinal: 0,
            }) {
                Ok(source) => source,
                Err(_) => panic!("bounded movie metadata must pass ordinary interpretation"),
            };
            assert_eq!(source.format, format);
            assert_eq!(source.duration_native_ticks, Some(246_784));
        }
    }

    #[test]
    fn quicktime_fractional_native_source_ticks_keep_existing_millisecond_policy() {
        let mut movie = movie_with_video_timing(1, 30, 1, 1);
        let position = movie
            .windows(4)
            .position(|window| window == b"mvhd")
            .unwrap();
        movie[position + 16..position + 20].copy_from_slice(&30_u32.to_be_bytes());
        let parsed = quicktime(&movie).unwrap();
        assert_eq!(parsed.duration_millis, 34);
        assert_eq!(parsed.duration_native_ticks, None);
    }

    #[test]
    fn reads_exactly_representable_fractional_frame_rate() {
        let movie = movie_with_video_timing(2_000, 3, 3, 2);
        assert_eq!(
            quicktime(&movie),
            Ok(QuickTimeMetadata {
                dimensions: [1920, 1080],
                duration_millis: 2_000,
                duration_millis_floor: 2_000,
                duration_native_ticks: Some(49_152),
                frame_rate: NativeFrameRate {
                    integer: 1,
                    fractional: 32_768,
                },
                native_duration: None,
                video_codec: *b"avc1",
                audio_sample_rate: 0.0,
            })
        );
    }

    #[test]
    fn rejects_unrepresentable_fractional_frame_rate() {
        let movie = movie_with_video_timing(1_001, 24_000, 24, 1_001);
        assert_eq!(
            quicktime(&movie),
            Err(MetadataError::Unsupported(
                "QuickTime video frame rate cannot be represented exactly"
            ))
        );
    }

    #[test]
    fn rejects_non_quicktime_brand_as_unsupported() {
        let mut movie = valid_movie();
        movie[8..12].copy_from_slice(b"isom");
        assert!(matches!(
            quicktime(&movie),
            Err(MetadataError::Unsupported(_))
        ));
    }

    #[test]
    fn rejects_truncated_atom_as_malformed() {
        let mut movie = valid_movie();
        movie.truncate(movie.len() - 1);
        assert!(matches!(
            quicktime(&movie),
            Err(MetadataError::Malformed(_))
        ));
    }

    #[test]
    fn rejects_variable_rate_timing_as_unsupported() {
        let payload = [
            [0_u8; 4],
            2_u32.to_be_bytes(),
            12_u32.to_be_bytes(),
            1_000_u32.to_be_bytes(),
            12_u32.to_be_bytes(),
            1_001_u32.to_be_bytes(),
        ]
        .concat();
        assert!(matches!(
            parse_timing(&payload),
            Err(MetadataError::Unsupported(_))
        ));
    }

    #[test]
    fn duplicate_quicktime_file_types_remain_malformed_through_streaming() {
        let movie = valid_movie();
        let file_type_length =
            usize::try_from(u32::from_be_bytes(movie[0..4].try_into().unwrap())).unwrap();
        let mut duplicate = movie[..file_type_length].to_vec();
        duplicate.extend_from_slice(&movie);
        assert!(matches!(
            quicktime_from_reader(
                &mut Cursor::new(&duplicate),
                u64::try_from(duplicate.len()).unwrap()
            ),
            Err(MetadataReadError::Profile(MetadataError::Malformed(
                "QuickTime file has duplicate ftyp atoms"
            )))
        ));
    }

    // These sparse-reader regressions target the Read + Seek boundary introduced
    // by this fix. There was no callable streaming helper to execute before it.
    #[test]
    fn sparse_quicktime_media_payload_is_sought_over_under_a_tiny_read_limit() {
        let movie = valid_movie();
        let file_type_length =
            usize::try_from(u32::from_be_bytes(movie[0..4].try_into().unwrap())).unwrap();
        let (file_type, movie_atom) = movie.split_at(file_type_length);
        let media_atom_length = 16 * 1024 * 1024_u32;
        let movie_offset = u64::try_from(file_type.len()).unwrap() + u64::from(media_atom_length);
        let file_length = movie_offset + u64::try_from(movie_atom.len()).unwrap();
        let mut file = tempfile::tempfile().unwrap();
        file.write_all(file_type).unwrap();
        file.write_all(&media_atom_length.to_be_bytes()).unwrap();
        file.write_all(b"mdat").unwrap();
        file.seek(SeekFrom::Start(movie_offset)).unwrap();
        file.write_all(movie_atom).unwrap();
        file.set_len(file_length).unwrap();
        let expected_read = file_type.len() + 8 + movie_atom.len();
        let mut file = ReadLimit::new(file, expected_read);
        let parsed = quicktime_from_reader(&mut file, file_length).unwrap();
        assert_eq!(parsed.dimensions, [1920, 1080]);
        assert_eq!(file.bytes_read, expected_read);
    }

    #[test]
    fn malformed_large_quicktime_atom_still_rejects() {
        let file_type = atom(*b"ftyp", &[b"qt  ".as_slice(), &[0, 0, 0, 0]].concat());
        let movie_atom_length = 8 * 1024 * 1024_u64 + 8;
        let file_length = u64::try_from(file_type.len()).unwrap() + movie_atom_length;
        let mut file = tempfile::tempfile().unwrap();
        file.write_all(&file_type).unwrap();
        file.write_all(&u32::try_from(movie_atom_length).unwrap().to_be_bytes())
            .unwrap();
        file.write_all(b"moov").unwrap();
        file.set_len(file_length).unwrap();
        let mut file = ReadLimit::new(file, usize::try_from(file_length).unwrap());
        assert!(matches!(
            quicktime_from_reader(&mut file, file_length),
            Err(MetadataReadError::Profile(MetadataError::Malformed(_)))
        ));
    }

    #[test]
    fn quicktime_skips_more_than_former_atom_quota_without_losing_movie() {
        let mut movie = valid_movie();
        for _ in 0..513 {
            movie.extend_from_slice(&atom(*b"free", &[]));
        }
        let parsed = quicktime_from_reader(
            &mut std::io::Cursor::new(&movie),
            u64::try_from(movie.len()).unwrap(),
        )
        .unwrap();
        assert_eq!(parsed.dimensions, [1920, 1080]);
    }

    #[test]
    fn open_exr_preserves_native_name_bounds_without_an_attribute_count_quota() {
        let mut long_field = open_exr_header();
        append_exr_attribute(&mut long_field, &vec![b'x'; 256], b"string", &[]);
        append_data_window(&mut long_field, 640, 480);
        long_field.push(0);
        let length = u64::try_from(long_field.len()).unwrap();
        assert!(matches!(
            open_exr_dimensions(&mut Cursor::new(long_field), length),
            Err(MetadataReadError::Profile(MetadataError::Malformed(_)))
        ));

        let mut many_attributes = open_exr_header();
        for index in 0_i32..1_025 {
            append_exr_attribute(
                &mut many_attributes,
                format!("x{index}").as_bytes(),
                b"int",
                &index.to_le_bytes(),
            );
        }
        append_data_window(&mut many_attributes, 320, 240);
        many_attributes.push(0);
        let length = u64::try_from(many_attributes.len()).unwrap();
        assert_eq!(
            open_exr_dimensions(&mut Cursor::new(many_attributes), length).unwrap(),
            [320, 240]
        );
    }

    #[test]
    fn open_exr_still_rejects_unterminated_fields() {
        let mut bytes = open_exr_header();
        bytes.extend_from_slice(b"unterminated");
        let length = u64::try_from(bytes.len()).unwrap();
        assert!(matches!(
            open_exr_dimensions(&mut Cursor::new(bytes), length),
            Err(MetadataReadError::Profile(MetadataError::Malformed(_)))
        ));
    }

    #[test]
    fn wave_accepts_more_than_former_chunk_quota() {
        let mut wave = wave_header();
        append_wave_chunk(
            &mut wave,
            *b"fmt ",
            &[1, 0, 1, 0, 0x40, 0x1f, 0, 0, 0x40, 0x1f, 0, 0, 1, 0, 8, 0],
        );
        for _ in 0..1_025 {
            append_wave_chunk(&mut wave, *b"JUNK", &[]);
        }
        append_wave_chunk(&mut wave, *b"data", &[0]);
        finish_wave_header(&mut wave);
        let length = u64::try_from(wave.len()).unwrap();
        let parsed = wave_from_reader(&mut Cursor::new(wave), length).unwrap();
        assert_eq!(parsed.sample_rate, 8_000);
        assert_eq!(parsed.sample_frames, 1);
    }

    #[test]
    fn wave_still_rejects_a_chunk_extending_beyond_the_file() {
        let mut wave = wave_header();
        wave.extend_from_slice(b"JUNK");
        wave.extend_from_slice(&4_u32.to_le_bytes());
        finish_wave_header(&mut wave);
        let length = u64::try_from(wave.len()).unwrap();
        assert!(matches!(
            wave_from_reader(&mut Cursor::new(wave), length),
            Err(MetadataReadError::Profile(MetadataError::Malformed(_)))
        ));
    }

    fn open_exr_header() -> Vec<u8> {
        [20_000_630_u32.to_le_bytes(), 2_u32.to_le_bytes()].concat()
    }

    fn append_exr_attribute(bytes: &mut Vec<u8>, name: &[u8], value_type: &[u8], value: &[u8]) {
        bytes.extend_from_slice(name);
        bytes.push(0);
        bytes.extend_from_slice(value_type);
        bytes.push(0);
        bytes.extend_from_slice(&u32::try_from(value.len()).unwrap().to_le_bytes());
        bytes.extend_from_slice(value);
    }

    fn append_data_window(bytes: &mut Vec<u8>, width: i32, height: i32) {
        let payload = [
            0_i32.to_le_bytes(),
            0_i32.to_le_bytes(),
            (width - 1).to_le_bytes(),
            (height - 1).to_le_bytes(),
        ]
        .concat();
        append_exr_attribute(bytes, b"dataWindow", b"box2i", &payload);
    }

    fn wave_header() -> Vec<u8> {
        [b"RIFF".as_slice(), &[0; 4], b"WAVE"].concat()
    }

    fn append_wave_chunk(bytes: &mut Vec<u8>, kind: [u8; 4], payload: &[u8]) {
        bytes.extend_from_slice(&kind);
        bytes.extend_from_slice(&u32::try_from(payload.len()).unwrap().to_le_bytes());
        bytes.extend_from_slice(payload);
        if !payload.len().is_multiple_of(2) {
            bytes.push(0);
        }
    }

    fn finish_wave_header(bytes: &mut [u8]) {
        let size = u32::try_from(bytes.len() - 8).unwrap();
        bytes[4..8].copy_from_slice(&size.to_le_bytes());
    }

    fn valid_movie() -> Vec<u8> {
        movie_with_video_timing(1_000, 24_000, 24, 1_000)
    }

    fn movie_with_video_timing(
        movie_duration: u32,
        media_timescale: u32,
        sample_count: u32,
        sample_delta: u32,
    ) -> Vec<u8> {
        let media_duration = sample_count
            .checked_mul(sample_delta)
            .expect("test media duration fits in 32 bits");
        let ftyp = atom(*b"ftyp", &[b"qt  ".as_slice(), &[0, 0, 0, 0]].concat());
        let mvhd = atom(*b"mvhd", &movie_header(movie_duration));
        let tkhd = atom(*b"tkhd", &track_header(movie_duration));
        let mdhd = atom(*b"mdhd", &media_header(media_timescale, media_duration));
        let hdlr = atom(*b"hdlr", &handler(*b"vide"));
        let stsd = atom(*b"stsd", &video_descriptions());
        let stts = atom(*b"stts", &timing(sample_count, sample_delta));
        let stbl = atom(*b"stbl", &[stsd, stts].concat());
        let minf = atom(*b"minf", &stbl);
        let mdia = atom(*b"mdia", &[mdhd, hdlr, minf].concat());
        let trak = atom(*b"trak", &[tkhd, mdia].concat());
        let moov = atom(*b"moov", &[mvhd, trak].concat());
        [ftyp, moov].concat()
    }

    fn movie_with_video_edit(duration: u32, media_time: i32, rate: u32) -> Vec<u8> {
        let movie = valid_movie();
        let ftyp_size = u32::from_be_bytes(movie[0..4].try_into().unwrap()) as usize;
        let moov = &movie[ftyp_size..];
        let mvhd_size = u32::from_be_bytes(moov[8..12].try_into().unwrap()) as usize;
        let trak = &moov[8 + mvhd_size..];
        let tkhd_size = u32::from_be_bytes(trak[8..12].try_into().unwrap()) as usize;
        let mut payload = vec![0_u8; 8];
        payload[4..8].copy_from_slice(&1_u32.to_be_bytes());
        payload.extend_from_slice(&duration.to_be_bytes());
        payload.extend_from_slice(&media_time.to_be_bytes());
        payload.extend_from_slice(&rate.to_be_bytes());
        let edts = atom(*b"edts", &atom(*b"elst", &payload));
        let assembled = atom(
            *b"trak",
            &[&trak[8..8 + tkhd_size], &edts, &trak[8 + tkhd_size..]].concat(),
        );
        let moov = atom(*b"moov", &[&moov[8..8 + mvhd_size], &assembled].concat());
        [&movie[..ftyp_size], &moov].concat()
    }

    fn atom(kind: [u8; 4], payload: &[u8]) -> Vec<u8> {
        let size =
            u32::try_from(payload.len() + 8).expect("test atom fits in its 32-bit size field");
        [size.to_be_bytes().as_slice(), &kind, payload].concat()
    }

    fn movie_header(duration: u32) -> Vec<u8> {
        let mut payload = vec![0_u8; 20];
        payload[12..16].copy_from_slice(&1_000_u32.to_be_bytes());
        payload[16..20].copy_from_slice(&duration.to_be_bytes());
        payload
    }

    fn track_header(duration: u32) -> Vec<u8> {
        let mut payload = vec![0_u8; 84];
        payload[3] = 3;
        payload[20..24].copy_from_slice(&duration.to_be_bytes());
        for (index, value) in [0x0001_0000_u32, 0, 0, 0, 0x0001_0000, 0, 0, 0, 0x4000_0000]
            .into_iter()
            .enumerate()
        {
            let offset = 40 + index * 4;
            payload[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
        }
        payload[76..80].copy_from_slice(&(1920_u32 << 16).to_be_bytes());
        payload[80..84].copy_from_slice(&(1080_u32 << 16).to_be_bytes());
        payload
    }

    fn media_header(timescale: u32, duration: u32) -> Vec<u8> {
        let mut payload = vec![0_u8; 20];
        payload[12..16].copy_from_slice(&timescale.to_be_bytes());
        payload[16..20].copy_from_slice(&duration.to_be_bytes());
        payload
    }

    fn handler(kind: [u8; 4]) -> Vec<u8> {
        let mut payload = vec![0_u8; 12];
        payload[8..12].copy_from_slice(&kind);
        payload
    }

    fn video_descriptions() -> Vec<u8> {
        let mut entry = vec![0_u8; 78];
        entry[24..26].copy_from_slice(&1920_u16.to_be_bytes());
        entry[26..28].copy_from_slice(&1080_u16.to_be_bytes());
        let description = atom(*b"avc1", &entry);
        [&[0_u8; 4], &1_u32.to_be_bytes(), description.as_slice()].concat()
    }

    fn timing(sample_count: u32, sample_delta: u32) -> Vec<u8> {
        [
            [0_u8; 4],
            1_u32.to_be_bytes(),
            sample_count.to_be_bytes(),
            sample_delta.to_be_bytes(),
        ]
        .concat()
    }
}
