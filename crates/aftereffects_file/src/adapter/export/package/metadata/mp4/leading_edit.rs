//! Original-container admission for a leading empty edit and one full picture run.

use super::super::{AtomBudget, MetadataError, atoms, exactly_one, optional_one};

#[derive(Clone, Copy)]
pub(super) struct LeadingEdit {
    timescale: u32,
    delay: u64,
    content_duration: u64,
    media_timescale: u32,
    timing: super::super::Timing,
}

pub(super) fn from_original(bytes: &[u8]) -> Result<Option<LeadingEdit>, MetadataError> {
    let mut budget = AtomBudget {
        remaining: usize::MAX,
    };
    let roots = atoms(bytes, &mut budget)?;
    let movie = exactly_one(&roots, *b"moov", "MP4 requires one movie")?;
    let children = atoms(movie.payload, &mut budget)?;
    let header = super::super::parse_movie_header(
        exactly_one(&children, *b"mvhd", "MP4 requires movie header")?.payload,
    )?;
    let mut result = None;
    for track in children.iter().filter(|atom| atom.kind == *b"trak") {
        let track = atoms(track.payload, &mut budget)?;
        let media = exactly_one(&track, *b"mdia", "MP4 requires media")?;
        let media = atoms(media.payload, &mut budget)?;
        if super::super::parse_handler(
            exactly_one(&media, *b"hdlr", "MP4 requires handler")?.payload,
        )? != *b"vide"
        {
            continue;
        }
        let Some(edits) = optional_one(&track, *b"edts", "MP4 duplicate edits")? else {
            return Ok(None);
        };
        let edits = atoms(edits.payload, &mut budget)?;
        let list = exactly_one(&edits, *b"elst", "MP4 requires edit list")?.payload;
        if list.len() != 32 || list[..8] != [0, 0, 0, 0, 0, 0, 0, 2] {
            return Ok(None);
        }
        let read = |offset| super::super::read_u32(list, offset, "MP4 delayed edit is truncated");
        let delay = u64::from(read(8)?);
        let content_duration = u64::from(read(20)?);
        let track_header = super::super::parse_track_header(
            exactly_one(&track, *b"tkhd", "MP4 requires track header")?.payload,
        )?;
        if delay == 0
            || content_duration == 0
            || read(12)? != u32::MAX
            || read(16)? != 0x0001_0000
            || read(24)? != 0
            || read(28)? != 0x0001_0000
            || delay + content_duration != header.duration
            || track_header.duration != header.duration
            || result.is_some()
        {
            return Ok(None);
        }
        let media_header = super::super::parse_media_header(
            exactly_one(&media, *b"mdhd", "MP4 requires media header")?.payload,
        )?;
        let info = exactly_one(&media, *b"minf", "MP4 requires media information")?;
        let info = atoms(info.payload, &mut budget)?;
        let table = exactly_one(&info, *b"stbl", "MP4 requires sample table")?;
        let table = atoms(table.payload, &mut budget)?;
        let timing = super::super::parse_timing(
            exactly_one(&table, *b"stts", "MP4 requires sample timing")?.payload,
        )?;
        if timing.duration != media_header.duration {
            return Ok(None);
        }
        result = Some(LeadingEdit {
            timescale: header.timescale,
            delay,
            content_duration,
            media_timescale: media_header.timescale,
            timing,
        });
    }
    Ok(result)
}

impl LeadingEdit {
    pub(super) fn matches(
        self,
        origin: i64,
        duration: i64,
        delta: i64,
        count: usize,
        num: i32,
        den: i32,
    ) -> bool {
        if origin <= 0 || duration <= 0 || num <= 0 || den <= 0 || self.timescale == 0 {
            return false;
        }
        let scale = u128::from(self.timescale) * num as u128;
        let denominator = den as u128;
        origin as u128 * scale == u128::from(self.delay) * denominator
            && (duration as u128 * scale).div_ceil(denominator) == u128::from(self.content_duration)
            && usize::try_from(self.timing.sample_count).ok() == Some(count)
            && delta as u128 * num as u128 * u128::from(self.media_timescale)
                == u128::from(self.timing.sample_delta) * denominator
            && duration as u128 * num as u128 * u128::from(self.media_timescale)
                == u128::from(self.timing.duration) * denominator
    }
}
