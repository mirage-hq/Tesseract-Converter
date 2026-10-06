//! Project native half-open intervals onto the saved sequence's root samples.
//! This preserves the sampled picture, not arbitrary-rate or sub-frame timing.
use crate::{
    error::{ensure, unsupported, Result},
    schema::FrameRate,
};
use fx_schema::Time;

/// The consumer rounds `frame / fps` to integer milliseconds before Group
/// evaluation. Use that same operation, after exact native sample selection.
pub(crate) fn sample_time(frame: i64, rate: FrameRate) -> Result<Time> {
    ensure!(frame >= 0, "negative numbered-image sequence sample");
    let (numerator, denominator) = rate.frames_per_second();
    let fps = numerator as f64 / denominator as f64;
    let time = Time::from_secs(frame as f64 / fps);
    ensure!(
        time != Time::MAX,
        "numbered-image sample exceeds the editable clock"
    );
    Ok(time)
}

/// First saved sequence sample at or after the exact native boundary. Shared
/// cuts use this same projection, regardless of which occurrence owns them.
pub(crate) fn boundary(ticks: i64, rate: FrameRate) -> Result<Time> {
    ensure!(ticks >= 0, "negative numbered-image boundary");
    let step = i128::from(rate.ticks_per_frame());
    let frame = i64::try_from((i128::from(ticks) + step - 1) / step)
        .map_err(|_| unsupported("numbered-image sample index overflows"))?;
    let time = sample_time(frame, rate)?;
    if frame > 0 {
        ensure!(
            sample_time(frame - 1, rate)? < time,
            "numbered-image sequence samples collide on the editable clock"
        );
    }
    Ok(time)
}

pub(crate) struct SampledOccurrence {
    pub(crate) window: fx_schema::TimeRangeProperty,
    pub(crate) frames: Vec<(usize, fx_schema::TimeRangeProperty)>,
}

pub(crate) fn occurrence(
    clip: &crate::schema::PrVideoOccurrence,
    source: &crate::schema::PrVideoStream,
    rate: FrameRate,
) -> Result<SampledOccurrence> {
    use fx_schema::{Duration, TimeRangeProperty};
    let start = boundary(clip.start_ticks, rate)?;
    let end = boundary(clip.end_ticks, rate)?;
    ensure!(
        end > start,
        "numbered-image occurrence contains no sequence sample"
    );
    // Moving an animated owner's zero would change its unchanged keys. Keep
    // the existing unit clock or diagnose this occurrence rather than retime it.
    let rounded_start = (i128::from(clip.start_ticks)
        + i128::from(crate::schema::TICKS_PER_MILLISECOND) / 2)
        / i128::from(crate::schema::TICKS_PER_MILLISECOND);
    ensure!(
        clip.animations.is_empty() || i128::from(start.as_millis()) == rounded_start,
        "sequence-sampled placement would move the numbered-image Motion/Opacity key origin"
    );
    let window = TimeRangeProperty::new(
        start,
        Duration::from_millis(end.as_millis() - start.as_millis()),
    );
    let step = i128::from(source.frame_rate.ticks_per_frame());
    let mut frames = Vec::new();
    for index in 0..super::frame_count(source)? {
        let source_start = (index as i128 * step).max(i128::from(clip.in_ticks));
        let source_end = ((index as i128 + 1) * step).min(i128::from(clip.out_ticks));
        if source_start >= source_end {
            continue;
        }
        let placed = |source_time: i128| -> Result<Time> {
            let ticks = i128::from(clip.start_ticks) + source_time - i128::from(clip.in_ticks);
            let ticks = i64::try_from(ticks)
                .map_err(|_| unsupported("numbered-image placement overflows"))?;
            boundary(ticks.clamp(clip.start_ticks, clip.end_ticks), rate)
        };
        let left = placed(source_start)?;
        let right = placed(source_end)?;
        if left == right {
            continue;
        }
        ensure!(
            left >= start && right <= end && left < right,
            "numbered-image sampled interval exceeds its owner"
        );
        frames.push((
            index,
            TimeRangeProperty::new(
                Time::from_millis(left.as_millis() - start.as_millis()),
                Duration::from_millis(right.as_millis() - left.as_millis()),
            ),
        ));
    }
    ensure!(
        !frames.is_empty(),
        "numbered-image source covers no selected sequence samples"
    );
    Ok(SampledOccurrence { window, frames })
}

#[cfg(test)]
mod object_mask_tests {
    use super::*;
    use crate::{
        schema::SourceFrameRate,
        tests::support::{video_media, video_sequence},
    };

    #[test]
    fn object_mask_exact_204_frame_source_repeats_on_30fps_grid_without_clock_drift() {
        let step = 8_511_237_907;
        let mut sequence = video_sequence();
        let clip = sequence.video_tracks[0].clip_mut(0);
        clip.in_ticks = 0;
        clip.out_ticks = 204 * step;
        clip.end_ticks = clip.out_ticks;
        let mut media = video_media();
        let source = media.values_mut().next().unwrap().video.as_mut().unwrap();
        source.frame_rate = SourceFrameRate::from_ticks_per_frame(step).unwrap();
        source.intrinsic_ticks = 204 * step;
        let sampled = occurrence(clip, source, FrameRate::Fps30).unwrap();
        assert_eq!(sampled.frames.len(), 204);
        let mut selected = Vec::new();
        for sample in 0..206 {
            let time = sample_time(sample, FrameRate::Fps30).unwrap().as_millis();
            let index = sampled
                .frames
                .iter()
                .find(|(_, range)| {
                    time >= range.start.as_millis()
                        && time < range.start.as_millis() + range.duration.as_millis()
                })
                .unwrap()
                .0;
            assert_eq!(
                index as i64,
                sample * FrameRate::Fps30.ticks_per_frame() / step
            );
            selected.push(index);
        }
        assert_eq!(&selected[..3], &[0, 0, 1]);
        assert_eq!(selected.last(), Some(&203));
        assert_eq!(
            selected
                .windows(2)
                .filter(|pair| pair[0] == pair[1])
                .count(),
            2
        );
        // Actual subject.mov SHA 5d9e6c34…: ffprobe presentation times,
        // indices 0..2 and 190..194. FX Time::from_secs rounds to ms; video_time_us
        // scales by 1000, then FFmpeg display_index_position and VideoToolbox
        // bridge_precise_request_time_us both add 499us before the floor walk.
        // These are real residuals, NOT corrected by shifting every matte.
        let early_pts = [0_u64, 33_510, 67_020];
        let late_pts = [6_366_770_u64, 6_400_280, 6_433_790, 6_467_290, 6_500_800];
        for (sample, pts, base, expected_picture, expected_matte) in [
            (2, early_pts.as_slice(), 0, 2, 1),
            (193, late_pts.as_slice(), 190, 191, 192),
            (194, late_pts.as_slice(), 190, 193, 192),
        ] {
            let request = sample_time(sample, FrameRate::Fps30).unwrap().as_millis() * 1000 + 499;
            let picture = base + pts.partition_point(|pts| *pts <= request) - 1;
            assert_eq!(picture, expected_picture);
            assert_eq!(selected[sample as usize], expected_matte);
            assert_ne!(picture, selected[sample as usize]);
        }
        assert_eq!(source.frame_rate.ticks_per_frame(), step);
        assert_eq!(clip.out_ticks, 1_736_292_533_028);
    }
}
