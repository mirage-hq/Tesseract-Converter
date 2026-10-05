//! Native AVC interpretation of one zero-duration terminal sample.

use super::super::{MetadataError, atoms, exactly_one, optional_one, read_u32};

#[derive(Clone, Copy)]
pub(super) struct TerminalSample {
    count: u32,
    delta: u32,
    timescale: u32,
    duration: u64,
}

impl TerminalSample {
    pub(super) fn matches(
        self,
        count: usize,
        delta: i64,
        num: i32,
        den: i32,
        duration: i64,
    ) -> bool {
        u64::try_from(count).ok() == Some(u64::from(self.count) + 1)
            && num == 1
            && i64::from(den) == i64::from(self.timescale)
            && delta == i64::from(self.delta)
            && u64::try_from(duration).ok() == Some(self.duration)
    }
}

pub(super) fn from_original(bytes: &[u8]) -> Result<Option<TerminalSample>, MetadataError> {
    let mut budget = super::super::AtomBudget {
        remaining: usize::MAX,
    };
    let roots = atoms(bytes, &mut budget)?;
    let movie = exactly_one(&roots, *b"moov", "MP4 requires movie")?;
    let children = atoms(movie.payload, &mut budget)?;
    let header = super::super::parse_movie_header(
        exactly_one(&children, *b"mvhd", "MP4 requires movie header")?.payload,
    )?;
    if header.timescale != 1000 {
        return Ok(None);
    }
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
        if result.is_some() {
            return Ok(None);
        }
        let media_header = super::super::parse_media_header(
            exactly_one(&media, *b"mdhd", "MP4 requires media header")?.payload,
        )?;
        let info = exactly_one(&media, *b"minf", "MP4 requires media information")?;
        let info = atoms(info.payload, &mut budget)?;
        let table = exactly_one(&info, *b"stbl", "MP4 requires sample table")?;
        let table = atoms(table.payload, &mut budget)?;
        let description = exactly_one(&table, *b"stsd", "MP4 requires sample description")?.payload;
        if description.len() < 8 || description[..8] != [0, 0, 0, 0, 0, 0, 0, 1] {
            return Ok(None);
        }
        let entries = atoms(&description[8..], &mut budget)?;
        // Only avc1 was independently exercised. Other AVC variants and codecs
        // must not inherit its terminal-frame duration interpretation.
        if entries.len() != 1 || entries[0].kind != *b"avc1" {
            return Ok(None);
        }
        if optional_one(&table, *b"ctts", "MP4 duplicate composition offsets")?.is_some() {
            return Ok(None);
        }
        let timing = exactly_one(&table, *b"stts", "MP4 requires sample timing")?.payload;
        if timing.len() != 24
            || timing[..8] != [0, 0, 0, 0, 0, 0, 0, 2]
            || timing[16..24] != [0, 0, 0, 1, 0, 0, 0, 0]
        {
            return Ok(None);
        }
        let count = read_u32(timing, 8, "MP4 sample count")?;
        let delta = read_u32(timing, 12, "MP4 sample delta")?;
        let duration = u64::from(count) * u64::from(delta);
        if count == 0
            || delta == 0
            || media_header.timescale == 0
            || media_header.duration != duration
        {
            return Ok(None);
        }
        let sizes = exactly_one(&table, *b"stsz", "MP4 requires sample sizes")?.payload;
        if sizes.len() < 12
            || sizes[..4] != [0; 4]
            || u64::from(read_u32(sizes, 8, "MP4 size count")?) != u64::from(count) + 1
        {
            return Ok(None);
        }
        let Some(edits) = optional_one(&track, *b"edts", "MP4 duplicate edits")? else {
            return Ok(None);
        };
        let edits = atoms(edits.payload, &mut budget)?;
        let edit = exactly_one(&edits, *b"elst", "MP4 requires edit list")?.payload;
        if edit.len() != 20
            || edit[..8] != [0, 0, 0, 0, 0, 0, 0, 1]
            || edit[12..20] != [0, 0, 0, 0, 0, 1, 0, 0]
        {
            return Ok(None);
        }
        let rounded_duration =
            (u128::from(duration) * 1000).div_ceil(u128::from(media_header.timescale));
        let track_header = super::super::parse_track_header(
            exactly_one(&track, *b"tkhd", "MP4 requires track header")?.payload,
        )?;
        if u128::from(read_u32(edit, 8, "MP4 edit duration")?) != rounded_duration
            || u128::from(header.duration) != rounded_duration
            || track_header.duration != header.duration
        {
            return Ok(None);
        }
        result = Some(TerminalSample {
            count,
            delta,
            timescale: media_header.timescale,
            duration,
        });
    }
    Ok(result)
}
