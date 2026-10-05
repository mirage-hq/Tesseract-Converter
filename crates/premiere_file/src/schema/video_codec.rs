//! Video codecs that conversion reads from a packaged file, and which of them
//! each direction accepts.

use super::records;

/// One video codec that inspection classifies, identified by its ISO BMFF
/// sample-entry type.
///
/// H.264 and HEVC are decoded by the native export renderer and the web
/// player (WebCodecs), and each is a format Adobe documents Premiere
/// importing. AME has rendered the derived HEVC input; generated HEVC Premiere
/// projects remain unverified. Entry types that allow in-band parameter-set
/// changes (`avc3`, `hev1`) are excluded because inspection could not see
/// those changes. Dolby Vision entries (`dvh1`, `dvhe`) are excluded because
/// neither renderer maps them to its HEVC decoder
/// (`video_format::validate_codec`); a Dolby Vision record on an `hvc1`
/// entry, the iPhone form, is checked by `video_format::hevc`.
///
/// Apple ProRes is classified for the Adobe-bound direction only: Premiere and
/// After Effects decode every ProRes profile, and the After Effects exporter
/// already packages it, so a Tesseract document's ProRes asset passes through
/// to a Premiere or linked AEP package byte for byte. Import additionally
/// admits `ap4h` through the existing native FFmpeg decoder, with an explicit
/// diagnostic that WebCodecs playback is unavailable. Other profiles remain
/// transcode candidates on import ([`Self::tesseract_player_rejection`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VideoCodec {
    /// H.264 with its parameter sets in the sample entry (`avc1`).
    H264,
    /// HEVC Main or Main 10 with every parameter set in the sample entry
    /// (`hvc1`), with or without a Dolby Vision record.
    HevcMain,
    /// Apple ProRes in a QuickTime or MP4 container. `alpha` is whether a
    /// 4444 sample entry declares a 32-bit depth, the only ProRes form that
    /// carries an alpha channel.
    ProRes { profile: ProResProfile, alpha: bool },
}

/// The Apple ProRes profiles, by sample-entry type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProResProfile {
    /// `apco`
    Proxy,
    /// `apcs`
    Lt,
    /// `apcn`
    Standard,
    /// `apch`
    Hq,
    /// `ap4h`
    P4444,
    /// `ap4x`
    P4444Xq,
}

impl ProResProfile {
    fn from_sample_entry(sample_entry: [u8; 4]) -> Option<Self> {
        Some(match &sample_entry {
            b"apco" => Self::Proxy,
            b"apcs" => Self::Lt,
            b"apcn" => Self::Standard,
            b"apch" => Self::Hq,
            b"ap4h" => Self::P4444,
            b"ap4x" => Self::P4444Xq,
            _ => return None,
        })
    }

    /// The sample-entry type of the profile.
    pub(crate) fn sample_entry(self) -> &'static str {
        match self {
            Self::Proxy => "apco",
            Self::Lt => "apcs",
            Self::Standard => "apcn",
            Self::Hq => "apch",
            Self::P4444 => "ap4h",
            Self::P4444Xq => "ap4x",
        }
    }

    /// Whether the profile stores 4:4:4 picture and may carry alpha.
    pub(crate) fn is_4444(self) -> bool {
        matches!(self, Self::P4444 | Self::P4444Xq)
    }

    /// The profile's name as Apple documents it.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Proxy => "Apple ProRes 422 Proxy",
            Self::Lt => "Apple ProRes 422 LT",
            Self::Standard => "Apple ProRes 422",
            Self::Hq => "Apple ProRes 422 HQ",
            Self::P4444 => "Apple ProRes 4444",
            Self::P4444Xq => "Apple ProRes 4444 XQ",
        }
    }

    /// The native `VideoStream.CodecType` that Premiere saves for the profile:
    /// the big-endian code of its sample-entry type, as the corpus holds it
    /// (`ap4h` 1634743400 on five Podcast Opener masters, `apco` 1634755439,
    /// `apcs` 1634755443 and `apcn` 1634755438 on Co-Editor and Komodo
    /// masters); `apch` and `ap4x` follow the same rule by inference.
    fn codec_type(self) -> &'static str {
        match self {
            Self::Proxy => records::PRORES_PROXY_CODEC_TYPE,
            Self::Lt => records::PRORES_LT_CODEC_TYPE,
            Self::Standard => records::PRORES_CODEC_TYPE,
            Self::Hq => records::PRORES_HQ_CODEC_TYPE,
            Self::P4444 => records::PRORES_4444_CODEC_TYPE,
            Self::P4444Xq => records::PRORES_4444_XQ_CODEC_TYPE,
        }
    }
}

impl VideoCodec {
    /// Maps a sample-entry type to a classified codec. `depth` is the visual
    /// sample entry's depth field, 32 when a 4444 picture carries alpha.
    pub(crate) fn from_sample_entry(sample_entry: [u8; 4], depth: u16) -> Option<Self> {
        Some(match &sample_entry {
            b"avc1" => Self::H264,
            b"hvc1" => Self::HevcMain,
            _ => {
                let profile = ProResProfile::from_sample_entry(sample_entry)?;
                Self::ProRes {
                    profile,
                    alpha: profile.is_4444() && depth == 32,
                }
            }
        })
    }

    /// Returns the native `VideoStream.CodecType` that export writes, the code
    /// Premiere itself saves for the codec family rather than the file's
    /// sample entry: [`records::CODEC_TYPE`] (`avc1`) for H.264, as Premiere
    /// 24.3-26.5.1 corpus projects store it (the Premiere 12.1.1
    /// `copy_and_paste_effects` project stores `AVC1` for a lowercase `avc1`
    /// file), [`records::HEVC_CODEC_TYPE`] (`HEVC`) for HEVC, as Premiere
    /// 26.5.1 saved it for three Main 10 `hvc1` masters, one with a Dolby
    /// Vision box, and the profile's own sample-entry code for ProRes
    /// ([`ProResProfile::codec_type`]).
    pub(crate) fn codec_type(self) -> &'static str {
        match self {
            Self::H264 => records::CODEC_TYPE,
            Self::HevcMain => records::HEVC_CODEC_TYPE,
            Self::ProRes { profile, .. } => profile.codec_type(),
        }
    }

    /// Whether the picture carries an alpha channel that Premiere reads
    /// (`AlphaType` 1, straight, as the corpus `ap4h` masters are saved).
    pub(crate) fn has_alpha(self) -> bool {
        matches!(self, Self::ProRes { alpha: true, .. })
    }

    /// The codec family name of an admitted Tesseract media descriptor.
    /// ProRes 4444 is native-only; import reports unavailable web playback.
    pub(crate) fn tesseract_name(self) -> Option<&'static str> {
        match self {
            Self::H264 => Some("h264"),
            Self::HevcMain => Some("hevc"),
            Self::ProRes {
                profile: ProResProfile::P4444,
                ..
            } => Some("prores"),
            Self::ProRes { .. } => None,
        }
    }

    /// Why import rejects this codec, if it does. ProRes 4444 can play through
    /// the existing native decoder, but no ProRes profile plays in WebCodecs.
    /// Other opaque profiles may explicitly prepare to H.264.
    pub(crate) fn tesseract_player_rejection(self) -> Option<String> {
        match self {
            Self::H264 | Self::HevcMain | Self::ProRes { profile: ProResProfile::P4444, .. } => None,
            Self::ProRes { profile, alpha } => Some(format!(
                "video codec {:?} ({}) is not decodable by the web player; import accepts H.264 (avc1) or HEVC (hvc1){}",
                profile.sample_entry(),
                profile.name(),
                if alpha {
                    ", and no web-playable preparation keeps this picture's alpha"
                } else {
                    ", so prepare the source with tsrct-conv transcode"
                }
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prores_profiles_classify_by_entry_and_write_their_saved_codec_codes() {
        for (entry, profile, code) in [
            (b"apco", ProResProfile::Proxy, "1634755439"),
            (b"apcs", ProResProfile::Lt, "1634755443"),
            (b"apcn", ProResProfile::Standard, "1634755438"),
            (b"apch", ProResProfile::Hq, "1634755432"),
            (b"ap4h", ProResProfile::P4444, "1634743400"),
            (b"ap4x", ProResProfile::P4444Xq, "1634743416"),
        ] {
            let codec = VideoCodec::from_sample_entry(*entry, 24).unwrap();
            assert_eq!(
                codec,
                VideoCodec::ProRes {
                    profile,
                    alpha: false
                }
            );
            assert_eq!(codec.codec_type(), code);
            assert_eq!(
                code,
                u32::from_be_bytes(*entry).to_string(),
                "the saved code is the big-endian entry"
            );
            if profile == ProResProfile::P4444 {
                assert_eq!(codec.tesseract_name(), Some("prores"));
                assert!(codec.tesseract_player_rejection().is_none());
            } else {
                assert!(codec.tesseract_name().is_none());
                assert!(codec
                    .tesseract_player_rejection()
                    .unwrap()
                    .contains(profile.name()));
            }
        }
        // Only a 32-bit 4444 entry carries alpha.
        assert!(VideoCodec::from_sample_entry(*b"ap4h", 32)
            .unwrap()
            .has_alpha());
        assert!(VideoCodec::from_sample_entry(*b"ap4x", 32)
            .unwrap()
            .has_alpha());
        assert!(!VideoCodec::from_sample_entry(*b"apch", 32)
            .unwrap()
            .has_alpha());
        assert!(!VideoCodec::from_sample_entry(*b"ap4h", 24)
            .unwrap()
            .has_alpha());
        assert_eq!(VideoCodec::from_sample_entry(*b"aprn", 24), None);
        assert_eq!(
            VideoCodec::from_sample_entry(*b"avc1", 24),
            Some(VideoCodec::H264)
        );
        assert!(VideoCodec::H264.tesseract_player_rejection().is_none());
    }
}
