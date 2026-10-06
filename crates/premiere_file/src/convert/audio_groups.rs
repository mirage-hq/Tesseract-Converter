//! Export sound-only hierarchies as editable native clips, never black nests.

use fx_schema::{
    animator::{PropertyKeyframe, PropertyKeyframeTrack},
    AudioLayer, Duration, GroupLayer, LayerData, LayerPlayback, LayerPlaybackMapping, PropType,
    Time, TimeOffset, TimeRangeProperty,
};

use super::{
    linked_audio::Clock,
    nested::LayerExport,
    tesseract_to_premiere::{has_background, source_media, take_volume_track},
};
use crate::{
    approximate,
    error::{unsupported, Result},
    export_loss::OmissionSink,
};

pub(super) fn sound_only(group: &GroupLayer) -> bool {
    fn picture(group: &GroupLayer) -> bool {
        !group.is_hidden
            && (has_background(group)
                || group.layers.iter().any(|layer| match layer.data() {
                    LayerData::Audio(_) => false,
                    LayerData::Group(child) => picture(child),
                    _ => layer.wire_value()["isHidden"] != true,
                }))
    }
    fn sound(group: &GroupLayer) -> bool {
        group.layers.iter().any(|layer| match layer.data() {
            LayerData::Audio(_) => true,
            LayerData::Group(child) => sound(child),
            _ => false,
        })
    }
    !picture(group) && sound(group)
}

/// Source milliseconds minus parent milliseconds. A native audio clip has no
/// speed control; retain its source start and normal speed for other clocks.
fn offset(
    playback: &LayerPlayback,
    record: &str,
    omissions: &mut dyn OmissionSink,
) -> Result<i128> {
    let start = playback.input_range().start.as_millis() as f64;
    let clock = Clock {
        rate: 1.0,
        offset: 0.0,
    }
    .child(playback);
    let value = match clock {
        Some(clock) => {
            if (clock.rate - 1.0).abs() > 1e-12 {
                approximate(omissions, record, "sound clock exports at normal speed from its selected source start; authored playback speed is not retained");
            }
            clock.offset + (clock.rate - 1.0) * start
        }
        None => {
            approximate(omissions, record, "non-affine sound clock exports at normal speed from its first source key; holds, reverse and speed ramps are not retained");
            let LayerPlaybackMapping::TimeRemap { property } = playback.mapping() else {
                return Err(unsupported("invalid sound clock"));
            };
            property.keyframes()[0].value.as_millis() as f64 - start
        }
    };
    if !value.is_finite() || value.abs() > ((1_u64 << 53) - 1) as f64 {
        return Err(unsupported(
            "sound clock exceeds the exact millisecond range",
        ));
    }
    if value.fract() != 0.0 {
        approximate(
            omissions,
            record,
            "sound source start rounded to the nearest editable millisecond",
        );
    }
    Ok(value.round() as i128)
}

#[derive(Clone, Copy)]
struct Window {
    start: i128,
    end: i128,
    /// Current parent time = output-sequence time + offset.
    offset: i128,
}

impl Window {
    fn intersect(self, playback: &LayerPlayback) -> Self {
        let range = playback.input_range();
        Self {
            start: self
                .start
                .max(i128::from(range.start.as_millis()) - self.offset),
            end: self
                .end
                .min(i128::from(range.end().as_millis()) - self.offset),
            ..self
        }
    }
}

pub(super) fn export(
    group: &GroupLayer,
    context: &mut LayerExport<'_, '_>,
    omissions: &mut dyn OmissionSink,
) -> Result<()> {
    approximate(omissions, format!("layer {} ({:?})", group.id, group.name),
        "sound-only group exported as individually editable native audio clips; grouping and the original Dynamic Link relationship are not retained, and no opaque nest picture is added");
    visit(
        group,
        Window {
            start: 0,
            end: i128::from(u64::MAX),
            offset: 0,
        },
        context,
        omissions,
    )
}

fn visit(
    group: &GroupLayer,
    window: Window,
    context: &mut LayerExport<'_, '_>,
    omissions: &mut dyn OmissionSink,
) -> Result<()> {
    if group.is_hidden {
        return Ok(());
    }
    let mut window = window.intersect(&group.playback);
    if window.end <= window.start {
        return Ok(());
    }
    window.offset += offset(
        &group.playback,
        &format!("layer {} ({:?})", group.id, group.name),
        omissions,
    )?;
    for layer in &group.layers {
        match layer.data() {
            LayerData::Group(child) => visit(child, window, context, omissions)?,
            LayerData::Audio(sound) => export_sound(sound, window, context, omissions)?,
            _ => {} // sound_only established that these pictures are hidden.
        }
    }
    Ok(())
}

fn export_sound(
    sound: &AudioLayer,
    window: Window,
    context: &mut LayerExport<'_, '_>,
    omissions: &mut dyn OmissionSink,
) -> Result<()> {
    if sound.is_hidden {
        return Ok(());
    }
    let mut window = window.intersect(&sound.playback);
    if window.end <= window.start {
        return Ok(());
    }
    let record = format!("layer {} ({:?})", sound.id, sound.name);
    let source_offset = window.offset + offset(&sound.playback, &record, omissions)?;
    window.start = window
        .start
        .max(i128::from(sound.source_range.start.as_millis()) - source_offset);
    window.end = window.end.min(
        i128::from(
            sound
                .source_range
                .end()
                .as_millis()
                .min(sound.source_intrinsic_duration.as_millis()),
        ) - source_offset,
    );
    if window.end <= window.start {
        return Ok(());
    }
    let time = |millis: i128| {
        u64::try_from(millis)
            .map(Time::from_millis)
            .map_err(|_| unsupported("sound placement exceeds its time range"))
    };
    let duration = Duration::from_millis(
        u64::try_from(window.end - window.start)
            .map_err(|_| unsupported("sound duration exceeds its time range"))?,
    );
    let active = TimeRangeProperty::new(time(window.start)?, duration);
    let source = TimeRangeProperty::new(time(window.start + source_offset)?, duration);
    let input_start = match sound.playback.mapping() {
        LayerPlaybackMapping::Linear { input, .. } => input.start,
        LayerPlaybackMapping::TimeRemap { .. } => sound.playback.input_range().start,
    };
    let shift = window.start + window.offset + i128::from(sound.playback.input_offset_ms())
        - i128::from(input_start.as_millis());
    let track = take_volume_track(context.property_tracks, sound.id)
        .map(|track| {
            let keys = track
                .keyframes()
                .iter()
                .map(|key| {
                    let millis = i128::from(key.layer_time().as_millis()) - shift;
                    Ok(PropertyKeyframe::new(
                        key.id().clone(),
                        TimeOffset::from_millis(
                            i64::try_from(millis)
                                .map_err(|_| unsupported("sound key exceeds its time range"))?,
                        ),
                        key.value().clone(),
                        key.easing(),
                    ))
                })
                .collect::<Result<Vec<_>>>()?;
            PropertyKeyframeTrack::new(keys).map_err(|error| unsupported(error.to_string()))
        })
        .transpose()?;
    let mut placed = sound.clone();
    placed.parent = None;
    placed.source_range = source;
    placed.playback = LayerPlayback::linear(active, active, source, 0).map_err(unsupported)?;
    if let Some((occurrence, facts)) = super::audio::layer(
        &placed,
        None,
        track.as_ref(),
        context.audio_facts,
        omissions,
    )? {
        if occurrence.has_volume_animation() {
            context.written.record(sound.id, PropType::AudioVolume);
        }
        source_media(context.media, &occurrence.media).audio = Some(facts.clone());
        context.audio.push(occurrence);
    }
    Ok(())
}
