use super::{Atom, MetadataError, Timing, optional_one, read_u32};

// Validate complete constant-rate presentation, not merely decode duration.
// Each unsigned ctts run is a contiguous presentation interval because stts has
// already established a single sample delta. Sorting runs proves the whole
// sample grid without allocating one entry for every declared media frame.
pub(super) fn origin(sample_atoms: &[Atom<'_>], timing: Timing) -> Result<u64, MetadataError> {
    let Some(composition) = optional_one(
        sample_atoms,
        *b"ctts",
        "QuickTime video has duplicate ctts atoms",
    )?
    else {
        return Ok(0);
    };
    let payload = composition.payload;
    if payload.get(..4) != Some(&[0, 0, 0, 0]) {
        return Err(MetadataError::Unsupported(
            "QuickTime signed composition offsets or ctts flags are unsupported",
        ));
    }
    let count = read_u32(payload, 4, "QuickTime ctts header is truncated")?;
    let entries_len = usize::try_from(count)
        .ok()
        .and_then(|count| count.checked_mul(8))
        .and_then(|bytes| bytes.checked_add(8))
        .ok_or(MetadataError::Malformed("QuickTime ctts length overflows"))?;
    if count == 0 || payload.len() != entries_len {
        return Err(MetadataError::Malformed(
            "QuickTime ctts entry count does not match its payload",
        ));
    }

    let mut intervals = Vec::with_capacity((payload.len() - 8) / 8);
    let mut sample_count = 0_u64;
    let mut decode_time = 0_u64;
    for entry in payload[8..].chunks_exact(8) {
        let samples = read_u32(entry, 0, "QuickTime ctts sample count is truncated")?;
        let offset = read_u32(entry, 4, "QuickTime ctts offset is truncated")?;
        if samples == 0 {
            return Err(MetadataError::Malformed("QuickTime ctts run is empty"));
        }
        sample_count =
            sample_count
                .checked_add(u64::from(samples))
                .ok_or(MetadataError::Malformed(
                    "QuickTime ctts sample count overflows",
                ))?;
        if sample_count > u64::from(timing.sample_count) {
            return Err(MetadataError::Malformed(
                "QuickTime ctts and stts sample counts disagree",
            ));
        }
        let duration = u64::from(samples) * u64::from(timing.sample_delta);
        let start = decode_time
            .checked_add(u64::from(offset))
            .ok_or(MetadataError::Malformed(
                "QuickTime presentation time overflows",
            ))?;
        let end = start.checked_add(duration).ok_or(MetadataError::Malformed(
            "QuickTime presentation duration overflows",
        ))?;
        intervals.push((start, end));
        decode_time = decode_time
            .checked_add(duration)
            .ok_or(MetadataError::Malformed(
                "QuickTime decode duration overflows",
            ))?;
    }
    if sample_count != u64::from(timing.sample_count) {
        return Err(MetadataError::Malformed(
            "QuickTime ctts and stts sample counts disagree",
        ));
    }

    intervals.sort_unstable();
    let origin = intervals[0].0;
    let mut previous_end = origin;
    for (start, end) in intervals {
        if start != previous_end {
            return Err(MetadataError::Unsupported(
                "QuickTime composition offsets do not form a complete constant-rate presentation grid",
            ));
        }
        previous_end = end;
    }
    if previous_end.checked_sub(origin) != Some(timing.duration) {
        return Err(MetadataError::Malformed(
            "QuickTime presentation and decode durations disagree",
        ));
    }
    Ok(origin)
}
