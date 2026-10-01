//! Audio stream facts from original bytes: layout, sample rate, and duration.
//!
//! MP4-family containers use the shared MP4 parser, whose edit list carries the
//! presentation duration Premiere uses (AAC priming excluded). WAV and MP3 are
//! demuxed, not decoded, with gapless trimming applied to packet times.

use crate::{
    error::{ensure, unsupported, Result},
    media::MediaContainer,
    schema::{AudioChannels, PrAudioStream, TICKS},
};
use std::io::{Read, Seek, SeekFrom};
use symphonia::core::{
    formats::FormatOptions,
    io::{MediaSource, MediaSourceStream},
    meta::MetadataOptions,
    probe::{Hint, Probe},
};

/// Seekable byte source for the WAV/MP3 demuxer.
struct Source<R> {
    reader: R,
    size: u64,
}

impl<R: Read> Read for Source<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.reader.read(buffer)
    }
}

impl<R: Seek> Seek for Source<R> {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        self.reader.seek(position)
    }
}

impl<R: Read + Seek + Send + Sync> MediaSource for Source<R> {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        Some(self.size)
    }
}

/// The sound of one packaged source that export plays: its stream facts, or
/// why conversion cannot export it. A source without sound has neither.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SourceSound {
    Supported(PrAudioStream),
    /// A movie sound that ends shortly before its picture. `stream` carries
    /// the picture duration Premiere records; `file_ticks` is the AAC length.
    PaddedToPicture {
        stream: PrAudioStream,
        file_ticks: i64,
    },
    Unsupported(String),
}

/// Returns `None` when an MP4-family source has no audio stream.
pub(crate) fn inspect_audio_media(
    reader: impl Read + Seek + Send + Sync + 'static,
    size: u64,
    extension: &str,
) -> Result<Option<PrAudioStream>> {
    match MediaContainer::from_extension(extension) {
        Some(MediaContainer::Mp4 | MediaContainer::Mov | MediaContainer::M4a) => {
            inspect_mp4(reader, size)
        }
        Some(MediaContainer::Wav | MediaContainer::Mp3) => inspect_stream(reader, size).map(Some),
        Some(MediaContainer::Image(_)) | None => {
            Err(unsupported("unsupported audio source extension"))
        }
    }
}

fn exact_ticks(units: u64, timescale: u32, context: &str) -> Result<i64> {
    ensure!(timescale > 0, "{context}: zero timescale");
    let numerator = i128::from(units) * i128::from(TICKS);
    ensure!(
        numerator % i128::from(timescale) == 0,
        "{context} is not representable in native ticks"
    );
    i64::try_from(numerator / i128::from(timescale))
        .map_err(|_| unsupported(format!("{context} overflows native ticks")))
}

fn channels(count: usize) -> Result<AudioChannels> {
    match count {
        1 => Ok(AudioChannels::Mono),
        2 => Ok(AudioChannels::Stereo),
        _ => Err(unsupported("only mono/stereo source audio is supported")),
    }
}

fn ticks_per_sample(sample_rate: u32) -> Result<i64> {
    ensure!(
        (8_000..=192_000).contains(&sample_rate) && TICKS % i64::from(sample_rate) == 0,
        "unsupported audio sample rate"
    );
    Ok(TICKS / i64::from(sample_rate))
}

/// A movie-clock duration in whole samples, nearest, as Premiere stores it.
fn nearest_samples(duration: u64, timescale: u32, sample_rate: u32) -> Result<u64> {
    ensure!(timescale > 0, "audio edit duration: zero timescale");
    let (timescale, scaled) = (
        u128::from(timescale),
        u128::from(duration) * u128::from(sample_rate),
    );
    u64::try_from((2 * scaled + timescale) / (2 * timescale))
        .map_err(|_| unsupported("audio edit duration overflows"))
}

fn stream(
    channels: AudioChannels,
    sample_rate: u32,
    intrinsic_ticks: i64,
) -> Result<PrAudioStream> {
    ticks_per_sample(sample_rate)?;
    ensure!(intrinsic_ticks > 0, "audio stream is empty");
    Ok(PrAudioStream {
        intrinsic_ticks,
        channels,
        sample_rate,
    })
}

fn inspect_mp4(mut reader: impl Read + Seek, size: u64) -> Result<Option<PrAudioStream>> {
    use media_transcode::inspect::{inspect, StreamKind};

    let metadata = crate::media_metadata::read_movie_metadata(&mut reader, size, false)?;
    reader.seek(SeekFrom::Start(0))?;
    let inspection = inspect(reader, size, false)?;
    let mut streams = inspection
        .streams
        .iter()
        .filter(|stream| stream.kind == StreamKind::Audio);
    let Some(media_stream) = streams.next() else {
        return Ok(None);
    };
    ensure!(
        streams.next().is_none(),
        "multiple audio streams are unsupported"
    );
    ensure!(
        media_stream.codec_name == "aac" && media_stream.codec_tag == *b"mp4a",
        "unsupported media type: MP4 audio must be AAC"
    );
    let mut tracks = metadata
        .tracks
        .iter()
        .filter(|track| track.handler == *b"soun");
    let Some(track) = tracks.next() else {
        return Err(unsupported("MP4 audio metadata is missing"));
    };
    ensure!(
        tracks.next().is_none(),
        "multiple audio streams are unsupported"
    );
    let channels = channels(
        usize::try_from(media_stream.channels)
            .map_err(|_| unsupported("audio channel count exceeds host address space"))?,
    )?;
    let sample_rate = media_stream.sample_rate;
    let media_ticks = exact_ticks(track.duration, track.timescale, "audio duration")?;
    let intrinsic_ticks = match &track.edit {
        None => media_ticks,
        Some(entries) => {
            let [entry] = entries.as_slice() else {
                return Err(unsupported(
                    "audio edit list must contain one playback segment",
                ));
            };
            ensure!(
                entry.media_rate == 1 && entry.media_rate_fraction == 0,
                "audio edit-list retiming is unsupported"
            );
            let edit_start = u64::try_from(entry.media_time)
                .map_err(|_| unsupported("audio edit list starts before the first sample"))?;
            let start = exact_ticks(edit_start, track.timescale, "audio edit start")?;
            ensure!(
                start < media_ticks,
                "audio edit list starts past the last sample"
            );
            let samples = nearest_samples(entry.segment_duration, metadata.timescale, sample_rate)?;
            let ticks_per_sample = ticks_per_sample(sample_rate)?;
            i64::try_from(samples)
                .ok()
                .and_then(|samples| samples.checked_mul(ticks_per_sample))
                .ok_or_else(|| unsupported("audio edit duration overflows native ticks"))?
        }
    };
    stream(channels, sample_rate, intrinsic_ticks).map(Some)
}

fn inspect_stream(
    mut reader: impl Read + Seek + Send + Sync + 'static,
    size: u64,
) -> Result<PrAudioStream> {
    // The probe scans for a RIFF or MP3 marker, so MP4 data saved under a
    // WAV or MP3 name fails with whatever the scan meets first; only that
    // failure is renamed. Bytes 4..8 hold an MP4 file's first box type, but a
    // RIFF header's size, which spells `ftyp` at 0x70797466 bytes. A read
    // error from this peek or from the probe stays an I/O error, and content
    // does not choose the demuxer.
    let mut head = Vec::with_capacity(8);
    reader.by_ref().take(8).read_to_end(&mut head)?;
    reader.seek(SeekFrom::Start(0))?;
    let mp4_file_type = head.get(..4) != Some(b"RIFF") && head.get(4..8) == Some(b"ftyp");
    let mut probe = Probe::default();
    probe.register_all::<symphonia::default::formats::WavReader>();
    probe.register_all::<symphonia::default::formats::MpaReader>();
    let source = MediaSourceStream::new(Box::new(Source { reader, size }), Default::default());
    let mut format = match probe.format(
        &Hint::new(),
        source,
        &FormatOptions {
            enable_gapless: true,
            ..Default::default()
        },
        &MetadataOptions::default(),
    ) {
        Ok(probed) => probed.format,
        Err(symphonia::core::errors::Error::IoError(error))
            if error.kind() != std::io::ErrorKind::UnexpectedEof =>
        {
            return Err(symphonia::core::errors::Error::IoError(error).into());
        }
        Err(_) if mp4_file_type => {
            return Err(unsupported(
                "audio source is MP4 container data, not RIFF/WAVE or MP3 audio",
            ));
        }
        Err(error) => return Err(error.into()),
    };
    let [track] = format.tracks() else {
        return Err(unsupported("source must contain exactly one audio stream"));
    };
    let params = &track.codec_params;
    let channels = channels(
        params
            .channels
            .ok_or_else(|| unsupported("audio channel layout missing"))?
            .count(),
    )?;
    let sample_rate = params
        .sample_rate
        .ok_or_else(|| unsupported("audio sample rate missing"))?;
    let time_base = params
        .time_base
        .ok_or_else(|| unsupported("audio time base missing"))?;
    let mut end = 0_u64;
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(symphonia::core::errors::Error::IoError(error))
                if error.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(error) => return Err(error.into()),
        };
        end = packet
            .ts()
            .checked_add(packet.dur())
            .ok_or_else(|| unsupported("audio duration overflows"))?;
    }
    let ticks = i128::from(end) * i128::from(time_base.numer) * i128::from(TICKS)
        / i128::from(time_base.denom);
    stream(
        channels,
        sample_rate,
        i64::try_from(ticks).map_err(|_| unsupported("audio duration overflows"))?,
    )
}

/// The picture duration Premiere records for `file`, a movie's sound that ends
/// within one video frame or two 1024-sample AAC frames, whichever is longer,
/// before its picture. `None` when the sound is not shorter in that way.
pub(crate) fn padded_to_picture(
    file: &PrAudioStream,
    picture: Option<PictureClock>,
) -> Result<Option<i64>> {
    let aac_tail = 2 * 1024 * ticks_per_sample(file.sample_rate)?;
    Ok(picture
        .filter(|picture| {
            (1..=picture.frame_ticks.max(aac_tail))
                .contains(&(picture.duration_ticks - file.intrinsic_ticks))
        })
        .map(|picture| picture.duration_ticks))
}

/// The measured constant-rate picture of the movie that embeds a sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PictureClock {
    pub(crate) duration_ticks: i64,
    pub(crate) frame_ticks: i64,
}

impl PictureClock {
    pub(crate) fn of(facts: &crate::media::MediaFacts) -> Option<Self> {
        let crate::media::MediaFacts::Video(video) = facts else {
            return None;
        };
        let (rate, duration_ticks) = video.timing.supported().ok()?;
        Some(Self {
            duration_ticks,
            frame_ticks: rate.ticks_per_frame(),
        })
    }
}

/// How a native `AudioStream` duration matched the file it links.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AudioDurationMatch {
    Exact,
    /// Premiere recorded the movie's picture duration for an embedded sound
    /// whose AAC ends shortly before the picture.
    PaddedToPicture,
}

/// Compares a native `AudioStream` with the file it links. Camera movies often
/// end their AAC shortly before the picture; Premiere then records the picture
/// duration for the embedded sound. That exact picture duration is accepted
/// when the sound ends within one video frame or two 1024-sample AAC frames,
/// whichever is longer, so the identity is still checked against measured bytes.
pub(crate) fn validate_source(
    file: &PrAudioStream,
    native: &PrAudioStream,
    picture: Option<PictureClock>,
) -> Result<AudioDurationMatch> {
    ensure!(
        (file.channels, file.sample_rate) == (native.channels, native.sample_rate),
        "native AudioStream layout or sample rate differs from the file"
    );
    if file.intrinsic_ticks == native.intrinsic_ticks {
        return Ok(AudioDurationMatch::Exact);
    }
    let padded = padded_to_picture(file, picture)? == Some(native.intrinsic_ticks);
    ensure!(
        padded,
        "native AudioStream Duration {} ticks differs from the file duration {} ticks",
        native.intrinsic_ticks,
        file.intrinsic_ticks
    );
    Ok(AudioDurationMatch::PaddedToPicture)
}
