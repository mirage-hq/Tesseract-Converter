//! Audio stream facts and full-source mono channel extraction.
//!
//! Stream inspection uses the shared MP4 parser for AAC presentation edits and
//! demuxes WAV/MP3 without decoding, applying MP3 gapless metadata to packet times.
//! Source-channel packaging copies PCM bytes or decodes MP3/AAC with Symphonia.
//! Missing MP3 gapless tags do not establish native priming/padding alignment.

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

/// Packages one stereo channel without mixing, resampling, or baking clip edits.
/// WAV samples retain their bytes; MP3/AAC decode to float32 on their presentation clock.
pub(crate) fn copy_source_channel(
    path: &std::path::Path,
    output: &std::path::Path,
    channel: usize,
    expected: &PrAudioStream,
) -> Result<()> {
    ensure!(
        channel < 2 && expected.channels == AudioChannels::Stereo,
        "source-channel extraction requires stereo channel 0 or 1"
    );
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .ok_or_else(|| unsupported("source-channel extraction requires a media extension"))?;
    let container = MediaContainer::from_extension(extension)
        .ok_or_else(|| unsupported("unsupported source-channel media extension"))?;
    ensure!(
        matches!(
            container,
            MediaContainer::Wav
                | MediaContainer::Mp3
                | MediaContainer::M4a
                | MediaContainer::Mp4
                | MediaContainer::Mov
        ),
        "source-channel extraction supports PCM WAV, MP3 and MP4-family AAC only"
    );
    // Failed decoding or clock validation must not leave a partial derivative,
    // nor replace an existing file. The archive still verifies the original hash.
    let mut temporary = tempfile::NamedTempFile::new_in(
        output.parent().unwrap_or_else(|| std::path::Path::new(".")),
    )?;
    if container == MediaContainer::Wav {
        copy_pcm_channel(path, temporary.as_file_mut(), channel, expected)?;
    } else {
        decode_channel(path, temporary.as_file_mut(), channel, expected, container)?;
    }
    temporary
        .persist_noclobber(output)
        .map_err(|error| error.error)?;
    Ok(())
}

fn copy_pcm_channel(
    path: &std::path::Path,
    output: &mut std::fs::File,
    channel: usize,
    expected: &PrAudioStream,
) -> Result<()> {
    use std::{
        fs::File,
        io::{BufReader, BufWriter, Write},
    };
    let file = File::open(path)?;
    let size = file.metadata()?.len();
    let mut reader = BufReader::new(file);
    let mut header = [0; 12];
    reader.read_exact(&mut header)?;
    ensure!(
        &header[..4] == b"RIFF" && &header[8..] == b"WAVE",
        "expected RIFF/WAVE source"
    );
    reader.seek(SeekFrom::Start(0))?;
    let mut probe = Probe::default();
    probe.register_all::<symphonia::default::formats::WavReader>();
    let source = MediaSourceStream::new(Box::new(Source { reader, size }), Default::default());
    let mut format = probe
        .format(
            &Hint::new(),
            source,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )?
        .format;
    let [track] = format.tracks() else {
        return Err(unsupported(
            "source-channel extraction requires one WAV stream",
        ));
    };
    let params = &track.codec_params;
    let (sample_bytes, format_tag, bits) = match params.codec {
        symphonia::core::codecs::CODEC_TYPE_PCM_S16LE => (2_usize, 1_u16, 16_u16),
        symphonia::core::codecs::CODEC_TYPE_PCM_F32LE => (4, 3, 32),
        _ => {
            return Err(unsupported(
                "source-channel selection supports PCM16/float32 WAV only",
            ))
        }
    };
    ensure!(
        params
            .channels
            .is_some_and(|channels| channels.count() == 2)
            && params.sample_rate == Some(expected.sample_rate)
            && params
                .time_base
                .is_some_and(|base| base.numer == 1 && base.denom == expected.sample_rate),
        "mono source-channel selection supports uncompressed stereo PCM16/float32 WAV only"
    );
    let track_id = track.id;
    let period = ticks_per_sample(expected.sample_rate)?;
    ensure!(
        expected.intrinsic_ticks > 0 && expected.intrinsic_ticks % period == 0,
        "source-channel extraction requires a whole-sample duration"
    );
    let frames = u64::try_from(expected.intrinsic_ticks / period)
        .map_err(|_| unsupported("source-channel duration exceeds sample range"))?;
    let mut output = BufWriter::new(output);
    write_mono_header(
        &mut output,
        expected.sample_rate,
        frames,
        sample_bytes,
        format_tag,
        bits,
    )?;
    let mut written = 0_u64;
    let mut mono = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(symphonia::core::errors::Error::IoError(error))
                if error.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break
            }
            Err(error) => return Err(error.into()),
        };
        let frame_bytes = sample_bytes * 2;
        ensure!(
            packet.track_id() == track_id
                && packet.ts() == written
                && packet.data.len() % frame_bytes == 0
                && packet.dur() == (packet.data.len() / frame_bytes) as u64,
            "PCM source packets do not form contiguous stereo sample frames"
        );
        mono.clear();
        for frame in packet.data.chunks_exact(frame_bytes) {
            mono.extend_from_slice(&frame[channel * sample_bytes..(channel + 1) * sample_bytes]);
        }
        written = written
            .checked_add(packet.dur())
            .filter(|&count| count <= frames)
            .ok_or_else(|| unsupported("PCM source exceeds inspected duration"))?;
        output.write_all(&mono)?;
    }
    ensure!(
        written == frames,
        "PCM source ends before inspected duration"
    );
    output.flush()?;
    Ok(())
}

fn write_mono_header(
    output: &mut impl std::io::Write,
    sample_rate: u32,
    frames: u64,
    sample_bytes: usize,
    format_tag: u16,
    bits: u16,
) -> Result<()> {
    let bytes = frames
        .checked_mul(sample_bytes as u64)
        .and_then(|bytes| u32::try_from(bytes).ok())
        .ok_or_else(|| unsupported("mono WAV exceeds RIFF size"))?;
    // RIFF's size excludes its first 8 bytes: the canonical header contributes 36.
    let riff_size = bytes
        .checked_add(36)
        .ok_or_else(|| unsupported("mono WAV exceeds RIFF size"))?;
    let byte_rate = sample_rate
        .checked_mul(sample_bytes as u32)
        .ok_or_else(|| unsupported("mono WAV byte rate overflows"))?;
    output.write_all(b"RIFF")?;
    output.write_all(&riff_size.to_le_bytes())?;
    output.write_all(b"WAVEfmt ")?;
    output.write_all(&16_u32.to_le_bytes())?;
    output.write_all(&format_tag.to_le_bytes())?;
    output.write_all(&1_u16.to_le_bytes())?;
    output.write_all(&sample_rate.to_le_bytes())?;
    output.write_all(&byte_rate.to_le_bytes())?;
    output.write_all(&(sample_bytes as u16).to_le_bytes())?;
    output.write_all(&bits.to_le_bytes())?;
    output.write_all(b"data")?;
    output.write_all(&bytes.to_le_bytes())?;
    Ok(())
}

/// Symphonia's MP3 decoder applies packet gapless trims; its MP4 demuxer does
/// not apply edit lists. Map AAC's one unit-rate edit ourselves, in exact samples.
fn decode_channel(
    path: &std::path::Path,
    output: &mut std::fs::File,
    channel: usize,
    expected: &PrAudioStream,
    container: MediaContainer,
) -> Result<()> {
    use std::{
        fs::File,
        io::{BufWriter, Write},
    };
    use symphonia::core::{
        audio::SampleBuffer,
        codecs::{DecoderOptions, CODEC_TYPE_AAC, CODEC_TYPE_MP3},
    };

    let codec = match container {
        MediaContainer::Mp3 => CODEC_TYPE_MP3,
        MediaContainer::M4a | MediaContainer::Mp4 | MediaContainer::Mov => CODEC_TYPE_AAC,
        MediaContainer::Wav | MediaContainer::Image(_) => {
            return Err(unsupported(
                "compressed channel selection requires MP3 or MP4-family AAC",
            ));
        }
    };
    let period = ticks_per_sample(expected.sample_rate)?;
    ensure!(
        expected.intrinsic_ticks > 0 && expected.intrinsic_ticks % period == 0,
        "source-channel extraction requires a whole-sample duration"
    );
    let frames = u64::try_from(expected.intrinsic_ticks / period)
        .map_err(|_| unsupported("source-channel duration exceeds sample range"))?;
    let mut file = File::open(path)?;
    let (start, source_frames) = if container == MediaContainer::Mp3 {
        (0, frames)
    } else {
        let size = file.metadata()?.len();
        let metadata = crate::media_metadata::read_movie_metadata(&mut file, size, false)?;
        file.seek(SeekFrom::Start(0))?;
        let tracks: Vec<_> = metadata
            .tracks
            .iter()
            .filter(|track| track.handler == *b"soun")
            .collect();
        let [track] = tracks.as_slice() else {
            return Err(unsupported(
                "source-channel extraction requires one AAC stream",
            ));
        };
        let window = movie_audio_window(track, metadata.timescale, expected.sample_rate)?;
        let samples = |ticks: i64| -> Result<u64> {
            ensure!(ticks % period == 0, "AAC source clock is not whole samples");
            u64::try_from(ticks / period)
                .map_err(|_| unsupported("AAC source clock exceeds sample range"))
        };
        if track.edit.is_some() {
            ensure!(
                window.presentation_ticks == expected.intrinsic_ticks,
                "selected AAC native duration {frames} samples differs from file presentation {} samples; source-channel extraction cannot pad or shorten the full source",
                window.presentation_ticks / period
            );
        }
        (samples(window.start_ticks)?, samples(window.media_ticks)?)
    };
    let end = start
        .checked_add(frames)
        .filter(|&end| end <= source_frames)
        .ok_or_else(|| unsupported("selected audio presentation exceeds source samples"))?;
    let mut probe = Probe::default();
    if container == MediaContainer::Mp3 {
        probe.register_all::<symphonia::default::formats::MpaReader>();
    } else {
        probe.register_all::<symphonia::default::formats::IsoMp4Reader>();
    }
    let source = MediaSourceStream::new(Box::new(file), Default::default());
    let mut format = probe
        .format(
            &Hint::new(),
            source,
            &FormatOptions {
                enable_gapless: true,
                ..Default::default()
            },
            &MetadataOptions::default(),
        )?
        .format;
    let tracks: Vec<_> = format
        .tracks()
        .iter()
        .filter(|track| track.codec_params.codec == codec)
        .collect();
    let [track] = tracks.as_slice() else {
        return Err(unsupported(
            "selected source must contain one MP3/AAC stream",
        ));
    };
    let params = &track.codec_params;
    // Symphonia's MP4 demuxer leaves AAC channels unset. Admission already read
    // the source layout; every decoded packet must verify it below, never default it.
    ensure!(
        params.sample_rate == Some(expected.sample_rate)
            && params
                .channels
                .map_or(container != MediaContainer::Mp3, |channels| channels
                    .count()
                    == 2)
            && params
                .time_base
                .is_some_and(|base| base.numer == 1 && base.denom == expected.sample_rate)
            && params.start_ts == 0,
        "selected compressed source has an unsupported rate, layout or sample clock"
    );
    let id = track.id;
    let mut decoder = symphonia::default::get_codecs().make(params, &DecoderOptions::default())?;
    let mut output = BufWriter::new(output);
    write_mono_header(&mut output, expected.sample_rate, frames, 4, 3, 32)?;
    let mut buffer: Option<SampleBuffer<f32>> = None;
    let mut source_end = 0_u64;
    let mut written = 0_u64;
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(symphonia::core::errors::Error::IoError(error))
                if error.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break
            }
            Err(error) => return Err(error.into()),
        };
        if packet.track_id() != id {
            continue;
        }
        // Never skip a corrupt packet: that would silently shift all later sound.
        let decoded = decoder.decode(&packet)?;
        let spec = *decoded.spec();
        ensure!(
            spec.rate == expected.sample_rate && spec.channels.count() == 2,
            "selected audio decoder changed sample rate or channel layout"
        );
        let decoded_frames = decoded.frames() as u64;
        if container == MediaContainer::Mp3 && packet.dur() == 0 {
            ensure!(
                decoded_frames == 0,
                "MP3 gapless trim did not remove padding samples"
            );
            continue;
        }
        ensure!(
            packet.ts() == source_end
                && packet.dur() > 0
                && if container == MediaContainer::Mp3 {
                    decoded_frames == packet.dur()
                } else {
                    decoded_frames >= packet.dur()
                },
            "selected audio packets do not form contiguous decoded sample frames"
        );
        source_end = packet
            .ts()
            .checked_add(packet.dur())
            .filter(|&count| count <= source_frames)
            .ok_or_else(|| unsupported("selected audio exceeds inspected source duration"))?;
        // AAC may decode a whole final codec frame although stts declares only
        // part of it. Only that final, source-declared padding may be discarded.
        ensure!(
            decoded_frames == packet.dur() || source_end == source_frames,
            "AAC shortened packet is not the declared source tail"
        );
        let first = start.max(packet.ts());
        let last = end.min(source_end);
        if first >= last {
            continue;
        }
        let needed = decoded.frames().saturating_mul(2);
        let buffer = match &mut buffer {
            Some(buffer) if buffer.capacity() >= needed => buffer,
            slot => slot.insert(SampleBuffer::<f32>::new(decoded_frames, spec)),
        };
        buffer.copy_interleaved_ref(decoded);
        let offset = usize::try_from(first - packet.ts())
            .map_err(|_| unsupported("selected sample offset exceeds host address space"))?;
        let count = usize::try_from(last - first)
            .map_err(|_| unsupported("selected sample count exceeds host address space"))?;
        for frame in buffer.samples().chunks_exact(2).skip(offset).take(count) {
            ensure!(
                frame[channel].is_finite(),
                "selected audio contains nonfinite samples"
            );
            output.write_all(&frame[channel].to_le_bytes())?;
        }
        written += last - first;
    }
    ensure!(
        source_end == source_frames && written == frames,
        "selected audio ends before inspected duration"
    );
    output.flush()?;
    Ok(())
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
        prepared_clock: None,
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
    let window = movie_audio_window(track, metadata.timescale, sample_rate)?;
    stream(channels, sample_rate, window.presentation_ticks).map(Some)
}

/// The common native-tick presentation window. Inspection may retain an edit
/// past the physical tail; channel decoding separately requires whole samples
/// and a complete window inside that tail, without synthesizing padding.
#[derive(Debug)]
struct MovieAudioWindow {
    start_ticks: i64,
    presentation_ticks: i64,
    media_ticks: i64,
}

fn movie_audio_window(
    track: &crate::media_metadata::TrackMetadata,
    movie_timescale: u32,
    sample_rate: u32,
) -> Result<MovieAudioWindow> {
    let media_ticks = exact_ticks(track.duration, track.timescale, "audio duration")?;
    let (start_ticks, presentation_ticks) = match &track.edit {
        None => (0, media_ticks),
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
            let samples = nearest_samples(entry.segment_duration, movie_timescale, sample_rate)?;
            let ticks_per_sample = ticks_per_sample(sample_rate)?;
            let ticks = i64::try_from(samples)
                .ok()
                .and_then(|samples| samples.checked_mul(ticks_per_sample))
                .ok_or_else(|| unsupported("audio edit duration overflows native ticks"))?;
            (start, ticks)
        }
    };
    Ok(MovieAudioWindow {
        start_ticks,
        presentation_ticks,
        media_ticks,
    })
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
        let timing = match facts {
            crate::media::MediaFacts::Video(video) => &video.timing,
            crate::media::MediaFacts::UnsupportedVideo(video) => video.timing.as_ref()?,
            crate::media::MediaFacts::Still(_) | crate::media::MediaFacts::UnsupportedStill(_) => {
                return None;
            }
        };
        let (rate, duration_ticks) = timing.supported().ok()?;
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
    /// The fractional native endpoint and measured whole-sample endpoint
    /// occupy the same last sample; neither source clock is changed.
    RoundedUpToSample,
    /// Premiere recorded the movie's picture duration for an embedded sound
    /// whose AAC ends shortly before the picture.
    PaddedToPicture,
}

/// Compares a native `AudioStream` with the file it links. Camera movies often
/// end their AAC shortly before the picture; Premiere then records the picture
/// duration for the embedded sound. That exact picture duration is accepted
/// when the sound ends within one video frame or two 1024-sample AAC frames,
/// whichever is longer, so the identity is still checked against measured bytes.
/// A fractional native duration can also name the same final sample as the
/// measured whole-sample endpoint: Change Color's picture-length declarations
/// precede their rounded AAC edits by 0.6 and 0.4 samples. This is a ceiling
/// identity, not a tolerance for another sample or a longer native declaration.
pub(crate) fn validate_source(
    file: &PrAudioStream,
    native: &PrAudioStream,
    picture: Option<PictureClock>,
) -> Result<AudioDurationMatch> {
    ensure!(
        (file.channels, file.sample_rate) == (native.channels, native.sample_rate),
        "native AudioStream layout or sample rate differs from the file"
    );
    let sample_ticks = ticks_per_sample(file.sample_rate)?;
    ensure!(
        file.intrinsic_ticks > 0 && native.intrinsic_ticks > 0,
        "audio stream duration must be positive"
    );
    if file.intrinsic_ticks == native.intrinsic_ticks {
        return Ok(AudioDurationMatch::Exact);
    }
    // Positive i64 endpoints make this subtraction representable. Requiring
    // the file endpoint on the sample grid proves the same sample ceiling.
    if file.intrinsic_ticks % sample_ticks == 0
        && (1..sample_ticks).contains(&(file.intrinsic_ticks - native.intrinsic_ticks))
    {
        return Ok(AudioDurationMatch::RoundedUpToSample);
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

/// A proved empty lead followed by one zero-origin unit segment. The prepared
/// asset contains full raw samples; this clock alone owns presentation trimming.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DelayedAudioClock {
    lead_ticks: i64,
    end_ticks: i64,
    raw_ticks: i64,
    native_ticks: i64,
}

impl DelayedAudioClock {
    pub(crate) fn raw_ticks(&self) -> i64 {
        self.raw_ticks
    }

    /// Reverse native Clip bounds use this presentation endpoint, not raw EOF.
    pub(crate) fn presentation_duration_ticks(&self) -> i64 {
        self.native_ticks
    }

    /// Translation from the presentation clock to the prepared raw source.
    pub(crate) fn source_offset_ticks(&self) -> i64 {
        self.lead_ticks
    }

    pub(crate) fn presentation_note(&self) -> String {
        format!(
            "raw source {} ticks; empty lead {} ticks; playable presentation end {} ticks; native declaration {} ticks (native minus edited endpoint {} ticks); all clocks use {TICKS} ticks/second",
            self.raw_ticks, self.lead_ticks, self.end_ticks, self.native_ticks,
            self.native_ticks - self.end_ticks,
        )
    }

    /// Intersect on the original native clock before rounding either boundary.
    pub(crate) fn window(
        &self,
        clip: &crate::schema::PrAudioOccurrence,
    ) -> Result<Option<(std::ops::Range<i64>, std::ops::Range<i64>)>> {
        ensure!(
            clip.in_ticks >= 0
                && clip.out_ticks <= self.native_ticks
                && clip.start_ticks >= 0
                && clip.out_ticks > clip.in_ticks
                && clip.end_ticks > clip.start_ticks
                && clip.playback_rate.is_finite()
                && clip.playback_rate != 0.0
                && (clip.playback_rate != 1.0
                    || clip.out_ticks - clip.in_ticks == clip.end_ticks - clip.start_ticks),
            "delayed sound requires bounded native source and placement intervals"
        );
        let (source_in, source_out) = if clip.playback_rate < 0.0 {
            (
                self.native_ticks - clip.out_ticks,
                self.native_ticks - clip.in_ticks,
            )
        } else {
            (clip.in_ticks, clip.out_ticks)
        };
        let start = source_in.max(self.lead_ticks);
        let end = source_out.min(self.end_ticks);
        if start >= end {
            return Ok(None);
        }
        let native_source = |source| {
            if clip.playback_rate < 0.0 {
                self.native_ticks - source
            } else {
                source
            }
        };
        let first = clip.timeline_at(native_source(start))?;
        let last = clip.timeline_at(native_source(end))?;
        let (timeline_start, timeline_end) = (first.min(last), first.max(last));
        Ok(Some((
            timeline_start..timeline_end,
            start - self.lead_ticks..end - self.lead_ticks,
        )))
    }
}

/// Recognize only the bounded delayed form. Ordinary audio inspection deliberately
/// keeps rejecting it: decoders must not consume the original edit list by accident.
pub(crate) fn inspect_delayed_audio(
    mut reader: impl Read + Seek,
    size: u64,
    native: &PrAudioStream,
) -> Result<Option<DelayedAudioClock>> {
    let metadata = crate::media_metadata::read_movie_metadata(&mut reader, size, false)?;
    let audio: Vec<_> = metadata
        .tracks
        .iter()
        .filter(|track| track.handler == *b"soun")
        .collect();
    let [track] = audio.as_slice() else {
        return Ok(None);
    };
    let Some(entries) = &track.edit else {
        return Ok(None);
    };
    let [lead, play] = entries.as_slice() else {
        return Ok(None);
    };
    ensure!(
        lead.media_time == -1 && play.media_time == 0,
        "delayed audio requires an empty lead and zero-origin playback"
    );
    ensure!(
        entries
            .iter()
            .all(|entry| entry.media_rate == 1 && entry.media_rate_fraction == 0),
        "delayed audio edit-list retiming is unsupported"
    );
    reader.seek(SeekFrom::Start(0))?;
    let inspection = media_transcode::inspect::inspect(reader, size, false)?;
    let streams: Vec<_> = inspection
        .streams
        .iter()
        .filter(|stream| stream.kind == media_transcode::inspect::StreamKind::Audio)
        .collect();
    let [sound] = streams.as_slice() else {
        return Err(unsupported("delayed audio requires one stream"));
    };
    ensure!(
        sound.codec_name == "aac" && sound.codec_tag == *b"mp4a",
        "delayed movie sound must be AAC"
    );
    let layout = channels(
        usize::try_from(sound.channels)
            .map_err(|_| unsupported("audio channel count overflows"))?,
    )?;
    ensure!(
        layout == native.channels && sound.sample_rate == native.sample_rate,
        "delayed audio native sample rate/channel layout differs from source"
    );
    let period = ticks_per_sample(sound.sample_rate)?;
    ensure!(
        track.timescale == sound.sample_rate,
        "delayed AAC requires a raw sample clock"
    );
    let raw_ticks = exact_ticks(track.duration, track.timescale, "raw audio duration")?;
    let lead_ticks = exact_ticks(lead.segment_duration, metadata.timescale, "audio lead")?;
    let play_ticks = exact_ticks(
        play.segment_duration,
        metadata.timescale,
        "audio playback duration",
    )?;
    ensure!(
        lead_ticks > 0 && play_ticks > 0 && play_ticks <= raw_ticks && raw_ticks % period == 0,
        "delayed audio playback must fit the positive whole-sample raw source"
    );
    let end_ticks = lead_ticks
        .checked_add(play_ticks)
        .ok_or_else(|| unsupported("audio presentation end overflows"))?;
    // This is a presentation-descriptor rounding allowance, not AAC padding or
    // arbitrary duration tolerance. Both endpoints must name the same millisecond.
    let ms = crate::schema::TICKS_PER_MILLISECOND;
    ensure!(
        native.intrinsic_ticks > 0
            && (i128::from(native.intrinsic_ticks) - i128::from(end_ticks)).abs()
                <= i128::from(ms / 2)
            && (i128::from(native.intrinsic_ticks) + i128::from(ms / 2)) / i128::from(ms)
                == (i128::from(end_ticks) + i128::from(ms / 2)) / i128::from(ms),
        "delayed audio native declaration differs from the edited presentation endpoint"
    );
    Ok(Some(DelayedAudioClock {
        lead_ticks,
        end_ticks,
        raw_ticks,
        native_ticks: native.intrinsic_ticks,
    }))
}
