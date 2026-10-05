//! Bounded, read-only media inspection through libavformat.

use std::io::{self, Read, Seek};

/// Facts collected from a media container without decoding its payloads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaInspection {
    pub streams: Vec<StreamInfo>,
    pub packets: Vec<PacketInfo>,
}

/// The broad media kind declared by a stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamKind {
    Video,
    Audio,
    Subtitle,
    Other,
}

/// Integer stream metadata reported by libavformat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamInfo {
    pub index: usize,
    pub id: i32,
    pub kind: StreamKind,
    pub codec_tag: [u8; 4],
    pub codec_name: String,
    pub width: u32,
    pub height: u32,
    /// Resolved stream/codec pixel ratio; None means FFmpeg reports it unspecified.
    pub sample_aspect_ratio: Option<[i32; 2]>,
    /// Native fixed-point display matrix, or None when no matrix is present.
    pub display_matrix: Option<[i32; 9]>,
    pub sample_rate: u32,
    pub channels: u32,
    pub time_base_num: i32,
    pub time_base_den: i32,
    pub duration: i64,
    pub frame_count: i64,
    pub extradata: Vec<u8>,
}

/// Integer timing and location facts for one video packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PacketInfo {
    pub stream_index: usize,
    pub dts: Option<i64>,
    pub pts: Option<i64>,
    pub duration: i64,
    pub position: i64,
    pub size: usize,
}

/// Failure to inspect caller-owned media bytes.
#[derive(Debug, thiserror::Error)]
pub enum InspectError {
    #[error("media input I/O failed: {0}")]
    Io(#[source] io::Error),
    #[error("invalid media: {0}")]
    Invalid(String),
    #[error("media inspection requires the ffmpeg-library feature")]
    Unavailable,
}

/// Inspect an ISO-BMFF input of the declared size without decoding or opening references.
///
/// The demuxer is fixed to MOV/MP4 so renamed media from unrelated container
/// families cannot cross callers' existing MP4 admission boundary.
pub fn inspect(
    reader: impl Read + Seek,
    size: u64,
    scan_video_packets: bool,
) -> Result<MediaInspection, InspectError> {
    imp::inspect_with_edits(reader, size, scan_video_packets, false)
}

/// Inspect the displayed MOV/MP4 clock, applying native edit lists to packet PTS.
/// Existing `inspect` callers retain their unedited media-clock behavior.
/// Like `inspect`, this never decodes or rewrites media.
pub fn inspect_presentation(
    reader: impl Read + Seek,
    size: u64,
) -> Result<MediaInspection, InspectError> {
    imp::inspect_with_edits(reader, size, true, true)
}

#[cfg(not(feature = "ffmpeg-library"))]
mod imp {
    use super::*;

    pub(super) fn inspect_with_edits(
        _reader: impl Read + Seek,
        _size: u64,
        _scan_video_packets: bool,
        _apply_edits: bool,
    ) -> Result<MediaInspection, InspectError> {
        Err(InspectError::Unavailable)
    }
}

#[cfg(feature = "ffmpeg-library")]
mod imp {
    use super::*;
    use ffmpeg_next::{self as ffmpeg, ffi};
    use std::{
        ffi::{c_char, c_int, c_void},
        io::SeekFrom,
        panic::{catch_unwind, AssertUnwindSafe},
        ptr, slice,
    };

    const IO_BUFFER_SIZE: usize = 32 * 1024;

    struct ReaderState<R> {
        reader: R,
        size: u64,
        position: u64,
        error: Option<io::Error>,
        panicked: bool,
    }

    impl<R> ReaderState<R> {
        fn fail(&mut self, error: io::Error) -> c_int {
            if self.error.is_none() {
                self.error = Some(error);
            }
            ffi::AVERROR(libc::EIO)
        }
    }

    unsafe extern "C" fn read_packet<R: Read + Seek>(
        opaque: *mut c_void,
        buffer: *mut u8,
        buffer_size: c_int,
    ) -> c_int {
        let state = unsafe { &mut *opaque.cast::<ReaderState<R>>() };
        let result = catch_unwind(AssertUnwindSafe(|| {
            let length = usize::try_from(buffer_size).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidInput, "negative FFmpeg read size")
            })?;
            let remaining = state.size.saturating_sub(state.position);
            let length = length.min(usize::try_from(remaining).unwrap_or(usize::MAX));
            if length == 0 {
                return Ok(0);
            }
            // FFmpeg provides a writable buffer of buffer_size bytes; the slice
            // is also capped at the caller's declared input boundary.
            let bytes = unsafe { slice::from_raw_parts_mut(buffer, length) };
            let count = state.reader.read(bytes)?;
            if count > length {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "reader exceeded buffer",
                ));
            }
            state.position += count as u64;
            Ok(count)
        }));
        match result {
            Ok(Ok(0)) => ffi::AVERROR_EOF,
            Ok(Ok(count)) => c_int::try_from(count).unwrap_or(ffi::AVERROR(libc::EIO)),
            Ok(Err(error)) => state.fail(error),
            Err(_) => {
                state.panicked = true;
                ffi::AVERROR(libc::EIO)
            }
        }
    }

    unsafe extern "C" fn seek<R: Read + Seek>(
        opaque: *mut c_void,
        offset: i64,
        whence: c_int,
    ) -> i64 {
        let state = unsafe { &mut *opaque.cast::<ReaderState<R>>() };
        let result = catch_unwind(AssertUnwindSafe(|| -> io::Result<u64> {
            if whence & ffi::AVSEEK_SIZE == ffi::AVSEEK_SIZE {
                return Ok(state.size);
            }
            let whence = whence & !ffi::AVSEEK_FORCE;
            let base = match whence {
                libc::SEEK_SET => 0_i128,
                libc::SEEK_CUR => i128::from(state.position),
                libc::SEEK_END => i128::from(state.size),
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "unsupported FFmpeg seek mode",
                    ));
                }
            };
            let target = base.checked_add(i128::from(offset)).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "FFmpeg seek overflow")
            })?;
            let target = u64::try_from(target).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidInput, "FFmpeg seek before input")
            })?;
            if target > state.size {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "FFmpeg seek beyond declared input size",
                ));
            }
            let position = state.reader.seek(SeekFrom::Start(target))?;
            if position != target {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "reader seek position mismatch",
                ));
            }
            state.position = position;
            Ok(position)
        }));
        match result {
            Ok(Ok(position)) => i64::try_from(position).unwrap_or_else(|_| {
                i64::from(state.fail(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "media position exceeds FFmpeg limits",
                )))
            }),
            Ok(Err(error)) => i64::from(state.fail(error)),
            Err(_) => {
                state.panicked = true;
                i64::from(ffi::AVERROR(libc::EIO))
            }
        }
    }

    unsafe extern "C" fn reject_external_input(
        _context: *mut ffi::AVFormatContext,
        _io: *mut *mut ffi::AVIOContext,
        _url: *const c_char,
        _flags: c_int,
        _options: *mut *mut ffi::AVDictionary,
    ) -> c_int {
        ffi::AVERROR(libc::EPERM)
    }

    struct Input<R> {
        format: *mut ffi::AVFormatContext,
        io: *mut ffi::AVIOContext,
        state: Box<ReaderState<R>>,
    }

    impl<R> Input<R> {
        fn callback_error(&mut self) -> Option<InspectError> {
            if self.state.panicked {
                Some(InspectError::Invalid(
                    "media reader callback panicked".into(),
                ))
            } else {
                self.state.error.take().map(InspectError::Io)
            }
        }
    }

    impl<R> Drop for Input<R> {
        fn drop(&mut self) {
            unsafe {
                if !self.format.is_null() {
                    ffi::avformat_close_input(&mut self.format);
                }
                if !self.io.is_null() {
                    // avio_context_free does not free the caller-owned buffer.
                    // FFmpeg may replace it, so free the current buffer pointer.
                    ffi::av_free((*self.io).buffer.cast());
                    (*self.io).buffer = ptr::null_mut();
                    ffi::avio_context_free(&mut self.io);
                }
            }
        }
    }

    #[cfg(test)]
    fn inspect<R: Read + Seek>(
        reader: R,
        size: u64,
        scan_video_packets: bool,
    ) -> Result<MediaInspection, InspectError> {
        inspect_with_edits(reader, size, scan_video_packets, false)
    }

    pub(super) fn inspect_with_edits<R: Read + Seek>(
        reader: R,
        size: u64,
        scan_video_packets: bool,
        apply_edits: bool,
    ) -> Result<MediaInspection, InspectError> {
        ffmpeg::init().map_err(|error| InspectError::Invalid(error.to_string()))?;
        let mut input = allocate_input(reader, size)?;
        let mut options = ptr::null_mut();
        let option_name = b"ignore_editlist\0";
        let option_value = if apply_edits { b"0\0" } else { b"1\0" };
        unsafe {
            ffi::av_dict_set(
                &mut options,
                option_name.as_ptr().cast(),
                option_value.as_ptr().cast(),
                0,
            );
        }
        let mov_name = b"mov\0";
        let mov_format = unsafe { ffi::av_find_input_format(mov_name.as_ptr().cast()) };
        if mov_format.is_null() {
            unsafe { ffi::av_dict_free(&mut options) };
            return Err(InspectError::Unavailable);
        }
        let open_result = unsafe {
            ffi::avformat_open_input(&mut input.format, ptr::null(), mov_format, &mut options)
        };
        unsafe { ffi::av_dict_free(&mut options) };
        check_result(&mut input, open_result, "open media input")?;
        // MOV headers contain the required metadata. find_stream_info may open
        // decoders and inspect frames, which ordinary admission must never do.
        let streams = collect_streams(input.format, size)?;
        let packets = if scan_video_packets {
            collect_video_packets(&mut input, &streams)?
        } else {
            Vec::new()
        };
        Ok(MediaInspection { streams, packets })
    }

    fn allocate_input<R: Read + Seek>(mut reader: R, size: u64) -> Result<Input<R>, InspectError> {
        i64::try_from(size)
            .map_err(|_| InspectError::Invalid("media size exceeds FFmpeg limits".into()))?;
        reader.seek(SeekFrom::Start(0)).map_err(InspectError::Io)?;
        let mut state = Box::new(ReaderState {
            reader,
            size,
            position: 0,
            error: None,
            panicked: false,
        });
        let buffer = unsafe { ffi::av_malloc(IO_BUFFER_SIZE).cast::<u8>() };
        if buffer.is_null() {
            return Err(InspectError::Invalid("allocate FFmpeg I/O buffer".into()));
        }
        let io = unsafe {
            ffi::avio_alloc_context(
                buffer,
                IO_BUFFER_SIZE as c_int,
                0,
                (&mut *state as *mut ReaderState<R>).cast(),
                Some(read_packet::<R>),
                None,
                Some(seek::<R>),
            )
        };
        if io.is_null() {
            unsafe { ffi::av_free(buffer.cast()) };
            return Err(InspectError::Invalid("allocate FFmpeg I/O context".into()));
        }
        let format = unsafe { ffi::avformat_alloc_context() };
        if format.is_null() {
            let mut io = io;
            unsafe {
                ffi::av_free((*io).buffer.cast());
                (*io).buffer = ptr::null_mut();
                ffi::avio_context_free(&mut io);
            }
            return Err(InspectError::Invalid(
                "allocate FFmpeg format context".into(),
            ));
        }
        unsafe {
            (*format).pb = io;
            (*format).flags |= ffi::AVFMT_FLAG_CUSTOM_IO;
            (*format).io_open = Some(reject_external_input);
        }
        Ok(Input { format, io, state })
    }

    fn check_result<R>(
        input: &mut Input<R>,
        result: c_int,
        action: &str,
    ) -> Result<(), InspectError> {
        if let Some(error) = input.callback_error() {
            return Err(error);
        }
        if result >= 0 {
            return Ok(());
        }
        Err(InspectError::Invalid(format!(
            "{action}: {}",
            ffmpeg::Error::from(result)
        )))
    }

    fn collect_streams(
        format: *mut ffi::AVFormatContext,
        size: u64,
    ) -> Result<Vec<StreamInfo>, InspectError> {
        let count = unsafe { (*format).nb_streams as usize };
        let mut streams = Vec::new();
        for index in 0..count {
            let stream = unsafe { *(*format).streams.add(index) };
            if stream.is_null() {
                return Err(InspectError::Invalid("media contains a null stream".into()));
            }
            let parameters = unsafe { (*stream).codecpar };
            if parameters.is_null() {
                return Err(InspectError::Invalid(format!(
                    "stream {index} has no codec parameters"
                )));
            }
            let extra_size =
                usize::try_from(unsafe { (*parameters).extradata_size }).map_err(|_| {
                    InspectError::Invalid(format!("stream {index} has negative codec data size"))
                })?;
            if extra_size as u64 > size {
                return Err(InspectError::Invalid(format!(
                    "stream {index} codec data exceeds input size"
                )));
            }
            let extra = unsafe { (*parameters).extradata };
            if extra_size != 0 && extra.is_null() {
                return Err(InspectError::Invalid(format!(
                    "stream {index} codec data is missing"
                )));
            }
            let extradata = if extra_size == 0 {
                Vec::new()
            } else {
                unsafe { slice::from_raw_parts(extra, extra_size) }.to_vec()
            };
            let media_type = unsafe { (*parameters).codec_type };
            let kind = match media_type {
                ffi::AVMediaType::AVMEDIA_TYPE_VIDEO => StreamKind::Video,
                ffi::AVMediaType::AVMEDIA_TYPE_AUDIO => StreamKind::Audio,
                ffi::AVMediaType::AVMEDIA_TYPE_SUBTITLE => StreamKind::Subtitle,
                _ => StreamKind::Other,
            };
            let codec_id = unsafe { (*parameters).codec_id };
            let codec_name = unsafe {
                let name = ffi::avcodec_get_name(codec_id);
                if name.is_null() {
                    String::new()
                } else {
                    std::ffi::CStr::from_ptr(name)
                        .to_string_lossy()
                        .into_owned()
                }
            };
            let codec_tag = unsafe { (*parameters).codec_tag.to_le_bytes() };
            let time_base = unsafe { (*stream).time_base };
            let aspect =
                unsafe { ffi::av_guess_sample_aspect_ratio(format, stream, ptr::null_mut()) };
            let sample_aspect_ratio = (aspect.num != 0).then_some([aspect.num, aspect.den]);
            let display_matrix = unsafe {
                let side = ffi::av_packet_side_data_get(
                    (*parameters).coded_side_data,
                    (*parameters).nb_coded_side_data,
                    ffi::AVPacketSideDataType::AV_PKT_DATA_DISPLAYMATRIX,
                );
                if side.is_null() {
                    None
                } else {
                    if (*side).size != 9 * std::mem::size_of::<i32>() || (*side).data.is_null() {
                        return Err(InspectError::Invalid(format!(
                            "stream {index} has invalid display matrix data"
                        )));
                    }
                    let mut matrix = [0; 9];
                    for (index, value) in matrix.iter_mut().enumerate() {
                        *value = ptr::read_unaligned((*side).data.add(index * 4).cast::<i32>());
                    }
                    Some(matrix)
                }
            };
            streams.push(StreamInfo {
                index,
                id: unsafe { (*stream).id },
                kind,
                codec_tag,
                codec_name,
                width: u32::try_from(unsafe { (*parameters).width }).unwrap_or(0),
                height: u32::try_from(unsafe { (*parameters).height }).unwrap_or(0),
                sample_aspect_ratio,
                display_matrix,
                sample_rate: u32::try_from(unsafe { (*parameters).sample_rate }).unwrap_or(0),
                channels: u32::try_from(unsafe { (*parameters).ch_layout.nb_channels })
                    .unwrap_or(0),
                time_base_num: time_base.num,
                time_base_den: time_base.den,
                duration: unsafe { (*stream).duration },
                frame_count: unsafe { (*stream).nb_frames },
                extradata,
            });
        }
        Ok(streams)
    }

    fn collect_video_packets<R>(
        input: &mut Input<R>,
        streams: &[StreamInfo],
    ) -> Result<Vec<PacketInfo>, InspectError> {
        let packet = unsafe { ffi::av_packet_alloc() };
        if packet.is_null() {
            return Err(InspectError::Invalid("allocate FFmpeg packet".into()));
        }
        struct Packet(*mut ffi::AVPacket);
        impl Drop for Packet {
            fn drop(&mut self) {
                unsafe { ffi::av_packet_free(&mut self.0) }
            }
        }
        let packet = Packet(packet);
        let mut packets = Vec::new();
        loop {
            let result = unsafe { ffi::av_read_frame(input.format, packet.0) };
            if result == ffi::AVERROR_EOF {
                if let Some(error) = input.callback_error() {
                    return Err(error);
                }
                break;
            }
            check_result(input, result, "read media packet")?;
            let stream_index = usize::try_from(unsafe { (*packet.0).stream_index })
                .map_err(|_| InspectError::Invalid("packet has negative stream index".into()))?;
            if streams.get(stream_index).map(|stream| stream.kind) == Some(StreamKind::Video) {
                let no_timestamp = ffi::AV_NOPTS_VALUE;
                let dts = unsafe { (*packet.0).dts };
                let pts = unsafe { (*packet.0).pts };
                packets.push(PacketInfo {
                    stream_index,
                    dts: (dts != no_timestamp).then_some(dts),
                    pts: (pts != no_timestamp).then_some(pts),
                    duration: unsafe { (*packet.0).duration },
                    position: unsafe { (*packet.0).pos },
                    size: usize::try_from(unsafe { (*packet.0).size }).map_err(|_| {
                        InspectError::Invalid("packet has negative payload size".into())
                    })?,
                });
            }
            unsafe { ffi::av_packet_unref(packet.0) };
        }
        Ok(packets)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::io::Cursor;

        #[test]
        fn inspection_reports_non_square_pixels_and_native_display_matrix() {
            let non_square =
                include_bytes!("../../premiere_file/tests/fixtures/video-nonsquare.mp4");
            let result = super::inspect(
                std::io::Cursor::new(non_square),
                non_square.len() as u64,
                false,
            )
            .unwrap();
            let video = result
                .streams
                .iter()
                .find(|stream| stream.kind == StreamKind::Video)
                .unwrap();
            let [num, den] = video.sample_aspect_ratio.unwrap();
            assert!(num > 0 && den > 0 && num != den);

            // Supplementary container mutation checks FFmpeg's matrix extraction,
            // not independently authored rotation fidelity or native export proof.
            let mut bytes = MP4.to_vec();
            let identity = [65536_i32, 0, 0, 0, 65536, 0, 0, 0, 1073741824];
            let identity_bytes: Vec<_> = identity.into_iter().flat_map(i32::to_be_bytes).collect();
            let offset = bytes
                .windows(identity_bytes.len())
                .position(|window| window == identity_bytes)
                .unwrap();
            // The first matrix is mvhd; edit the subsequent video tkhd matrix.
            let start = offset + identity_bytes.len();
            let offset = bytes[start..]
                .windows(identity_bytes.len())
                .position(|window| window == identity_bytes)
                .unwrap()
                + start;
            let rotation = [0_i32, 65536, 0, -65536, 0, 0, 0, 0, 1073741824];
            let rotation_bytes: Vec<_> = rotation.into_iter().flat_map(i32::to_be_bytes).collect();
            bytes[offset..offset + rotation_bytes.len()].copy_from_slice(&rotation_bytes);
            let result =
                super::inspect(std::io::Cursor::new(&bytes), bytes.len() as u64, false).unwrap();
            let video = result
                .streams
                .iter()
                .find(|stream| stream.kind == StreamKind::Video)
                .unwrap();
            assert_eq!(video.display_matrix, Some(rotation));
        }

        const MP4: &[u8] =
            include_bytes!("../../premiere_file/tests/fixtures/feature_rate_24_blue.mp4");

        struct FailingReader {
            inner: Cursor<&'static [u8]>,
            remaining: usize,
        }

        impl Read for FailingReader {
            fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
                if self.remaining == 0 {
                    return Err(io::Error::other("injected failure"));
                }
                let length = output.len().min(self.remaining);
                let count = self.inner.read(&mut output[..length])?;
                self.remaining -= count;
                Ok(count)
            }
        }

        impl Seek for FailingReader {
            fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
                self.inner.seek(position)
            }
        }

        struct PanickingReader;

        impl Read for PanickingReader {
            fn read(&mut self, _output: &mut [u8]) -> io::Result<usize> {
                panic!("injected reader panic")
            }
        }

        impl Seek for PanickingReader {
            fn seek(&mut self, _position: SeekFrom) -> io::Result<u64> {
                Ok(0)
            }
        }

        #[test]
        fn metadata_and_video_packets_are_selected_explicitly() {
            let metadata = inspect(Cursor::new(MP4), MP4.len() as u64, false).unwrap();
            assert!(metadata.packets.is_empty());
            assert!(metadata
                .streams
                .iter()
                .any(|stream| stream.kind == StreamKind::Video));

            let scanned = inspect(Cursor::new(MP4), MP4.len() as u64, true).unwrap();
            assert!(!scanned.packets.is_empty());
            assert!(scanned
                .packets
                .iter()
                .all(|packet| { scanned.streams[packet.stream_index].kind == StreamKind::Video }));
        }

        #[test]
        fn unrelated_container_is_not_admitted_as_mp4() {
            let wav = b"RIFF\x24\0\0\0WAVEfmt \x10\0\0\0\x01\0\x01\0\x40\x1f\0\0\x80\x3e\0\0\x02\0\x10\0data\0\0\0\0";
            let error = inspect(Cursor::new(wav), wav.len() as u64, false).unwrap_err();
            assert!(matches!(error, InspectError::Invalid(_)));
        }

        #[test]
        fn caller_io_error_is_preserved() {
            let reader = FailingReader {
                inner: Cursor::new(MP4),
                remaining: 16,
            };
            let error = inspect(reader, MP4.len() as u64, false).unwrap_err();
            assert!(
                matches!(error, InspectError::Io(error) if error.to_string() == "injected failure")
            );
        }

        #[test]
        fn reader_panic_does_not_cross_the_c_boundary() {
            let error = inspect(PanickingReader, 1, false).unwrap_err();
            assert!(
                matches!(error, InspectError::Invalid(message) if message.contains("panicked"))
            );
        }

        #[test]
        fn read_callback_cannot_cross_the_declared_boundary() {
            let mut state = ReaderState {
                reader: Cursor::new([1_u8, 2, 3, 4]),
                size: 2,
                position: 0,
                error: None,
                panicked: false,
            };
            let opaque = (&mut state as *mut ReaderState<Cursor<[u8; 4]>>).cast();
            let mut output = [0_u8; 4];
            assert_eq!(
                unsafe { read_packet::<Cursor<[u8; 4]>>(opaque, output.as_mut_ptr(), 4) },
                2
            );
            assert_eq!(output, [1, 2, 0, 0]);
            assert_eq!(
                unsafe { read_packet::<Cursor<[u8; 4]>>(opaque, output.as_mut_ptr(), 4) },
                ffi::AVERROR_EOF
            );
        }

        #[test]
        fn demuxer_success_does_not_hide_callback_io_failure() {
            let mut input = allocate_input(Cursor::new(MP4), MP4.len() as u64).unwrap();
            input.state.error = Some(io::Error::other("injected late I/O failure"));
            assert!(matches!(
                check_result(&mut input, 0, "test"),
                Err(InspectError::Io(_))
            ));
        }

        #[test]
        fn inspection_resets_the_callers_cursor() {
            let mut reader = Cursor::new(MP4);
            reader.set_position(MP4.len() as u64);
            let metadata = inspect(reader, MP4.len() as u64, false).unwrap();
            assert!(metadata
                .streams
                .iter()
                .any(|stream| stream.kind == StreamKind::Video));
        }

        #[test]
        fn seek_beyond_declared_size_is_rejected() {
            let mut state = ReaderState {
                reader: Cursor::new([0_u8; 4]),
                size: 4,
                position: 0,
                error: None,
                panicked: false,
            };
            let result = unsafe {
                seek::<Cursor<[u8; 4]>>(
                    (&mut state as *mut ReaderState<Cursor<[u8; 4]>>).cast(),
                    5,
                    libc::SEEK_SET,
                )
            };
            assert!(result < 0);
            assert_eq!(state.error.unwrap().kind(), io::ErrorKind::UnexpectedEof);
        }
    }
}

#[cfg(all(test, not(feature = "ffmpeg-library")))]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn inspection_is_explicitly_unavailable_without_ffmpeg() {
        let error = inspect(Cursor::new(Vec::<u8>::new()), 0, false).unwrap_err();
        assert!(matches!(error, InspectError::Unavailable));
    }
}
