use crate::{
    error::{ensure, unsupported, BuildError, Result},
    format::{FrameRate, PrMedia},
    image_media::{inspect_image_media, ImageFormat, ValidatedImage},
    media_metadata::ColourDescription,
    schema::{HdrProfile, PrMediaKind, PrVideoStream, VideoCodec},
    video_format::VideoFormat,
};
use std::{
    io::{Read, Seek},
    ops::Range,
    path::Path,
};
use tesseract_file::AssetKind;

/// A media file container that conversion packages, named by its file
/// extension in any letter case. Each media kind admits its own containers
/// (see [`admitted_container`]); parsing checks the content separately.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MediaContainer {
    Mp4,
    Mov,
    M4a,
    Wav,
    Mp3,
    Image(ImageFormat),
}

impl MediaContainer {
    pub(crate) fn from_extension(extension: &str) -> Option<Self> {
        match extension.to_ascii_lowercase().as_str() {
            "mp4" => Some(Self::Mp4),
            "mov" => Some(Self::Mov),
            "m4a" => Some(Self::M4a),
            "wav" => Some(Self::Wav),
            "mp3" => Some(Self::Mp3),
            other => ImageFormat::from_extension(other).map(Self::Image),
        }
    }

    /// The container that a file name declares.
    pub(crate) fn from_path(path: &Path) -> Option<Self> {
        path.extension()
            .and_then(|extension| extension.to_str())
            .and_then(Self::from_extension)
    }

    /// MP4 and MOV, the containers that video media may use. Both the
    /// renderer's video and audio asset lookups resolve their extensions.
    pub(crate) fn holds_video(self) -> bool {
        matches!(self, Self::Mp4 | Self::Mov)
    }

    /// Whether a media record whose video stream has `kind` may use this
    /// container. `None` is a sound-only record, which any audio or video
    /// container can hold.
    fn admits(self, kind: Option<PrMediaKind>) -> bool {
        match kind {
            Some(PrMediaKind::Video { .. }) => self.holds_video(),
            Some(PrMediaKind::Still { .. }) => matches!(self, Self::Image(_)),
            // Generators have no file; linked AEPs are not renderable FX assets.
            Some(
                PrMediaKind::ColorMatte(_)
                | PrMediaKind::Adjustment
                | PrMediaKind::AfterEffectsComposition(_),
            ) => false,
            None => !matches!(self, Self::Image(_)),
        }
    }

    /// The package content type, as `tesseract_file` derives it from the name.
    pub(crate) fn content_type(self) -> &'static str {
        match self {
            Self::Mp4 => "video/mp4",
            Self::Mov => "video/quicktime",
            Self::M4a => "audio/mp4",
            Self::Wav => "audio/wav",
            Self::Mp3 => "audio/mpeg",
            Self::Image(format) => format.content_type(),
        }
    }

    /// The Tesseract asset kind that export requires for this container.
    pub(crate) fn asset_kind(self) -> AssetKind {
        match self {
            Self::Mp4 | Self::Mov => AssetKind::Video,
            Self::M4a | Self::Wav | Self::Mp3 => AssetKind::Audio,
            Self::Image(_) => AssetKind::Image,
        }
    }
}

/// The container of the file named `file` that holds `media`, or `None` when
/// conversion does not accept that file type for the media's kind: MP4/MOV
/// video, PNG/JPEG stills, and MP4/MOV/M4A/WAV/MP3 sound-only sources.
/// Import and asset package binding use this rule. The writer separately
/// validates linked AEP paths, which must never become ordinary FX assets.
pub(crate) fn admitted_container(media: &PrMedia, file: &Path) -> Option<MediaContainer> {
    MediaContainer::from_path(file)
        .filter(|container| container.admits(media.video.as_ref().map(|video| video.kind)))
}

impl PrMedia {
    /// Whether this is generator media (a Color Matte or an adjustment layer's
    /// Black Video), which has no file to package.
    pub(crate) fn is_generator(&self) -> bool {
        self.video.as_ref().is_some_and(|video| {
            matches!(
                video.kind,
                PrMediaKind::ColorMatte(_) | PrMediaKind::Adjustment
            )
        })
    }
}

/// Inspected facts for one media record, routed by the kind its native or
/// document record declares. A still record must name a PNG/JPEG file and a
/// video record must name an MP4/MOV; a mismatch is unsupported.
#[derive(Debug)]
pub(crate) enum MediaFacts {
    Video(VideoMedia),
    Still(ValidatedImage),
}

impl MediaFacts {
    pub(crate) fn dimensions(&self) -> (u32, u32) {
        match self {
            Self::Video(video) => (video.width, video.height),
            Self::Still(image) => (image.width, image.height),
        }
    }

    /// Compare the frame rate and duration of a native media record with the file.
    /// A still has no media clock: its native still `FrameRate` and synthetic
    /// `Duration` have no counterpart in the image file.
    pub(crate) fn validate_source(&self, source: &PrVideoStream) -> Result<()> {
        match self {
            Self::Video(video) => video.validate_source(source),
            Self::Still(_) => Ok(()),
        }
    }
}

/// Splits a failed media inspection into source content that conversion does
/// not support, whose reason omits the media's placements, and an operational
/// failure that stops the conversion. A failed read stays fatal; an audio
/// stream that ends early is malformed content, as for stills.
pub(crate) fn unsupported_media_reason(error: BuildError) -> Result<String> {
    use media_transcode::inspect::InspectError;
    use symphonia::core::errors::Error as AudioError;
    match error {
        BuildError::Mp4(InspectError::Io(error)) => Err(error.into()),
        BuildError::Audio(AudioError::IoError(error))
            if error.kind() != std::io::ErrorKind::UnexpectedEof =>
        {
            Err(error.into())
        }
        error @ (BuildError::Unsupported(_) | BuildError::Mp4(_) | BuildError::Audio(_)) => {
            Ok(error.to_string())
        }
        error => Err(error),
    }
}

/// The selected picture intervals on the source clock. Construction proves
/// unit playback through every containing nest; sound is independently owned.
pub(crate) struct VideoUse {
    pub(crate) ranges: Vec<Range<i64>>,
    pub(crate) uses_audio: bool,
}

/// Inspect one media source according to the kind its record declares.
pub(crate) fn inspect_media(
    kind: PrMediaKind,
    reader: impl Read + Seek,
    metadata_reader: impl Read + Seek,
    size: u64,
    usage: Option<&VideoUse>,
) -> Result<MediaFacts> {
    match kind {
        PrMediaKind::Video { .. } => {
            inspect_video_for_use(reader, metadata_reader, size, usage).map(MediaFacts::Video)
        }
        PrMediaKind::Still { .. } => inspect_image_media(reader).map(MediaFacts::Still),
        PrMediaKind::AfterEffectsComposition(_) => Err(unsupported(
            "linked After Effects composition requires editable AEP resolution; it is not a decoded video asset",
        )),
        PrMediaKind::ColorMatte(_) => Err(unsupported(
            "Color Matte generator media has no file to inspect",
        )),
        PrMediaKind::Adjustment => Err(unsupported(
            "adjustment layer generator media has no file to inspect",
        )),
    }
}

/// Inspect one video source before checking its individual occurrences.
/// The second reader exposes container metadata normalized by the demuxer.
pub(crate) fn inspect_video_media(
    reader: impl Read + Seek,
    metadata_reader: impl Read + Seek,
    size: u64,
) -> Result<VideoMedia> {
    inspect_video_for_use(reader, metadata_reader, size, None)
}

/// [`inspect_video_media`] for the selected `usage` of the source; `None`
/// keeps the whole-source checks.
pub(crate) fn inspect_video_for_use(
    reader: impl Read + Seek,
    mut metadata_reader: impl Read + Seek,
    size: u64,
    usage: Option<&VideoUse>,
) -> Result<VideoMedia> {
    use media_transcode::inspect::{inspect, StreamKind};
    let metadata = crate::media_metadata::read_movie_metadata(&mut metadata_reader, size, true)?;
    // Diagnose unsupported HEVC structure before libavformat can reject it with
    // a generic demux error. This is parameter-set inspection, not decoding.
    for description in metadata
        .tracks
        .iter()
        .filter_map(|track| track.sample_description.as_ref())
    {
        if matches!(&description.entry, b"hvc1" | b"hev1") {
            crate::video_format::validate_codec(
                description.entry,
                &[],
                description,
                &mut metadata_reader,
            )?;
        }
    }
    let inspection = inspect(reader, size, true)?;
    let mut videos = inspection
        .streams
        .iter()
        .filter(|stream| stream.kind == StreamKind::Video);
    let Some(stream) = videos.next() else {
        return Err(unsupported("source requires exactly one video stream"));
    };
    ensure!(
        videos.next().is_none(),
        "source requires exactly one video stream"
    );
    ensure!(
        inspection
            .streams
            .iter()
            .filter(|stream| stream.kind == StreamKind::Audio)
            .count()
            <= 1
            && metadata
                .tracks
                .iter()
                .filter(|track| track.handler == *b"soun")
                .count()
                <= 1
            || usage.is_some_and(|usage| !usage.uses_audio),
        "multiple audio streams are unsupported when sound is consumed or usage is unknown"
    );
    for track in &metadata.tracks {
        match &track.handler {
            b"sbtl" | b"text" | b"subt" | b"clcp" => {
                return Err(unsupported(format!(
                    "subtitle or caption tracks ({}) are unsupported",
                    String::from_utf8_lossy(&track.handler)
                )));
            }
            _ => {}
        }
    }
    let mut video_metadata = metadata
        .tracks
        .iter()
        .filter(|track| track.handler == *b"vide");
    let Some(track) = video_metadata.next() else {
        return Err(unsupported("source requires exactly one video stream"));
    };
    ensure!(
        video_metadata.next().is_none(),
        "source requires exactly one video stream"
    );
    let description = track
        .sample_description
        .as_ref()
        .ok_or_else(|| unsupported("missing video sample description"))?;
    ensure!(
        (stream.width, stream.height)
            == (u32::from(description.width), u32::from(description.height)),
        "MP4 sample-entry dimensions must match the decoded stream dimensions"
    );
    let format = crate::video_format::validate_codec(
        stream.codec_tag,
        &stream.extradata,
        description,
        &mut metadata_reader,
    )?;
    validate_media_timing(
        stream,
        &inspection.packets,
        track,
        metadata.timescale,
        size,
        format,
        usage,
    )
}

#[derive(Debug)]
pub(crate) struct VideoMedia {
    pub(crate) codec: VideoCodec,
    /// Luma and chroma bit depth, 8 or 10.
    pub(crate) bit_depth: u8,
    /// The non-BT.709 colour that passes through, reported once per file.
    pub(crate) colour: Option<ColourDescription>,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) timing: VideoTiming,
    /// The unmirrored quarter turn of the display matrix.
    pub(crate) orientation: crate::schema::VideoOrientation,
}

/// The source interval that a selected unit use may consume: from the first
/// physical presentation time, which the first playback edit starts at, to
/// the start of the last sample, whose duration is uncertain, and within that
/// first edit.
#[derive(Debug, Clone, Copy)]
pub(crate) struct VideoWindow {
    /// The last sample's presentation start after the first, in `timescale`
    /// units.
    last_start: i128,
    timescale: u32,
    /// The first edit's duration and the movie timescale, when the file has
    /// an edit list.
    edit_duration: Option<(u64, u32)>,
}

impl VideoWindow {
    fn validate(&self, ranges: &[Range<i64>]) -> Result<()> {
        ensure!(
            !ranges.is_empty(),
            "selected picture has no proved unit source intervals"
        );
        let ticks_per_second = i128::from(crate::schema::TICKS);
        for range in ranges {
            // A selection past the first edit names the edit list, which is
            // the cause even when it also reaches the uncertain final sample.
            if let Some((duration, movie_scale)) = self.edit_duration {
                ensure!(
                    i128::from(range.end) * i128::from(movie_scale)
                        <= i128::from(duration) * ticks_per_second,
                    "selected picture interval crosses the first MP4 edit list segment; later segments are unsupported"
                );
            }
            let source_end = i128::from(range.end) * i128::from(self.timescale);
            ensure!(
                range.start >= 0
                    && range.end > range.start
                    && source_end <= self.last_start * ticks_per_second,
                "selected picture interval is outside the proved physical presentation window (uncertain final sample excluded)"
            );
        }
        Ok(())
    }
}

/// The sample clock proved by container inspection. A quantized clock has
/// exact rounded frame boundaries, rather than a universal sample duration.
/// Irregular presentation has only a physical endpoint, no nominal frame grid.
#[derive(Debug, Clone, Copy)]
pub(crate) struct VideoTiming {
    pub(crate) clock: SampleClock,
    pub(crate) timescale: u32,
    pub(crate) sample_count: u32,
    /// What a selected use may consume when only proved intervals import.
    pub(crate) window: VideoWindow,
    /// Whether the edit list keeps only part of the source presentation, so
    /// only proved selected intervals import.
    pub(crate) partial_timeline: bool,
    /// Whether version 0 composition offsets are read as signed values.
    pub(crate) legacy_signed_ctts: bool,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum SampleClock {
    Constant { sample_duration: u32 },
    Quantized { nominal: FrameRate },
    Irregular { media_end: u64 },
}

impl SampleClock {
    /// A boundary of an established constant/quantized grid, rounded half up.
    /// Irregular presentation has no index-to-time formula.
    fn boundary(self, index: u32, timescale: u32) -> Result<u64> {
        let units = match self {
            Self::Constant { sample_duration } => u128::from(index) * u128::from(sample_duration),
            Self::Irregular { .. } => {
                return Err(unsupported(
                    "irregular presentation has no nominal sample boundary",
                ));
            }
            Self::Quantized { nominal } => {
                let (numerator, denominator) = nominal.frames_per_second();
                let scaled = u128::from(index)
                    .checked_mul(u128::from(timescale))
                    .and_then(|value| value.checked_mul(u128::from(denominator)))
                    .ok_or_else(|| unsupported("media sample boundary overflows"))?;
                scaled
                    .checked_add(u128::from(numerator / 2))
                    .ok_or_else(|| unsupported("media sample boundary overflows"))?
                    / u128::from(numerator)
            }
        };
        u64::try_from(units).map_err(|_| unsupported("media sample boundary overflows"))
    }
}

impl VideoTiming {
    /// Export remains restricted to exact constant clocks at listed rates.
    pub(crate) fn supported(&self) -> Result<(FrameRate, i64)> {
        let SampleClock::Constant { sample_duration } = self.clock else {
            return Err(unsupported(match self.clock {
                SampleClock::Irregular { .. } => {
                    "irregular video presentation clocks are unsupported for export"
                }
                _ => "quantized video sample clocks are unsupported for export",
            }));
        };
        let rate = FrameRate::from_seconds_per_frame(sample_duration, self.timescale).ok_or_else(
            || {
                unsupported(format!(
                    "unsupported video frame rate ({}/{} seconds per frame)",
                    sample_duration, self.timescale
                ))
            },
        )?;
        let duration = i64::from(self.sample_count)
            .checked_mul(rate.ticks_per_frame())
            .ok_or_else(|| unsupported("media duration exceeds Premiere's tick range"))?;
        Ok((rate, duration))
    }

    #[cfg(test)]
    pub(crate) fn for_test(rate: FrameRate, duration_ticks: i64) -> Self {
        let (timescale, sample_duration) = rate.frames_per_second();
        assert_eq!(duration_ticks % rate.ticks_per_frame(), 0);
        let sample_count = u32::try_from(duration_ticks / rate.ticks_per_frame()).unwrap();
        Self {
            clock: SampleClock::Constant { sample_duration },
            timescale,
            sample_count,
            window: VideoWindow {
                last_start: i128::from(sample_count.saturating_sub(1))
                    * i128::from(sample_duration),
                timescale,
                edit_duration: None,
            },
            partial_timeline: false,
            legacy_signed_ctts: false,
        }
    }

    fn duration_millis(&self) -> Result<i128> {
        let endpoint = match self.clock {
            SampleClock::Irregular { media_end } => media_end,
            clock => clock.boundary(self.sample_count, self.timescale)?,
        };
        let numerator = i128::from(endpoint)
            .checked_mul(1000)
            .ok_or_else(|| unsupported("media duration overflows"))?;
        round_duration_millis(numerator, i128::from(self.timescale))
    }
}

/// Classify a nonconstant decode grid only when one supported nominal rate
/// explains every sample start and the endpoint in the file's time base.
fn matching_quantized_clock(
    presentation: &[i128],
    media_end: u64,
    timescale: u32,
) -> Result<Option<SampleClock>> {
    let count = u32::try_from(presentation.len())
        .map_err(|_| unsupported("media sample count overflows"))?;
    let mut matches = FrameRate::ALL.into_iter().filter(|nominal| {
        let clock = SampleClock::Quantized { nominal: *nominal };
        clock
            .boundary(count, timescale)
            .is_ok_and(|end| end == media_end)
            && presentation.iter().enumerate().all(|(index, time)| {
                clock
                    .boundary(index as u32, timescale)
                    .is_ok_and(|boundary| i128::from(boundary) == *time)
            })
    });
    let Some(nominal) = matches.next() else {
        return Ok(None);
    };
    ensure!(
        matches.next().is_none(),
        "MP4 quantized frame grid is ambiguous"
    );
    Ok(Some(SampleClock::Quantized { nominal }))
}

#[cfg(test)]
fn quantized_clock(presentation: &[i128], media_end: u64, timescale: u32) -> Result<SampleClock> {
    matching_quantized_clock(presentation, media_end, timescale)?.ok_or_else(|| {
        unsupported("MP4 sample durations must be constant or match one exact quantized frame grid")
    })
}

/// Round a nonnegative rational duration directly, with ties going forward.
fn round_duration_millis(numerator: i128, denominator: i128) -> Result<i128> {
    ensure!(numerator >= 0 && denominator > 0, "invalid media duration");
    numerator
        .checked_add(denominator / 2)
        .and_then(|value| value.checked_div(denominator))
        .ok_or_else(|| unsupported("media duration overflows"))
}

fn packet_time_in_track_units(
    value: i64,
    stream: &media_transcode::inspect::StreamInfo,
    timescale: u32,
) -> Result<i128> {
    let numerator = i128::from(value)
        .checked_mul(i128::from(stream.time_base_num))
        .and_then(|value| value.checked_mul(i128::from(timescale)))
        .ok_or_else(|| unsupported("video packet timestamp overflows"))?;
    let denominator = i128::from(stream.time_base_den);
    ensure!(
        numerator % denominator == 0,
        "video packet timestamp cannot be represented in the MP4 media timescale"
    );
    Ok(numerator / denominator)
}

fn validate_media_timing(
    stream: &media_transcode::inspect::StreamInfo,
    packets: &[media_transcode::inspect::PacketInfo],
    track: &crate::media_metadata::TrackMetadata,
    movie_timescale: u32,
    file_size: u64,
    format: VideoFormat,
    usage: Option<&VideoUse>,
) -> Result<VideoMedia> {
    ensure!(
        stream.time_base_num > 0 && stream.time_base_den > 0 && track.timescale > 0,
        "packaged MP4 has an invalid video time base"
    );
    let timing = track
        .sample_timing
        .ok_or_else(|| unsupported("missing video sample timing"))?;
    ensure!(
        timing.sample_count > 0 && timing.media_end == track.duration,
        "packaged MP4 declared duration differs from its exact sample timeline"
    );
    let packets: Vec<_> = packets
        .iter()
        .filter(|packet| packet.stream_index == stream.index)
        .collect();
    ensure!(
        usize::try_from(timing.sample_count) == Ok(packets.len()),
        "packaged MP4 declared sample count differs from its packet timeline"
    );
    let mut presentation = Vec::new();
    presentation
        .try_reserve(packets.len())
        .map_err(|_| unsupported("video sample timeline exceeds host address space"))?;
    let mut dts_origin = None;
    let mut previous_dts = None;
    for packet in &packets {
        let dts = packet
            .dts
            .ok_or_else(|| unsupported("packaged MP4 sample timeline has no decode timestamp"))?;
        let pts = packet.pts.ok_or_else(|| {
            unsupported("packaged MP4 sample timeline has no presentation timestamp")
        })?;
        let absolute_dts = packet_time_in_track_units(dts, stream, track.timescale)?;
        let origin = *dts_origin.get_or_insert(absolute_dts);
        let dts = absolute_dts
            .checked_sub(origin)
            .ok_or_else(|| unsupported("video packet timestamp overflows"))?;
        ensure!(
            previous_dts.map_or(dts == 0, |previous| previous < dts),
            "packaged MP4 sample timeline is empty or discontinuous"
        );
        if let (Some(previous), Some(expected)) = (previous_dts, timing.constant_duration) {
            ensure!(
                dts - previous == i128::from(expected),
                "packaged MP4 packet duration differs from its sample timing table"
            );
        }
        previous_dts = Some(dts);
        ensure!(
            packet.position >= 0 && packet.size > 0,
            "packaged MP4 sample byte range is empty or outside the file"
        );
        let start = u64::try_from(packet.position).map_err(|_| {
            unsupported("packaged MP4 sample byte range is empty or outside the file")
        })?;
        let length = u64::try_from(packet.size).map_err(|_| {
            unsupported("packaged MP4 sample byte range exceeds host address space")
        })?;
        ensure!(
            start
                .checked_add(length)
                .is_some_and(|end| end <= file_size),
            "packaged MP4 sample byte range is empty or outside the file"
        );
        presentation.push(packet_time_in_track_units(pts, stream, track.timescale)?);
    }
    let final_dts = previous_dts.expect("nonempty packet timeline");
    ensure!(
        final_dts < i128::from(timing.media_end)
            && timing.constant_duration.is_none_or(|duration| {
                final_dts + i128::from(duration) == i128::from(timing.media_end)
            })
            && (stream.duration <= 0
                || packet_time_in_track_units(stream.duration, stream, track.timescale)?
                    == i128::from(timing.media_end))
            && (stream.frame_count <= 0
                || usize::try_from(stream.frame_count) == Ok(packets.len())),
        "packaged MP4 declared duration differs from its exact sample timeline"
    );

    let timescale = track.timescale;
    let sample_count = timing.sample_count;
    let media_end = timing.media_end;
    let constant = timing.constant_duration.is_some();
    let frame_duration = timing.constant_duration;
    let last_duration = timing.final_duration;
    let zero_composition_offsets = timing.zero_composition_offsets;
    let legacy_signed_ctts = timing.legacy_signed_ctts;
    presentation.sort_unstable();
    let first_presentation = presentation[0];
    // The latest presentation start after the first; `presentation` is sorted.
    let last_start = presentation[sample_count as usize - 1] - first_presentation;
    let unique_presentation = presentation.windows(2).all(|pair| pair[0] < pair[1]);
    let clock = if constant {
        let frame_duration = frame_duration.expect("nonempty sample timeline");
        let step = i128::from(frame_duration);
        let on_grid = presentation
            .iter()
            .enumerate()
            .all(|(index, time)| *time == first_presentation + index as i128 * step);
        ensure!(
            on_grid || usage.is_some(),
            "MP4 presentation samples must form an exact constant frame grid"
        );
        ensure!(
            on_grid || unique_presentation,
            "MP4 presentation timestamps must be unique"
        );
        if on_grid {
            SampleClock::Constant {
                sample_duration: frame_duration,
            }
        } else {
            SampleClock::Irregular { media_end }
        }
    } else {
        // Decode durations are not sorted presentation intervals on B-frame
        // sources. Normalize only after retaining the origin for edit validation.
        let normalized: Vec<_> = presentation
            .iter()
            .map(|time| *time - first_presentation)
            .collect();
        ensure!(
            unique_presentation,
            "MP4 presentation timestamps must be unique"
        );
        ensure!(
            last_start < i128::from(media_end),
            "MP4 normalized presentation timestamp is outside its physical endpoint"
        );
        // An ambiguous supported grid is a terminal error, never irregular.
        if let Some(clock) = matching_quantized_clock(&normalized, media_end, timescale)? {
            ensure!(
                zero_composition_offsets,
                "quantized MP4 sample clocks require zero composition offsets"
            );
            clock
        } else {
            ensure!(
                first_presentation >= 0,
                "irregular MP4 presentation origin cannot use an empty/negative playback edit"
            );
            SampleClock::Irregular { media_end }
        }
    };
    if legacy_signed_ctts {
        ensure!(
            first_presentation >= 0 && unique_presentation && last_start < i128::from(media_end),
            "legacy signed CTTSv0 requires unique physical PTS inside the exact decode endpoint"
        );
        ensure!(
            matches!(clock, SampleClock::Constant { .. }) || usage.is_some(),
            "irregular legacy signed CTTSv0 requires selected unit interior picture intervals"
        );
    }
    let final_presentation_interval = if matches!(clock, SampleClock::Irregular { .. }) {
        u64::try_from(i128::from(media_end) - last_start)
            .map_err(|_| unsupported("invalid final presentation interval"))?
    } else {
        u64::from(last_duration)
    };
    let edit = track.edit.as_deref();
    // Whole-source/export callers retain the original full-duration contract.
    let full_edit = (|| -> Result<()> {
        if let Some(edit) = edit {
            if movie_timescale == 0 || edit.len() != 1 {
                return Err(unsupported(
                    "MP4 edit list must contain one full-duration playback segment",
                ));
            }
            let entry = &edit[0];
            // Keep sample timing authoritative. A full-duration edit can round up
            // in the movie time base by less than one movie tick, or end inside
            // the last frame: IMG_2439 (iPhone) edits 1208/600 s of 49 frames of
            // 25/600 s, and Premiere 26.5.1 saves all 49 frames as its Duration.
            // No edit may hide an entire frame, have zero length, or shift the origin.
            let segment = u128::from(entry.segment_duration) * u128::from(timescale);
            let samples = u128::from(media_end) * u128::from(movie_timescale);
            let duration_error = segment.abs_diff(samples);
            if i128::from(entry.media_time) != first_presentation
                || entry.media_rate != 1
                || entry.media_rate_fraction != 0
                || entry.segment_duration == 0
                || (segment > samples && duration_error >= u128::from(timescale))
                || duration_error
                    >= u128::from(final_presentation_interval) * u128::from(movie_timescale)
            {
                return Err(unsupported(
                    "MP4 edit list changes the full source presentation timeline",
                ));
            }
        } else if first_presentation != 0 {
            return Err(unsupported(
                "MP4 presentation origin requires an explicit full-duration edit list",
            ));
        }

        Ok(())
    })();
    let partial_timeline = full_edit.is_err();
    if usage.is_none() {
        full_edit?;
    }
    let edit_duration = if let Some(edit) = edit {
        ensure!(
            movie_timescale > 0,
            "invalid MP4 playback edit clock or version"
        );
        let first = edit
            .first()
            .ok_or_else(|| unsupported("empty MP4 edit list"))?;
        ensure!(
            first.media_time >= 0
                && first.media_rate == 1
                && first.media_rate_fraction == 0
                && first.segment_duration > 0,
            "first MP4 edit list segment must be positive unit playback; empty, negative or retimed edits are unsupported"
        );
        ensure!(
            i128::from(first.media_time) == first_presentation,
            "first MP4 edit list origin must match the first physical presentation timestamp"
        );
        Some((first.segment_duration, movie_timescale))
    } else {
        ensure!(
            first_presentation == 0,
            "MP4 presentation origin requires an explicit playback edit"
        );
        None
    };
    // The presentation origin is the first edit's media time, so the window
    // starts at zero on the source clock.
    let window = VideoWindow {
        last_start,
        timescale,
        edit_duration,
    };
    if let Some(usage) = usage {
        if partial_timeline
            || (matches!(clock, SampleClock::Irregular { .. }) && (legacy_signed_ctts || constant))
        {
            window.validate(&usage.ranges)?;
        }
    }

    Ok(VideoMedia {
        codec: format.codec,
        bit_depth: format.bit_depth,
        colour: format.colour,
        width: stream.width,
        height: stream.height,
        orientation: format.orientation,
        timing: VideoTiming {
            clock,
            timescale,
            sample_count,
            window,
            partial_timeline,
            legacy_signed_ctts,
        },
    })
}

impl VideoMedia {
    /// The `OriginalColorSpace` profile that Premiere saves for this source, when
    /// its colour and depth are a measured combination (`HdrProfile`).
    pub(crate) fn hdr_profile(&self) -> Option<HdrProfile> {
        match (self.colour?.codes(), self.bit_depth) {
            ((9, 18, 9), 10) => Some(HdrProfile::Hlg10Bit),
            ((9, 16, 9), 10) => Some(HdrProfile::Pq10Bit),
            _ => None,
        }
    }

    /// [`Self::validate_source`] for the selected `usage`, returning whether
    /// only its proved intervals import.
    pub(crate) fn validate_source_for_use(
        &self,
        source: &PrVideoStream,
        usage: Option<&VideoUse>,
    ) -> Result<bool> {
        let Err(error) = self.validate_source(source) else {
            return Ok(self.timing.partial_timeline);
        };
        // Known frame grids retain their exact rate meaning. Only the
        // average endpoint of an unlisted native source may differ;
        // its orientation and frame count must still match, or the
        // whole-source error names the cause.
        let endpoint_only = self.orientation == source.orientation
            && source.frame_rate.supported().is_none()
            && i64::from(self.timing.sample_count).checked_mul(source.frame_rate.ticks_per_frame())
                == Some(source.intrinsic_ticks);
        match usage {
            Some(usage) if endpoint_only => {
                self.timing.window.validate(&usage.ranges)?;
                Ok(true)
            }
            _ => Err(error),
        }
    }

    /// Compare the orientation, frame rate and duration of a native video
    /// stream with the file.
    pub(crate) fn validate_source(&self, source: &PrVideoStream) -> Result<()> {
        ensure!(
            self.orientation == source.orientation,
            "native source orientation disagrees with the MP4 quarter-turn display matrix"
        );
        if matches!(self.timing.clock, SampleClock::Irregular { .. }) {
            ensure!(
                source.frame_rate.supported().is_none(),
                "irregular presentation requires an unlisted native VideoStream FrameRate; supported-rate interpretation is not inferred"
            );
        }
        if let (Some(native_rate), SampleClock::Constant { .. }) =
            (source.frame_rate.supported(), self.timing.clock)
        {
            let (file_rate, duration_ticks) = self.timing.supported()?;
            ensure!(
                file_rate == native_rate,
                "native VideoStream FrameRate {} differs from the file frame rate {}",
                source.frame_rate,
                file_rate
            );
            ensure!(
                duration_ticks == source.intrinsic_ticks,
                "native VideoStream Duration {} ticks differs from the file duration {} ticks",
                source.intrinsic_ticks,
                duration_ticks
            );
        } else {
            if let (Some(native_rate), SampleClock::Quantized { nominal }) =
                (source.frame_rate.supported(), self.timing.clock)
            {
                ensure!(
                    native_rate == nominal,
                    "native VideoStream FrameRate {} differs from the quantized file nominal rate {}",
                    native_rate,
                    nominal
                );
            }
            let native_duration = i64::from(self.timing.sample_count)
                .checked_mul(source.frame_rate.ticks_per_frame())
                .ok_or_else(|| {
                    unsupported("native source duration exceeds Premiere's tick range")
                })?;
            ensure!(
                native_duration == source.intrinsic_ticks,
                "native VideoStream Duration does not match its constant frame duration and file sample count"
            );
            let native_millis = round_duration_millis(
                i128::from(native_duration)
                    .checked_mul(1000)
                    .ok_or_else(|| unsupported("native source duration overflows"))?,
                i128::from(crate::schema::TICKS),
            )?;
            ensure!(
                native_millis == self.timing.duration_millis()?,
                "native VideoStream duration and file duration round to different milliseconds"
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod quantized_tests {
    use super::*;

    #[test]
    fn full_microsecond_30fps_grid_has_one_nominal_rate() {
        // Independent integer rounding for Veronica's complete 1131 samples.
        let starts: Vec<i128> = (0..1131).map(|n| (n * 1_000_000_i128 + 15) / 30).collect();
        let clock = quantized_clock(&starts, 37_700_000, 1_000_000).unwrap();
        assert!(matches!(
            clock,
            SampleClock::Quantized {
                nominal: FrameRate::Fps30
            }
        ));
        assert_eq!(clock.boundary(1131, 1_000_000).unwrap(), 37_700_000);
        let mut shifted = starts;
        shifted[1] += 1;
        assert!(quantized_clock(&shifted, 37_700_000, 1_000_000).is_err());
    }

    #[test]
    fn coarse_short_grid_must_not_choose_between_24_and_23_976() {
        assert!(quantized_clock(&[0, 42, 83], 125, 1000)
            .unwrap_err()
            .to_string()
            .contains("ambiguous"));
    }
}
