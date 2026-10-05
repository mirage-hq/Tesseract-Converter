use ffmpeg::{encoder, ChannelLayout};

fn received(result: std::result::Result<(), ffmpeg::Error>, context: &'static str) -> Result<bool> {
    match result {
        Ok(()) => Ok(true),
        Err(ffmpeg::Error::Eof)
        | Err(ffmpeg::Error::Other {
            errno: ffmpeg::util::error::EAGAIN,
        }) => Ok(false),
        Err(source) => Err(Error::Ffmpeg { context, source }),
    }
}

fn write_packet(
    packet: &mut Packet,
    index: usize,
    from: Rational,
    to: Rational,
    output: &mut format::context::Output,
    cancelled: &AtomicBool,
) -> Result<()> {
    check_cancel(cancelled)?;
    packet.set_stream(index);
    packet.set_position(-1);
    packet.rescale_ts(from, to);
    packet
        .write_interleaved(output)
        .map_err(ffmpeg_error("write output packet"))
}

struct DataOutput {
    index: usize,
    input_time_base: Rational,
    output_time_base: Rational,
}

impl DataOutput {
    fn write(
        &self,
        mut packet: Packet,
        output: &mut format::context::Output,
        cancelled: &AtomicBool,
    ) -> Result<()> {
        write_packet(
            &mut packet,
            self.index,
            self.input_time_base,
            self.output_time_base,
            output,
            cancelled,
        )
    }
}

enum VideoOutput {
    Copy {
        index: usize,
        input_time_base: Rational,
        output_time_base: Rational,
    },
    Encode {
        index: usize,
        output_time_base: Rational,
        decoder: codec::decoder::Video,
        scaler: software::scaling::Context,
        encoder: encoder::video::Encoder,
        decoded: frame::Video,
        converted: frame::Video,
        next_pts: i64,
        progress_rate: Rational,
    },
}

impl VideoOutput {
    fn index(&self) -> usize {
        match self {
            Self::Copy { index, .. } | Self::Encode { index, .. } => *index,
        }
    }

    fn set_output_time_base(&mut self, value: Rational) {
        match self {
            Self::Copy {
                output_time_base, ..
            }
            | Self::Encode {
                output_time_base, ..
            } => *output_time_base = value,
        }
    }

    fn write(
        &mut self,
        mut packet: Packet,
        output: &mut format::context::Output,
        progress: &mut Progress<'_>,
    ) -> Result<()> {
        match self {
            Self::Copy {
                index,
                input_time_base,
                output_time_base,
            } => {
                if let Some(pts) = packet.pts() {
                    progress.report(
                        pts as f64 * f64::from(input_time_base.0) / f64::from(input_time_base.1),
                    )?;
                }
                write_packet(
                    &mut packet,
                    *index,
                    *input_time_base,
                    *output_time_base,
                    output,
                    progress.cancelled,
                )
            }
            Self::Encode { decoder, .. } => {
                decoder
                    .send_packet(&packet)
                    .map_err(ffmpeg_error("send video packet to decoder"))?;
                self.drain(output, progress)
            }
        }
    }

    fn drain(
        &mut self,
        output: &mut format::context::Output,
        progress: &mut Progress<'_>,
    ) -> Result<()> {
        let Self::Encode {
            index,
            output_time_base,
            decoder,
            scaler,
            encoder,
            decoded,
            converted,
            next_pts,
            progress_rate,
        } = self
        else {
            return Ok(());
        };
        loop {
            check_cancel(progress.cancelled)?;
            if !received(decoder.receive_frame(decoded), "receive video frame")? {
                break;
            }
            scaler
                .run(decoded, converted)
                .map_err(ffmpeg_error("convert video pixel format"))?;
            if converted.format() == format::Pixel::YUVA444P10LE {
                copy_eight_bit_alpha(decoded, converted)?;
            }
            converted.set_pts(Some(*next_pts));
            encoder
                .send_frame(converted)
                .map_err(ffmpeg_error("send video frame to encoder"))?;
            *next_pts += 1;
            progress.report(
                *next_pts as f64 * f64::from(progress_rate.1) / f64::from(progress_rate.0),
            )?;
            drain_video_packets(
                encoder,
                *index,
                Rational(progress_rate.1, progress_rate.0),
                *output_time_base,
                output,
                progress.cancelled,
            )?;
        }
        Ok(())
    }

    fn finish(
        &mut self,
        output: &mut format::context::Output,
        progress: &mut Progress<'_>,
    ) -> Result<()> {
        let Self::Encode { decoder, .. } = self else {
            return Ok(());
        };
        decoder
            .send_eof()
            .map_err(ffmpeg_error("flush video decoder"))?;
        self.drain(output, progress)?;
        let Self::Encode {
            index,
            output_time_base,
            encoder,
            progress_rate,
            ..
        } = self
        else {
            return Ok(());
        };
        encoder
            .send_eof()
            .map_err(ffmpeg_error("flush video encoder"))?;
        drain_video_packets(
            encoder,
            *index,
            Rational(progress_rate.1, progress_rate.0),
            *output_time_base,
            output,
            progress.cancelled,
        )
    }
}

/// Bypass swscale's packed-RGB alpha expansion; ProRes's 8-bit plane must
/// contain the original codes, not rounded, expanded 10-bit alpha values.
fn copy_eight_bit_alpha(source: &frame::Video, destination: &mut frame::Video) -> Result<()> {
    let name = source
        .format()
        .descriptor()
        .map(|descriptor| descriptor.name());
    let (plane, step, offset) = match name {
        Some("argb" | "abgr") => (0, 4, 0),
        Some("rgba" | "bgra" | "vuya") => (0, 4, 3),
        Some("gbrap" | "yuva420p" | "yuva422p" | "yuva444p") => (3, 1, 0),
        Some("ya8") => (0, 2, 1),
        _ => {
            return Err(Error::Unsupported(
                "unsupported decoded 8-bit alpha layout".into(),
            ))
        }
    };
    if source.width() != destination.width() || source.height() != destination.height() {
        return Err(Error::Unsupported(
            "alpha source dimensions changed during decode".into(),
        ));
    }
    let width = source.width() as usize;
    let height = source.height() as usize;
    let source_stride = source.stride(plane);
    let destination_stride = destination.stride(3);
    for (input, output) in source
        .data(plane)
        .chunks_exact(source_stride)
        .zip(destination.data_mut(3).chunks_exact_mut(destination_stride))
        .take(height)
    {
        for x in 0..width {
            let value = u16::from(input[x * step + offset]) << 2;
            output[x * 2..x * 2 + 2].copy_from_slice(&value.to_le_bytes());
        }
    }
    Ok(())
}

fn drain_video_packets(
    encoder: &mut encoder::video::Encoder,
    index: usize,
    time_base: Rational,
    output_time_base: Rational,
    output: &mut format::context::Output,
    cancelled: &AtomicBool,
) -> Result<()> {
    let mut packet = Packet::empty();
    loop {
        check_cancel(cancelled)?;
        if !received(
            encoder.receive_packet(&mut packet),
            "receive encoded video packet",
        )? {
            break;
        }
        // Admitted sources are CFR and this encoder clock advances one tick per frame.
        packet.set_duration(1);
        write_packet(
            &mut packet,
            index,
            time_base,
            output_time_base,
            output,
            cancelled,
        )?;
    }
    Ok(())
}

enum AudioMux {
    Copy {
        index: usize,
        input_time_base: Rational,
        output_time_base: Rational,
    },
    Encode(Box<AudioOutput>),
}

impl AudioMux {
    fn index(&self) -> usize {
        match self {
            Self::Copy { index, .. } => *index,
            Self::Encode(audio) => audio.index,
        }
    }
    fn set_output_time_base(&mut self, value: Rational) {
        match self {
            Self::Copy {
                output_time_base, ..
            } => *output_time_base = value,
            Self::Encode(audio) => audio.output_time_base = value,
        }
    }
    fn write(
        &mut self,
        mut packet: Packet,
        output: &mut format::context::Output,
        cancelled: &AtomicBool,
    ) -> Result<()> {
        check_cancel(cancelled)?;
        match self {
            Self::Copy {
                index,
                input_time_base,
                output_time_base,
            } => write_packet(
                &mut packet,
                *index,
                *input_time_base,
                *output_time_base,
                output,
                cancelled,
            ),
            Self::Encode(audio) => audio.write(packet, output, cancelled),
        }
    }
    fn finish(
        &mut self,
        output: &mut format::context::Output,
        cancelled: &AtomicBool,
    ) -> Result<()> {
        check_cancel(cancelled)?;
        match self {
            Self::Copy { .. } => Ok(()),
            Self::Encode(audio) => audio.finish(output, cancelled),
        }
    }
}

struct AudioOutput {
    index: usize,
    output_time_base: Rational,
    input_time_base: Rational,
    decoder: codec::decoder::Audio,
    resampler: software::resampling::Context,
    encoder: encoder::audio::Encoder,
    decoded: frame::Audio,
    pending: Option<Vec<Vec<f32>>>,
    frame_size: usize,
    output_format: ffmpeg::format::Sample,
    sample_rate: u32,
    layout: ChannelLayout,
    next_pts: i64,
    decoded_samples: i64,
    /// MP3 decoder priming accepted only by the AE audio-only source route.
    input_start_samples: i64,
    strict_timing: bool,
    /// Declared stream length in samples. FFmpeg 7 decoders keep trailing AAC
    /// padding past the container duration; output stops at this length.
    sample_limit: Option<i64>,
}

impl AudioOutput {
    fn write(
        &mut self,
        packet: Packet,
        output: &mut format::context::Output,
        cancelled: &AtomicBool,
    ) -> Result<()> {
        check_cancel(cancelled)?;
        self.decoder
            .send_packet(&packet)
            .map_err(ffmpeg_error("send audio packet to decoder"))?;
        self.drain(output, cancelled)
    }

    fn drain(
        &mut self,
        output: &mut format::context::Output,
        cancelled: &AtomicBool,
    ) -> Result<()> {
        loop {
            check_cancel(cancelled)?;
            if !received(
                self.decoder.receive_frame(&mut self.decoded),
                "receive audio frame",
            )? {
                break;
            }
            let start = self.decoded_samples;
            self.check_timing()?;
            if let Some(limit) = self.sample_limit {
                let keep = usize::try_from((limit - start).max(0)).unwrap_or(usize::MAX);
                if keep == 0 {
                    continue;
                }
                if keep < self.decoded.samples() {
                    self.decoded.set_samples(keep);
                }
            }
            let mut converted = frame::Audio::empty();
            self.resampler
                .run(&self.decoded, &mut converted)
                .map_err(ffmpeg_error("resample audio"))?;
            if let Some(pending) = &mut self.pending {
                for (channel, samples) in pending.iter_mut().enumerate() {
                    samples.extend_from_slice(converted.plane::<f32>(channel));
                }
                while self
                    .pending
                    .as_ref()
                    .is_some_and(|samples| samples[0].len() >= self.frame_size)
                {
                    check_cancel(cancelled)?;
                    self.send_frame(self.frame_size)?;
                    self.drain_packets(output, cancelled)?;
                }
            } else {
                converted.set_pts(Some(self.next_pts));
                self.next_pts += i64::try_from(converted.samples())
                    .map_err(|_| Error::Unsupported("audio frame is too large".to_owned()))?;
                self.encoder
                    .send_frame(&converted)
                    .map_err(ffmpeg_error("send PCM audio frame to encoder"))?;
                self.drain_packets(output, cancelled)?;
            }
        }
        Ok(())
    }

    fn check_timing(&mut self) -> Result<()> {
        let rate = i64::from(self.decoder.rate().max(1));
        if let Some(timestamp) = self.decoded.timestamp().or(self.decoded.pts()) {
            let Rational(num, den) = self.input_time_base;
            let position =
                i128::from(timestamp) * i128::from(num) * i128::from(rate) / i128::from(den.max(1));
            let position = i64::try_from(position).map_err(|_| {
                Error::Unsupported("audio timestamp exceeds supported range".to_owned())
            })?;
            let tolerance = i128::from(if self.strict_timing {
                0
            } else {
                (rate / 1000).max(1)
            });
            // Extreme hostile clocks must not overflow the validation itself.
            let offset = i128::from(position)
                - i128::from(self.input_start_samples)
                - i128::from(self.decoded_samples);
            if offset.abs() > tolerance {
                return Err(Error::Unsupported(format!(
                    "audio is offset or discontinuous by {offset} samples",
                )));
            }
        }
        if self.strict_timing && self.decoded.timestamp().or(self.decoded.pts()).is_none() {
            return Err(Error::Unsupported("raw audio frame has no timestamp".into()));
        }
        let count = i64::try_from(self.decoded.samples())
            .map_err(|_| Error::Unsupported("audio frame is too large".to_owned()))?;
        self.decoded_samples = self
            .decoded_samples
            .checked_add(count)
            .ok_or_else(|| Error::Unsupported("audio sample count overflow".to_owned()))?;
        Ok(())
    }

    fn send_frame(&mut self, samples: usize) -> Result<()> {
        let pending = self
            .pending
            .as_mut()
            .ok_or_else(|| Error::Unsupported("internal PCM buffering error".to_owned()))?;
        let mut output = frame::Audio::new(self.output_format, samples, self.layout);
        output.set_rate(self.sample_rate);
        output.set_pts(Some(self.next_pts));
        for (channel, buffered) in pending.iter_mut().enumerate() {
            output
                .plane_mut::<f32>(channel)
                .copy_from_slice(&buffered[..samples]);
            buffered.drain(..samples);
        }
        self.next_pts += i64::try_from(samples).map_err(|_| {
            Error::Unsupported("audio sample count exceeds supported range".to_owned())
        })?;
        self.encoder
            .send_frame(&output)
            .map_err(ffmpeg_error("send audio frame to encoder"))
    }

    fn drain_packets(
        &mut self,
        output: &mut format::context::Output,
        cancelled: &AtomicBool,
    ) -> Result<()> {
        let mut packet = Packet::empty();
        loop {
            check_cancel(cancelled)?;
            if !received(
                self.encoder.receive_packet(&mut packet),
                "receive encoded audio packet",
            )? {
                break;
            }
            write_packet(
                &mut packet,
                self.index,
                Rational(1, self.sample_rate as i32),
                self.output_time_base,
                output,
                cancelled,
            )?;
        }
        Ok(())
    }

    fn finish(
        &mut self,
        output: &mut format::context::Output,
        cancelled: &AtomicBool,
    ) -> Result<()> {
        check_cancel(cancelled)?;
        self.decoder
            .send_eof()
            .map_err(ffmpeg_error("flush audio decoder"))?;
        self.drain(output, cancelled)?;
        if let Some(samples) = self
            .pending
            .as_ref()
            .and_then(|pending| (!pending[0].is_empty()).then_some(pending[0].len()))
        {
            check_cancel(cancelled)?;
            self.send_frame(samples)?;
        }
        check_cancel(cancelled)?;
        self.encoder
            .send_eof()
            .map_err(ffmpeg_error("flush audio encoder"))?;
        self.drain_packets(output, cancelled)
    }
}

struct Progress<'a> {
    last: f64,
    callback: &'a mut dyn FnMut(f64),
    cancelled: &'a AtomicBool,
}
impl Progress<'_> {
    fn report(&mut self, seconds: f64) -> Result<()> {
        check_cancel(self.cancelled)?;
        if seconds + f64::EPSILON >= self.last + PROGRESS_INTERVAL_SECONDS {
            let seconds = seconds.max(0.0);
            (self.callback)(seconds);
            self.last = seconds;
        }
        Ok(())
    }
}

fn transcode_video(
    job: &Job,
    mode: VideoMode,
    callback: &mut dyn FnMut(f64),
    cancelled: &AtomicBool,
) -> Result<()> {
    check_cancel(cancelled)?;
    let mut input = open_local_input(&job.input)?;
    check_cancel(cancelled)?;
    let video_index = input
        .streams()
        .find(|stream| stream.parameters().medium() == media::Type::Video)
        .map(|stream| stream.index())
        .ok_or_else(|| Error::Unsupported("input has no video stream".to_owned()))?;
    let audio_index = input
        .streams()
        .find(|stream| stream.parameters().medium() == media::Type::Audio)
        .map(|stream| stream.index());
    let data = probe_data_streams(&input, input.format().name())?;
    if data.timecode != job.source.timecode
        || data.timecode_stream_index != job.source.timecode_stream_index
        || data.camera_metadata != job.source.camera_metadata
    {
        return Err(Error::Unsupported(
            "input data streams changed after probing".into(),
        ));
    }
    let data_index = data.timecode_stream_index;
    let muxer = if job.output.extension().and_then(|value| value.to_str()) == Some("mov") {
        "mov"
    } else {
        "mp4"
    };
    let mut output =
        format::output_as(&job.output, muxer).map_err(ffmpeg_error("create output container"))?;
    let video_input = input
        .stream(video_index)
        .ok_or_else(|| Error::Unsupported("video stream disappeared".to_owned()))?;
    let mut video = make_video_output(
        mode,
        &video_input,
        &mut output,
        job.destination == crate::model::Destination::AfterEffects,
    )?;
    let exact_length = header_stream_lengths(&input);
    let mut audio = if let Some(index) = audio_index {
        let stream = input
            .stream(index)
            .ok_or_else(|| Error::Unsupported("audio stream disappeared".to_owned()))?;
        Some(make_embedded_audio_output(&stream, &mut output, mode, exact_length)?)
    } else {
        None
    };
    let mut data = data_index
        .map(|index| {
            let stream = input
                .stream(index)
                .ok_or_else(|| Error::Unsupported("data stream disappeared".to_owned()))?;
            make_data_output(&stream, &mut output)
        })
        .transpose()?;
    let mut options = Dictionary::new();
    options.set("movflags", "+faststart");
    if matches!(mode, VideoMode::Copy) {
        let Rational(num, den) = video_input.time_base();
        let timescale = crate::backend::remux_video_timescale(&Ratio { num, den })
            .ok_or_else(|| Error::Unsupported("invalid remux video time base".into()))?;
        options.set("video_track_timescale", &timescale.to_string());
    }
    if data_index.is_none() && !job.source.camera_metadata.is_empty() {
        options.set("write_tmcd", "0");
    }
    options.set("avoid_negative_ts", "disabled");
    if job.destination == crate::model::Destination::AfterEffects {
        options.set("use_editlist", "0");
        if let Some(video) = &job.source.video {
            options.set("movie_timescale", &video.frame_rate.num.to_string());
        }
    } else {
        options.set("use_editlist", "1");
        let clock = output
            .stream(video.index())
            .ok_or_else(|| Error::Unsupported("output video stream disappeared".into()))?
            .time_base();
        let timescale = crate::backend::movie_timescale(
            &Ratio { num: clock.0, den: clock.1 },
            job.source.audio.as_ref().map(|audio| audio.sample_rate),
        )
        .ok_or_else(|| Error::Unsupported("movie clock exceeds supported timescale".into()))?;
        options.set("movie_timescale", &timescale.to_string());
    }
    check_cancel(cancelled)?;
    output
        .write_header_with(options)
        .map_err(ffmpeg_error("write output header"))?;
    let video_time_base = output
        .stream(video.index())
        .ok_or_else(|| Error::Unsupported("output video stream disappeared".to_owned()))?
        .time_base();
    video.set_output_time_base(video_time_base);
    if let Some(audio) = &mut audio {
        let index = audio.index();
        let time_base = output
            .stream(index)
            .ok_or_else(|| Error::Unsupported("output audio stream disappeared".to_owned()))?
            .time_base();
        audio.set_output_time_base(time_base);
    }
    if let Some(data) = &mut data {
        data.output_time_base = output
            .stream(data.index)
            .ok_or_else(|| Error::Unsupported("output data stream disappeared".to_owned()))?
            .time_base();
    }
    let mut progress = Progress {
        last: -PROGRESS_INTERVAL_SECONDS,
        callback,
        cancelled,
    };
    while let Some((stream, packet)) = read_packet(&mut input, cancelled)? {
        if stream == video_index {
            video.write(packet, &mut output, &mut progress)?;
        } else if Some(stream) == audio_index {
            if let Some(audio) = &mut audio {
                audio.write(packet, &mut output, cancelled)?;
            }
        } else if Some(stream) == data_index {
            if let Some(data) = &data {
                data.write(packet, &mut output, cancelled)?;
            }
        }
    }
    video.finish(&mut output, &mut progress)?;
    if let Some(audio) = &mut audio {
        audio.finish(&mut output, cancelled)?;
    }
    check_cancel(cancelled)?;
    output
        .write_trailer()
        .map_err(ffmpeg_error("write output trailer"))
}

fn make_data_output(
    input: &format::stream::Stream<'_>,
    output: &mut format::context::Output,
) -> Result<DataOutput> {
    let mut stream = output
        .add_stream(encoder::find(codec::Id::None))
        .map_err(ffmpeg_error("add copied timecode stream"))?;
    stream.set_parameters(input.parameters());
    stream.set_time_base(input.time_base());
    // MOV serializes the tmcd sample description using the track's frame rate.
    stream.set_avg_frame_rate(input.avg_frame_rate());
    stream.set_metadata(input.metadata().to_owned());
    Ok(DataOutput {
        index: stream.index(),
        input_time_base: input.time_base(),
        output_time_base: Rational(0, 1),
    })
}

fn video_encoder_options(mode: VideoMode) -> Dictionary<'static> {
    let mut options = Dictionary::new();
    match mode {
        VideoMode::H264 => {
            // Media Foundation uses the generic numeric profile option, without the
            // "high" alias supported by VideoToolbox's private option table.
            options.set("profile", &ffi::FF_PROFILE_H264_HIGH.to_string());
            // Prefer hardware, but let VideoToolbox use its own software encoder on
            // hosts without an available hardware encoder (including Intel CI VMs).
            #[cfg(target_os = "macos")]
            options.set("allow_sw", "1");
        }
        VideoMode::Prores => {
            options.set("profile", "4");
            // Preserve 8-bit alpha codes rather than introducing 10/16-bit rescaling drift.
            options.set("alpha_bits", "8");
        }
        VideoMode::Copy => {}
    }
    options
}

/// Encoder parameters do not carry the demuxer's display matrix automatically.
/// Copy the complete transform, including translation, without rotating pixels.
fn copy_display_matrix(
    input: &format::stream::Stream<'_>,
    output: &mut format::stream::StreamMut<'_>,
) -> Result<()> {
    for side_data in input.side_data() {
        if side_data.kind() != ffmpeg::codec::packet::side_data::Type::DisplayMatrix {
            continue;
        }
        let data = side_data.data();
        let mut parameters = output.parameters();
        // SAFETY: parameters belong to the exclusively borrowed output stream.
        // FFmpeg owns the side-data allocation; the successful allocation has
        // exactly data.len() writable bytes, disjoint from the input stream.
        unsafe {
            let raw = &mut *parameters.as_mut_ptr();
            ffi::av_packet_side_data_remove(
                raw.coded_side_data,
                &mut raw.nb_coded_side_data,
                ffi::AVPacketSideDataType::AV_PKT_DATA_DISPLAYMATRIX,
            );
            let destination = ffi::av_packet_side_data_new(
                &mut raw.coded_side_data,
                &mut raw.nb_coded_side_data,
                ffi::AVPacketSideDataType::AV_PKT_DATA_DISPLAYMATRIX,
                data.len(),
                0,
            );
            if destination.is_null() {
                return Err(Error::Ffmpeg {
                    context: "allocate output display matrix",
                    source: ffmpeg::Error::from(-libc::ENOMEM),
                });
            }
            std::ptr::copy_nonoverlapping(data.as_ptr(), (*destination).data, data.len());
        }
    }
    Ok(())
}

fn make_video_output(
    mode: VideoMode,
    input: &format::stream::Stream<'_>,
    output: &mut format::context::Output,
    preserve_sample_entry: bool,
) -> Result<VideoOutput> {
    if matches!(mode, VideoMode::Copy) {
        let mut stream = output
            .add_stream(encoder::find(codec::Id::None))
            .map_err(ffmpeg_error("add copied video stream"))?;
        stream.set_parameters(input.parameters());
        // The bounded AE MOV destination retains the source sample entry:
        // clearing ProRes 4444's ap4h selects apch despite unchanged packets.
        // General container conversion still lets its muxer choose a tag.
        if !preserve_sample_entry {
            unsafe {
                (*stream.parameters().as_mut_ptr()).codec_tag = 0;
            }
        }
        if input.parameters().id() == codec::Id::HEVC {
            unsafe {
                (*stream.parameters().as_mut_ptr()).codec_tag = u32::from_le_bytes(*b"hvc1");
            }
        }
        stream.set_time_base(input.time_base());
        copy_display_matrix(input, &mut stream)?;
        return Ok(VideoOutput::Copy {
            index: stream.index(),
            input_time_base: input.time_base(),
            output_time_base: Rational(0, 1),
        });
    }
    let decoder = codec::Context::from_parameters(input.parameters())
        .and_then(|context| context.decoder().video())
        .map_err(ffmpeg_error("open video decoder"))?;
    let rate = input.avg_frame_rate().reduce();
    if rate.0 <= 0 || rate.1 <= 0 {
        return Err(Error::Unsupported(
            "video has no positive frame rate".to_owned(),
        ));
    }
    let (name, pixel) = match mode {
        VideoMode::H264 => (
            H264_ENCODER.ok_or_else(|| {
                Error::Unsupported("this platform has no supported native H.264 encoder".to_owned())
            })?,
            ffmpeg::format::Pixel::YUV420P,
        ),
        VideoMode::Prores => ("prores_ks", ffmpeg::format::Pixel::YUVA444P10LE),
        VideoMode::Copy => unreachable!("copy returned above"),
    };
    let found = encoder::find_by_name(name).ok_or_else(|| {
        Error::Unsupported(format!(
            "required encoder {name} is absent from this FFmpeg build"
        ))
    })?;
    let global_header = output
        .format()
        .flags()
        .contains(format::Flags::GLOBAL_HEADER);
    let mut stream = output
        .add_stream(found)
        .map_err(ffmpeg_error("add encoded video stream"))?;
    let mut context = codec::Context::new_with_codec(found)
        .encoder()
        .video()
        .map_err(ffmpeg_error("create video encoder"))?;
    context.set_width(decoder.width());
    context.set_height(decoder.height());
    context.set_format(pixel);
    context.set_time_base(Rational(rate.1, rate.0));
    context.set_frame_rate(Some(rate));
    context.set_max_b_frames(0);
    let gop = ((f64::from(rate.0) / f64::from(rate.1)).ceil() as u32).max(1);
    context.set_gop(gop);
    context.set_colorspace(decoder.color_space());
    context.set_color_range(decoder.color_range());
    unsafe {
        (*context.as_mut_ptr()).color_primaries = decoder.color_primaries().into();
        (*context.as_mut_ptr()).color_trc = decoder.color_transfer_characteristic().into();
    }
    if matches!(mode, VideoMode::H264) {
        let bitrate = (f64::from(decoder.width()) * f64::from(decoder.height()) * f64::from(rate.0)
            / f64::from(rate.1)
            * H264_BITS_PER_PIXEL_FRAME) as usize;
        context.set_bit_rate(bitrate);
    }
    if global_header {
        context.set_flags(codec::Flags::GLOBAL_HEADER);
    }
    let encoder = context
        .open_with(video_encoder_options(mode))
        .map_err(ffmpeg_error("open video encoder"))?;
    stream.set_parameters(&encoder);
    copy_display_matrix(input, &mut stream)?;
    stream.set_time_base(Rational(rate.1, rate.0));
    let mut scaler = software::scaling::Context::get(
        decoder.format(),
        decoder.width(),
        decoder.height(),
        pixel,
        decoder.width(),
        decoder.height(),
        software::scaling::Flags::BICUBIC,
    )
    .map_err(ffmpeg_error("create video pixel converter"))?;
    configure_scaler_color(
        &mut scaler,
        decoder.format(),
        decoder.color_space().into(),
        decoder.color_range().into(),
    )?;
    Ok(VideoOutput::Encode {
        index: stream.index(),
        output_time_base: Rational(0, 1),
        decoder,
        scaler,
        encoder,
        decoded: frame::Video::empty(),
        converted: frame::Video::empty(),
        next_pts: 0,
        progress_rate: rate,
    })
}

// Encoder tags alone do not configure swscale's range or matrix coefficients.
fn configure_scaler_color(
    scaler: &mut software::scaling::Context,
    pixel: format::Pixel,
    space: ffi::AVColorSpace,
    range: ffi::AVColorRange,
) -> Result<()> {
    if space != ffi::AVColorSpace::AVCOL_SPC_UNSPECIFIED
        || range != ffi::AVColorRange::AVCOL_RANGE_UNSPECIFIED
    {
        unsafe {
            let descriptor = ffi::av_pix_fmt_desc_get(pixel.into());
            let rgb =
                !descriptor.is_null() && (*descriptor).flags & ffi::AV_PIX_FMT_FLAG_RGB as u64 != 0;
            let full = range == ffi::AVColorRange::AVCOL_RANGE_JPEG;
            let coefficients = ffi::sws_getCoefficients(space as i32);
            if ffi::sws_setColorspaceDetails(
                scaler.as_mut_ptr(),
                coefficients,
                i32::from(rgb || full),
                coefficients,
                i32::from(full),
                0,
                1 << 16,
                1 << 16,
            ) < 0
            {
                return Err(Error::Unsupported(
                    "cannot preserve source color conversion parameters".into(),
                ));
            }
        }
    }
    Ok(())
}

fn make_embedded_audio_output(
    input: &format::stream::Stream<'_>,
    output: &mut format::context::Output,
    mode: VideoMode,
    exact_length: bool,
) -> Result<AudioMux> {
    if matches!(mode, VideoMode::Copy) || input.parameters().id() == codec::Id::AAC {
        let mut stream = output
            .add_stream(encoder::find(codec::Id::None))
            .map_err(ffmpeg_error("add copied audio stream"))?;
        stream.set_parameters(input.parameters());
        // Preserve codec packets, not the source container's codec tag.
        unsafe {
            (*stream.parameters().as_mut_ptr()).codec_tag = 0;
        }
        stream.set_time_base(input.time_base());
        return Ok(AudioMux::Copy {
            index: stream.index(),
            input_time_base: input.time_base(),
            output_time_base: Rational(0, 1),
        });
    }
    make_audio_output(input, output, matches!(mode, VideoMode::Prores), exact_length)
        .map(|audio| AudioMux::Encode(Box::new(audio)))
}

/// `exact_length` states that the container header records the stream length.
/// Only then is trailing decoded audio past that length codec padding.
fn make_audio_output(
    input: &format::stream::Stream<'_>,
    output: &mut format::context::Output,
    pcm: bool,
    exact_length: bool,
) -> Result<AudioOutput> {
    let mut decoder = open_audio_decoder(input)?;
    if decoder.channel_layout().is_empty() {
        decoder.set_channel_layout(ChannelLayout::default(i32::from(decoder.channels())));
    }
    let layout = decoder.channel_layout();
    let sample_rate = decoder.rate();
    let (codec_id, sample_format) = if pcm {
        use ffmpeg::format::{sample::Type::Packed, Sample};
        match decoder.format() {
            Sample::U8(_) | Sample::I16(_) => (codec::Id::PCM_S16LE, Sample::I16(Packed)),
            Sample::I32(_) => (codec::Id::PCM_S32LE, Sample::I32(Packed)),
            Sample::F32(_) => (codec::Id::PCM_F32LE, Sample::F32(Packed)),
            Sample::F64(_) => (codec::Id::PCM_F64LE, Sample::F64(Packed)),
            Sample::I64(_) | Sample::None => {
                return Err(Error::Unsupported(format!(
                    "PCM cannot preserve source sample format {}",
                    decoder.format().name()
                )))
            }
        }
    } else {
        (
            codec::Id::AAC,
            ffmpeg::format::Sample::F32(ffmpeg::format::sample::Type::Planar),
        )
    };
    let codec = encoder::find(codec_id).ok_or_else(|| {
        Error::Unsupported(
            if pcm {
                "PCM encoder is absent"
            } else {
                "AAC encoder is absent"
            }
            .to_owned(),
        )
    })?;
    let global_header = output
        .format()
        .flags()
        .contains(format::Flags::GLOBAL_HEADER);
    let mut stream = output
        .add_stream(codec)
        .map_err(ffmpeg_error("add audio stream"))?;
    let mut context = codec::Context::new_with_codec(codec)
        .encoder()
        .audio()
        .map_err(ffmpeg_error("create audio encoder"))?;
    context.set_rate(sample_rate as i32);
    context.set_channel_layout(layout);
    context.set_format(sample_format);
    context.set_time_base(Rational(1, sample_rate as i32));
    if !pcm {
        context.set_bit_rate(AAC_BITS_PER_CHANNEL * usize::from(decoder.channels().max(1)));
    }
    if global_header {
        context.set_flags(codec::Flags::GLOBAL_HEADER);
    }
    let encoder = context.open().map_err(ffmpeg_error("open audio encoder"))?;
    stream.set_parameters(&encoder);
    stream.set_time_base(Rational(1, sample_rate as i32));
    let resampler = software::resampling::Context::get(
        decoder.format(),
        decoder.channel_layout(),
        decoder.rate(),
        sample_format,
        layout,
        sample_rate,
    )
    .map_err(ffmpeg_error("create audio resampler"))?;
    let frame_size = usize::try_from(encoder.frame_size()).unwrap_or(0).max(1024);
    let pending = (!pcm).then(|| vec![Vec::new(); layout.channels() as usize]);
    Ok(AudioOutput {
        index: stream.index(),
        output_time_base: Rational(0, 1),
        input_time_base: input.time_base(),
        decoder,
        resampler,
        encoder,
        decoded: frame::Audio::empty(),
        pending,
        frame_size,
        output_format: sample_format,
        sample_rate,
        layout,
        next_pts: 0,
        decoded_samples: 0,
        input_start_samples: 0,
        strict_timing: false,
        sample_limit: exact_length
            .then(|| declared_samples(input, sample_rate))
            .flatten(),
    })
}

/// Whether stream lengths come from container headers, not a bitrate estimate.
fn header_stream_lengths(input: &format::context::Input) -> bool {
    // SAFETY: `input` owns a valid opened format context for this read.
    unsafe {
        (*input.as_ptr()).duration_estimation_method
            == ffi::AVDurationEstimationMethod::AVFMT_DURATION_FROM_STREAM
    }
}

fn declared_samples(stream: &format::stream::Stream<'_>, sample_rate: u32) -> Option<i64> {
    let duration = stream.duration();
    let Rational(num, den) = stream.time_base();
    if duration <= 0 || num <= 0 || den <= 0 {
        return None;
    }
    let samples = i128::from(duration) * i128::from(num) * i128::from(sample_rate) / i128::from(den);
    i64::try_from(samples).ok().filter(|samples| *samples > 0)
}

fn transcode_audio_only(
    job: &Job,
    callback: &mut dyn FnMut(f64),
    cancelled: &AtomicBool,
) -> Result<()> {
    check_cancel(cancelled)?;
    let mut input = open_local_input(&job.input)?;
    check_cancel(cancelled)?;
    let index = input
        .streams()
        .find(|stream| stream.parameters().medium() == media::Type::Audio)
        .map(|stream| stream.index())
        .ok_or_else(|| Error::Unsupported("input has no audio stream".to_owned()))?;
    let mut output =
        format::output_as(&job.output, "wav").map_err(ffmpeg_error("create WAV output"))?;
    let stream = input
        .stream(index)
        .ok_or_else(|| Error::Unsupported("audio stream disappeared".to_owned()))?;
    let mut audio = make_audio_output(&stream, &mut output, true, header_stream_lengths(&input))?;
    audio.input_start_samples = job
        .source
        .audio
        .as_ref()
        .and_then(crate::model::ae_mp3_priming_samples)
        .unwrap_or(0);
    check_cancel(cancelled)?;
    output
        .write_header()
        .map_err(ffmpeg_error("write WAV header"))?;
    audio.output_time_base = output
        .stream(audio.index)
        .ok_or_else(|| Error::Unsupported("output audio stream disappeared".to_owned()))?
        .time_base();
    let mut progress = Progress {
        last: -PROGRESS_INTERVAL_SECONDS,
        callback,
        cancelled,
    };
    while let Some((stream, packet)) = read_packet(&mut input, cancelled)? {
        if stream == index {
            if let Some(pts) = packet.pts() {
                progress.report(
                    pts as f64 * f64::from(audio.input_time_base.0)
                        / f64::from(audio.input_time_base.1),
                )?;
            }
            audio.write(packet, &mut output, cancelled)?;
        }
    }
    audio.finish(&mut output, cancelled)?;
    check_cancel(cancelled)?;
    output
        .write_trailer()
        .map_err(ffmpeg_error("write WAV trailer"))
}

pub(super) fn prepare_raw_movie_audio(
    path: &Path,
    destination: &Path,
    expected: crate::RawMovieAudio,
) -> std::result::Result<(), crate::TranscodeError> {
    public_result((|| {
        init()?;
        validate_input(path)?;
        let mut options = Dictionary::new();
        options.set("protocol_whitelist", "file,pipe");
        options.set("format_whitelist", "mov");
        options.set("ignore_editlist", "1");
        let mut input = format::input_with_dictionary(path, options)
            .map_err(ffmpeg_error("open raw movie audio"))?;
        let mut streams = input
            .streams()
            .filter(|stream| stream.parameters().medium() == media::Type::Audio);
        let stream = streams
            .next()
            .ok_or_else(|| Error::Unsupported("movie audio is missing".into()))?;
        if streams.next().is_some() || stream.parameters().id() != codec::Id::AAC {
            return Err(Error::Unsupported(
                "raw movie preparation requires one AAC stream".into(),
            ));
        }
        let index = stream.index();
        let mut output = format::output_as(destination, "wav")
            .map_err(ffmpeg_error("create raw PCM output"))?;
        let mut audio = make_audio_output(&stream, &mut output, true, false)?;
        if audio.sample_rate != expected.sample_rate
            || audio.decoder.channels() != expected.channels
            || (audio.layout != ChannelLayout::MONO && audio.layout != ChannelLayout::STEREO)
            || audio.input_time_base != Rational(1, expected.sample_rate as i32)
            || declared_samples(&stream, expected.sample_rate) != Some(expected.samples as i64)
        {
            return Err(Error::Unsupported(
                "raw audio layout/rate/length differs from inspected source".into(),
            ));
        }
        audio.sample_limit = Some(expected.samples as i64);
        audio.strict_timing = true;
        let cancelled = AtomicBool::new(false);
        output
            .write_header()
            .map_err(ffmpeg_error("write raw PCM header"))?;
        audio.output_time_base = output
            .stream(audio.index)
            .ok_or_else(|| Error::Unsupported("raw PCM stream disappeared".into()))?
            .time_base();
        for (stream, packet) in input.packets() {
            if stream.index() == index {
                audio.write(packet, &mut output, &cancelled)?;
            }
        }
        audio.finish(&mut output, &cancelled)?;
        // AAC's last packet may contain padding beyond mdhd. Only that bounded
        // decoder tail is removed, never any presentation gap/trim.
        if audio.next_pts != expected.samples as i64
            || audio.decoded_samples < expected.samples as i64
            || audio.decoded_samples - expected.samples as i64 >= 1024
        {
            return Err(Error::Unsupported(
                "decoded raw AAC length differs from inspected source".into(),
            ));
        }
        output
            .write_trailer()
            .map_err(ffmpeg_error("write raw PCM trailer"))
    })())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remux_preserves_native_coarse_clock_on_fresh_probe() {
        init().unwrap();
        let clock = crate::tests::native_coarse_clock();
        let time_base = Rational(clock.native_time_base.num, clock.native_time_base.den);
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("coarse.mov");
        let target = directory.path().join("prepared.mp4");
        let codec = encoder::find_by_name("libx264").unwrap();
        let mut output = format::output_as(&source, "mov").unwrap();
        let mut stream = output.add_stream(codec).unwrap();
        let mut context = codec::Context::new_with_codec(codec).encoder().video().unwrap();
        context.set_width(64);
        context.set_height(48);
        context.set_format(format::Pixel::YUV420P);
        context.set_time_base(time_base);
        context.set_frame_rate(Some(Rational(30, 1)));
        context.set_max_b_frames(0);
        context.set_gop(30);
        context.set_flags(codec::Flags::GLOBAL_HEADER);
        let mut encoder = context.open().unwrap();
        stream.set_parameters(&encoder);
        stream.set_time_base(time_base);
        let mut options = Dictionary::new();
        options.set("video_track_timescale", &time_base.1.to_string());
        output.write_header_with(options).unwrap();
        assert_eq!(output.stream(0).unwrap().time_base(), time_base);
        let cancelled = AtomicBool::new(false);
        let drain = |encoder: &mut encoder::video::Encoder, output: &mut format::context::Output| {
            let mut packet = Packet::empty();
            while encoder.receive_packet(&mut packet).is_ok() {
                let index = clock.window_pts.iter().position(|pts| Some(*pts) == packet.pts()).unwrap();
                let duration = clock.window_pts.get(index + 1)
                    .map_or(clock.window_final_duration, |next| next - clock.window_pts[index]);
                packet.set_duration(duration);
                write_packet(&mut packet, 0, time_base, time_base, output, &cancelled).unwrap();
            }
        };
        for (index, pts) in clock.window_pts.iter().enumerate() {
            let mut frame = frame::Video::new(format::Pixel::YUV420P, 64, 48);
            frame.set_pts(Some(*pts));
            for plane in 0..3 {
                frame.data_mut(plane).fill(if plane == 0 { 16 + index as u8 * 7 } else { 128 });
            }
            encoder.send_frame(&frame).unwrap();
            drain(&mut encoder, &mut output);
        }
        encoder.send_eof().unwrap();
        drain(&mut encoder, &mut output);
        output.write_trailer().unwrap();
        drop(output);
        let result = crate::run(
            crate::TranscodeRequest {
                input: &source,
                output: &target,
                backend: crate::Backend::Library,
                cancelled: &cancelled,
            },
            &mut |_| {},
        ).unwrap();
        assert_eq!(result.operation, crate::Operation::Remux);
        let fresh = probe(&target, &cancelled, false).unwrap().video.unwrap();
        let original = result.source.video.unwrap();
        assert_eq!(fresh.time_base, original.time_base);
        assert_eq!(fresh.frame_rate, original.frame_rate);
        assert_eq!(fresh.frames, 30);
        assert!(fresh.constant_frame_rate);
        assert_eq!(fresh.start_seconds, original.start_seconds);
        assert_eq!(fresh.display_matrix, original.display_matrix);
        assert_eq!(fresh.color, original.color);
        let packets = |path: &Path| {
            let mut input = open_local_input(path).unwrap();
            input.packets().map(|(_, packet)| {
                (packet.pts(), packet.dts(), packet.duration(), packet.data().unwrap().to_vec())
            }).collect::<Vec<_>>()
        };
        let source_packets = packets(&source);
        assert_eq!(source_packets.iter().map(|packet| packet.0.unwrap()).collect::<Vec<_>>(), clock.window_pts);
        assert_eq!(packets(&target), source_packets);
    }

    #[test]
    fn h264_profile_is_accepted_by_generic_codec_options() {
        init().unwrap();
        let options = video_encoder_options(VideoMode::H264);
        let profile = std::ffi::CString::new(options.get("profile").unwrap()).unwrap();
        let mut context = codec::Context::new();
        // h264_mf uses AVCodecContext's generic option, not VideoToolbox's named constants.
        // Both pointers stay valid for this synchronous option parse; no encoder is opened.
        let status = unsafe {
            ffi::av_opt_set(
                context.as_mut_ptr().cast(),
                c"profile".as_ptr(),
                profile.as_ptr(),
                0,
            )
        };
        assert_eq!(
            status, 0,
            "H.264 profile must parse without codec-private aliases"
        );
        assert_eq!(
            unsafe { (*context.as_ptr()).profile },
            ffi::FF_PROFILE_H264_HIGH
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn h264_videotoolbox_allows_software_without_requiring_it() {
        let options = video_encoder_options(VideoMode::H264);
        assert_eq!(options.get("allow_sw"), Some("1"));
        assert_eq!(options.get("require_sw"), None);
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn h264_does_not_pass_videotoolbox_options_to_other_encoders() {
        let options = video_encoder_options(VideoMode::H264);
        assert_eq!(options.get("allow_sw"), None);
        assert_eq!(options.get("require_sw"), None);
    }

    #[test]
    fn prores_options_preserve_alpha_without_h264_fallback_options() {
        let options = video_encoder_options(VideoMode::Prores);
        assert_eq!(options.get("profile"), Some("4"));
        assert_eq!(options.get("alpha_bits"), Some("8"));
        assert_eq!(options.get("allow_sw"), None);
        assert_eq!(options.get("require_sw"), None);
    }

    #[test]
    fn ae_destination_remux_retains_high_precision_prores_alpha_packets() {
        init().unwrap();
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("alpha.mov");
        let target = directory.path().join("prepared.mov");
        let codec = encoder::find_by_name("prores_ks").unwrap();
        let mut output = format::output_as(&source, "mov").unwrap();
        let mut stream = output.add_stream(codec).unwrap();
        let mut context = codec::Context::new_with_codec(codec)
            .encoder()
            .video()
            .unwrap();
        context.set_width(16);
        context.set_height(16);
        context.set_format(format::Pixel::YUVA444P10LE);
        context.set_time_base(Rational(1, 24));
        context.set_frame_rate(Some(Rational(24, 1)));
        context.set_flags(codec::Flags::GLOBAL_HEADER);
        let mut options = Dictionary::new();
        options.set("profile", "4");
        options.set("alpha_bits", "16");
        let mut encoder = context.open_with(options).unwrap();
        stream.set_parameters(&encoder);
        stream.set_time_base(Rational(1, 24));
        let mut options = Dictionary::new();
        options.set("use_editlist", "1");
        options.set("movie_timescale", "24");
        output.write_header_with(options).unwrap();
        let output_time_base = output.stream(0).unwrap().time_base();
        let cancelled = AtomicBool::new(false);
        for index in 0..2 {
            let mut frame = frame::Video::new(format::Pixel::YUVA444P10LE, 16, 16);
            frame.set_pts(Some(index));
            for plane in 0..4 {
                for (offset, bytes) in frame.data_mut(plane).chunks_exact_mut(2).enumerate() {
                    let value = if plane == 3 {
                        ((offset % 256) * 4) as u16
                    } else {
                        512
                    };
                    bytes.copy_from_slice(&value.to_le_bytes());
                }
            }
            encoder.send_frame(&frame).unwrap();
            let mut packet = Packet::empty();
            while encoder.receive_packet(&mut packet).is_ok() {
                packet.set_duration(1);
                write_packet(
                    &mut packet,
                    0,
                    Rational(1, 24),
                    output_time_base,
                    &mut output,
                    &cancelled,
                )
                .unwrap();
            }
        }
        encoder.send_eof().unwrap();
        let mut packet = Packet::empty();
        while encoder.receive_packet(&mut packet).is_ok() {
            packet.set_duration(1);
            write_packet(
                &mut packet,
                0,
                Rational(1, 24),
                output_time_base,
                &mut output,
                &cancelled,
            )
            .unwrap();
        }
        output.write_trailer().unwrap();
        drop(output);
        let result = crate::run_for_after_effects(
            crate::TranscodeRequest {
                input: &source,
                output: &target,
                backend: crate::Backend::Library,
                cancelled: &cancelled,
            },
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(result.operation, crate::Operation::Remux);
        let video = result.source.video.unwrap();
        assert!(video.alpha);
        assert!(
            !video.has_eight_bit_alpha(),
            "precision must not enter the eight-bit encoder path"
        );
        let prepared = result.media.video.unwrap();
        assert!(prepared.alpha && !prepared.has_eight_bit_alpha());
        let video_tag = |path: &Path| {
            let input = open_local_input(path).unwrap();
            let stream = input
                .streams()
                .find(|stream| stream.parameters().medium() == media::Type::Video)
                .unwrap();
            // SAFETY: copy scalar metadata while the stream parameters live.
            unsafe { (*stream.parameters().as_ptr()).codec_tag }
        };
        assert_eq!(video_tag(&source), u32::from_le_bytes(*b"ap4h"));
        assert_eq!(
            video_tag(&target),
            video_tag(&source),
            "remux must preserve the supported ProRes 4444 sample entry"
        );
        let packets = |path: &Path| {
            let mut input = open_local_input(path).unwrap();
            input
                .packets()
                .filter(|(stream, _)| stream.parameters().medium() == media::Type::Video)
                .map(|(_, packet)| packet.data().unwrap().to_vec())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            packets(&source),
            packets(&target),
            "coded alpha and RGB bytes are unchanged"
        );
        assert_eq!(fx_conv::sha256_file(&source).unwrap(), result.input_sha256);
    }
    #[test]
    fn native_camera_metadata_keeps_later_tmcd_index_and_rejects_unknown_data() {
        init().unwrap();
        let directory = tempfile::tempdir().unwrap();
        let mut input = format::output_as(&directory.path().join("source.mov"), "mov").unwrap();
        for tag in [*b"rtmd", *b"mebx", *b"mebx", *b"tmcd"] {
            let mut stream = input.add_stream(encoder::find(codec::Id::None)).unwrap();
            // SAFETY: parameters are owned by the live stream and exclusively mutated here.
            unsafe {
                let parameters = (*stream.as_mut_ptr()).codecpar;
                (*parameters).codec_type = ffi::AVMediaType::AVMEDIA_TYPE_DATA;
                (*parameters).codec_tag = u32::from_le_bytes(tag);
            }
            let mut metadata = Dictionary::new();
            if tag != *b"mebx" {
                metadata.set("timecode", "01:02:03:04");
            }
            stream.set_metadata(metadata);
        }
        let data = probe_data_streams(&input, "mov,mp4,m4a,3gp,3g2,mj2").unwrap();
        assert_eq!(data.timecode_stream_index, Some(3));
        assert_eq!(data.timecode.as_deref(), Some("01:02:03:04"));
        assert_eq!(
            data.camera_metadata
                .iter()
                .map(|track| track.stream_index)
                .collect::<Vec<_>>(),
            [0, 1, 2]
        );
        assert!(data.camera_metadata[0].has_timecode_label);
        assert!(probe_data_streams(&input, "matroska").is_err());
        let mut stream = input.stream_mut(1).unwrap();
        // SAFETY: same exclusively borrowed, live stream parameters as above.
        unsafe {
            (*(*stream.as_mut_ptr()).codecpar).codec_tag = u32::from_le_bytes(*b"zzzz");
        }
        assert!(probe_data_streams(&input, "mov").is_err());
    }

    #[test]
    fn copied_timecode_retains_the_frame_rate_required_by_mov_muxing() {
        init().unwrap();
        let directory = tempfile::tempdir().unwrap();
        let mut input = format::output_as(&directory.path().join("source.mov"), "mov").unwrap();
        let mut source = input.add_stream(encoder::find(codec::Id::None)).unwrap();
        source.set_time_base(Rational(1, 24));
        source.set_avg_frame_rate(Rational(24, 1));
        let mut output = format::output_as(&directory.path().join("output.mov"), "mov").unwrap();
        let copied = make_data_output(&input.stream(0).unwrap(), &mut output).unwrap();
        assert_eq!(output.stream(copied.index).unwrap().avg_frame_rate(), Rational(24, 1));
    }

    #[test]
    fn video_remux_copies_non_aac_audio_without_opening_an_encoder() {
        init().unwrap();
        let directory = tempfile::tempdir().unwrap();
        let mut input = format::output_as(&directory.path().join("source.mov"), "mov").unwrap();
        let mut source = input.add_stream(encoder::find(codec::Id::None)).unwrap();
        // Only codec identity is needed for packet copying; no encoder context exists.
        unsafe {
            (*source.parameters().as_mut_ptr()).codec_type = ffi::AVMediaType::AVMEDIA_TYPE_AUDIO;
            (*source.parameters().as_mut_ptr()).codec_id = ffi::AVCodecID::AV_CODEC_ID_PCM_S16LE;
        }
        source.set_time_base(Rational(1, 48000));
        let mut output = format::output_as(&directory.path().join("output.mov"), "mov").unwrap();
        let audio = make_embedded_audio_output(
            &input.stream(0).unwrap(),
            &mut output,
            VideoMode::Copy,
            true,
        ).unwrap();
        assert!(matches!(audio, AudioMux::Copy { .. }));
        assert_eq!(output.stream(0).unwrap().parameters().id(), codec::Id::PCM_S16LE);
    }

    /// Writes `samples` of mono 44.1 kHz AAC with `muxer`. In MP4, like common
    /// camera files, the edit list skips 2112 priming samples, which end inside
    /// the third AAC frame, so the first kept frame is partially skipped.
    fn write_aac(path: &Path, muxer: &str, samples: usize) {
        let codec = encoder::find(codec::Id::AAC).unwrap();
        let mut output = format::output_as(path, muxer).unwrap();
        let mut stream = output.add_stream(codec).unwrap();
        let mut context = codec::Context::new_with_codec(codec)
            .encoder()
            .audio()
            .unwrap();
        let planar = ffmpeg::format::Sample::F32(ffmpeg::format::sample::Type::Planar);
        context.set_rate(44_100);
        context.set_channel_layout(ChannelLayout::MONO);
        context.set_format(planar);
        context.set_time_base(Rational(1, 44_100));
        context.set_flags(codec::Flags::GLOBAL_HEADER);
        let mut encoder = context.open().unwrap();
        stream.set_parameters(&encoder);
        stream.set_time_base(Rational(1, 44_100));
        output.write_header().unwrap();
        let output_time_base = output.stream(0).unwrap().time_base();
        let cancelled = AtomicBool::new(false);
        let drain = |encoder: &mut encoder::audio::Encoder,
                         output: &mut format::context::Output| {
            let mut packet = Packet::empty();
            while encoder.receive_packet(&mut packet).is_ok() {
                write_packet(&mut packet, 0, Rational(1, 44_100), output_time_base, output, &cancelled)
                    .unwrap();
            }
        };
        let mut written = 0;
        while written < samples {
            let count = (samples - written).min(1024);
            let mut frame = frame::Audio::new(planar, count, ChannelLayout::MONO);
            frame.set_rate(44_100);
            // The encoder adds 1024 samples of delay; the first packet is at -2112.
            frame.set_pts(Some(written as i64 - 1088));
            for (offset, sample) in frame.plane_mut::<f32>(0).iter_mut().enumerate() {
                *sample = ((written + offset) as f32 * 0.0626).sin() * 0.25;
            }
            encoder.send_frame(&frame).unwrap();
            drain(&mut encoder, &mut output);
            written += count;
        }
        encoder.send_eof().unwrap();
        drain(&mut encoder, &mut output);
        output.write_trailer().unwrap();
    }

    #[test]
    fn primed_aac_transcodes_to_pcm_at_its_declared_length() {
        init().unwrap();
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("source.m4a");
        // Not a multiple of the 1024-sample AAC frame, so the last frame is padded.
        // FFmpeg 7 decodes that padding; the output must stop at the declared length.
        write_aac(&input, "ipod", 88_641);
        let output = directory.path().join("prepared.wav");
        let cancelled = AtomicBool::new(false);
        let result = crate::run(
            crate::TranscodeRequest {
                input: &input,
                output: &output,
                backend: crate::Backend::Library,
                cancelled: &cancelled,
            },
            &mut |_| {},
        )
        .unwrap();
        let declared = result.source.audio.unwrap().duration_seconds;
        assert!(declared > 1.9, "unexpected fixture length {declared}");
        let written = result.media.audio.unwrap().duration_seconds;
        assert_eq!((written * 44_100.0).round(), (declared * 44_100.0).round());
    }

    #[test]
    fn estimated_aac_length_is_not_used_to_cut_audio() {
        init().unwrap();
        let directory = tempfile::tempdir().unwrap();
        // Raw ADTS has no length header; libavformat estimates it from bitrate.
        let input = directory.path().join("source.aac");
        write_aac(&input, "adts", 88_641);
        let output = directory.path().join("prepared.wav");
        let cancelled = AtomicBool::new(false);
        let result = crate::run(
            crate::TranscodeRequest {
                input: &input,
                output: &output,
                backend: crate::Backend::Library,
                cancelled: &cancelled,
            },
            &mut |_| {},
        );
        // The decoded length differs from the estimate, so policy must reject it
        // rather than publish audio trimmed to a guess.
        assert!(
            matches!(&result, Err(crate::TranscodeError::Policy(reason)) if reason.contains("audio timing")),
            "{result:?}"
        );
        assert!(!output.exists());
    }

    #[test]
    fn progress_callback_is_throttled_and_ignores_negative_time() {
        let cancelled = AtomicBool::new(false);
        let mut values = Vec::new();
        let mut callback = |value| values.push(value);
        let mut progress = Progress {
            last: -PROGRESS_INTERVAL_SECONDS,
            callback: &mut callback,
            cancelled: &cancelled,
        };
        progress.report(-1.0).unwrap();
        progress.report(0.1).unwrap();
        progress.report(0.25).unwrap();
        progress.report(0.35).unwrap();
        assert_eq!(values, [0.1, 0.35]);
    }

    #[test]
    fn progress_callback_can_cancel_an_in_flight_operation() {
        let cancelled = AtomicBool::new(false);
        let mut callback = |_| cancelled.store(true, Ordering::Relaxed);
        let mut progress = Progress {
            last: -PROGRESS_INTERVAL_SECONDS,
            callback: &mut callback,
            cancelled: &cancelled,
        };
        progress.report(0.0).unwrap();
        assert!(matches!(progress.report(0.25), Err(Error::Cancelled)));
    }

    #[test]
    fn pixel_conversion_honors_full_range_and_bt709_matrix() {
        for (rgb, expected_luma) in [([255, 255, 255], 255), ([0, 0, 255], 18)] {
            let mut source = frame::Video::new(format::Pixel::RGB24, 16, 16);
            let stride = source.stride(0);
            for row in source.data_mut(0).chunks_exact_mut(stride).take(16) {
                for pixel in row[..48].chunks_exact_mut(3) {
                    pixel.copy_from_slice(&rgb);
                }
            }
            let mut scaler = software::scaling::Context::get(
                format::Pixel::RGB24,
                16,
                16,
                format::Pixel::YUV420P,
                16,
                16,
                software::scaling::Flags::BICUBIC,
            )
            .unwrap();
            configure_scaler_color(
                &mut scaler,
                format::Pixel::RGB24,
                ffi::AVColorSpace::AVCOL_SPC_BT709,
                ffi::AVColorRange::AVCOL_RANGE_JPEG,
            )
            .unwrap();
            let mut output = frame::Video::empty();
            scaler.run(&source, &mut output).unwrap();
            assert_eq!(output.data(0)[0], expected_luma);
        }
    }

    #[test]
    fn alpha_copy_preserves_all_eight_bit_codes_before_encoding() {
        for (pixel, plane, step, offset) in [
            (format::Pixel::ARGB, 0, 4, 0),
            (format::Pixel::RGBA, 0, 4, 3),
            (format::Pixel::YUVA444P, 3, 1, 0),
        ] {
            let mut source = frame::Video::new(pixel, 256, 1);
            for value in 0..256 {
                source.data_mut(plane)[value * step + offset] = value as u8;
            }
            let mut destination = frame::Video::new(format::Pixel::YUVA444P10LE, 256, 1);
            copy_eight_bit_alpha(&source, &mut destination).unwrap();
            for (value, bytes) in destination.data(3)[..512].chunks_exact(2).enumerate() {
                assert_eq!(
                    u16::from_le_bytes(bytes.try_into().unwrap()) >> 2,
                    value as u16
                );
            }
        }
    }
}
