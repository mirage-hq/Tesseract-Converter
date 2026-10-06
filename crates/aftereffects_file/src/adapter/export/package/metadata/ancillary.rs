//! Admission of preserved PCM and ancillary timecode; no decoding or rewriting.

use super::*;

pub(super) fn require_pcm_description(
    payload: &[u8],
    version: u16,
    channels: u16,
    budget: &mut AtomBudget,
) -> Result<(), MetadataError> {
    if read_u16(payload, 6, "QuickTime PCM data reference is truncated")? != 1
        || read_u16(payload, 18, "QuickTime PCM precision is truncated")? != 16
        || !matches!(
            read_u16(payload, 20, "QuickTime PCM compression is truncated")?,
            0 | 0xffff
        )
        || read_u16(payload, 22, "QuickTime PCM packet size is truncated")? != 0
    {
        return Err(MetadataError::Unsupported(
            "QuickTime PCM precision or storage profile is unsupported",
        ));
    }
    let header_size = if version == 1 {
        for (offset, expected) in [(28, 1), (32, 2), (36, u32::from(channels) * 2), (40, 2)] {
            if read_u32(
                payload,
                offset,
                "QuickTime PCM packet extension is truncated",
            )? != expected
            {
                return Err(MetadataError::Malformed(
                    "QuickTime PCM packet layout is inconsistent",
                ));
            }
        }
        44
    } else {
        28
    };
    let children = atoms(&payload[header_size..], budget)?;
    if children.is_empty() {
        return Ok(());
    }
    // Only the explicit standard mono/stereo tag is known. Do not infer an
    // arbitrary authored speaker layout from the channel count.
    if children.len() != 1 || children[0].kind != *b"chan" {
        return Err(MetadataError::Unsupported(
            "QuickTime PCM description extensions are unsupported",
        ));
    }
    let layout = children[0].payload;
    if layout.len() < 16 || layout[..4] != [0; 4] {
        return Err(MetadataError::Malformed(
            "QuickTime PCM channel layout is truncated or invalid",
        ));
    }
    let expected_tag = if channels == 1 {
        0x0064_0001
    } else {
        0x0065_0002
    };
    if read_u32(layout, 4, "QuickTime PCM channel tag is truncated")? != expected_tag
        || layout[8..].iter().any(|&byte| byte != 0)
    {
        return Err(MetadataError::Unsupported(
            "QuickTime PCM channel layout is unsupported",
        ));
    }
    Ok(())
}

pub(super) fn require_timecode_description(
    payload: &[u8],
    media: MediaHeader,
    timing: Timing,
    budget: &mut AtomBudget,
) -> Result<(), MetadataError> {
    let entry = sample_description(payload, budget)?;
    if entry.kind != *b"tmcd" {
        return Err(MetadataError::Unsupported(
            "QuickTime timecode sample entry is unsupported",
        ));
    }
    let description = entry.payload;
    if description.len() != 26 {
        return Err(MetadataError::Malformed(
            "QuickTime timecode description length is invalid",
        ));
    }
    let scale = read_u32(description, 16, "QuickTime timecode timescale is truncated")?;
    let frame_duration = read_u32(
        description,
        20,
        "QuickTime timecode frame duration is truncated",
    )?;
    let frames = u32::from(description[24]);
    if read_u16(
        description,
        6,
        "QuickTime timecode data reference is truncated",
    )? != 1
        || description[..6] != [0; 6]
        || description[8..16] != [0; 8]
        || description[25] != 0
    {
        return Err(MetadataError::Unsupported(
            "QuickTime timecode flags or storage profile is unsupported",
        ));
    }
    if scale == 0
        || frame_duration == 0
        || frames == 0
        || frame_duration.checked_mul(frames) != Some(scale)
        || scale != media.timescale
        || !timing.sample_delta.is_multiple_of(frame_duration)
    {
        return Err(MetadataError::Malformed(
            "QuickTime timecode frame clock is inconsistent",
        ));
    }
    Ok(())
}
