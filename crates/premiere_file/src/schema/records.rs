//! XML record definitions, reference names, and fixed writer output values in
//! the supported Premiere XML subset.
//!
//! Class IDs, versions, and writer values reproduce a from-scratch Premiere
//! 26.3 project. They are compatibility output, not verified format
//! requirements unless a nearby comment says so; the reader does not validate
//! them. Obvious single-use values stay inline at their writer call sites.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct XmlRecordDefinition {
    pub(crate) tag: &'static str,
    pub(crate) class_id: &'static str,
    pub(crate) version: &'static str,
}

impl XmlRecordDefinition {
    pub(crate) const fn new(
        tag: &'static str,
        class_id: &'static str,
        version: &'static str,
    ) -> Self {
        Self {
            tag,
            class_id,
            version,
        }
    }
}

pub(crate) const PROJECT: XmlRecordDefinition =
    XmlRecordDefinition::new("Project", "62ad66dd-0dcd-42da-a660-6d8fbde94876", "45");
pub(crate) const ROOT_PROJECT_ITEM: XmlRecordDefinition = XmlRecordDefinition::new(
    "RootProjectItem",
    "1c307a89-9318-47d7-a583-bf2553736543",
    "1",
);
pub(crate) const PROJECT_SETTINGS: XmlRecordDefinition = XmlRecordDefinition::new(
    "ProjectSettings",
    "50c16708-a1a1-4d2f-98d5-4e283ae28353",
    "21",
);
pub(crate) const SCRATCH_DISK_SETTINGS: XmlRecordDefinition = XmlRecordDefinition::new(
    "ScratchDiskSettings",
    "4c6ed82b-a81c-4df1-8bd0-750504c4b560",
    "4",
);
pub(crate) const INGEST_SETTINGS: XmlRecordDefinition = XmlRecordDefinition::new(
    "IngestSettings",
    "2db8f76b-2c37-48ee-925d-9a4f7278152d",
    "2",
);
pub(crate) const WORKSPACE_SETTINGS: XmlRecordDefinition = XmlRecordDefinition::new(
    "WorkspaceSettings",
    "c4372273-e1aa-4683-98aa-a2ceadf3066c",
    "1",
);
pub(crate) const DUMMY_CAPTURE_SETTINGS: XmlRecordDefinition = XmlRecordDefinition::new(
    "DummyCaptureSettings",
    "328c2aa2-47f9-4211-805b-b6a6dbd4ca29",
    "1",
);
pub(crate) const DEFAULT_SEQUENCE_SETTINGS: XmlRecordDefinition = XmlRecordDefinition::new(
    "DefaultSequenceSettings",
    "567bdf53-d6d9-4d61-b2f1-f4834bebea9b",
    "2",
);
pub(crate) const CLIP_PROJECT_ITEM: XmlRecordDefinition = XmlRecordDefinition::new(
    "ClipProjectItem",
    "cb4e0ed7-aca1-4171-8525-e3658dec06dd",
    "1",
);
pub(crate) const MASTER_CLIP: XmlRecordDefinition =
    XmlRecordDefinition::new("MasterClip", "fb11c33a-b0a9-4465-aa94-b6d5db2628cf", "12");
pub(crate) const CLIP_LOGGING_INFO: XmlRecordDefinition = XmlRecordDefinition::new(
    "ClipLoggingInfo",
    "77ab7fdd-dcdf-465d-9906-7a330ca1e738",
    "10",
);
pub(crate) const AUDIO_COMPONENT_CHAIN: XmlRecordDefinition = XmlRecordDefinition::new(
    "AudioComponentChain",
    "3cb131d1-d3c0-47ae-a19a-bdf75ea11674",
    "4",
);
pub(crate) const AUDIO_CLIP: XmlRecordDefinition =
    XmlRecordDefinition::new("AudioClip", "b8830d03-de02-41ee-84ec-fe566dc70cd9", "8");
pub(crate) const VIDEO_CLIP: XmlRecordDefinition =
    XmlRecordDefinition::new("VideoClip", "9308dbef-2440-4acb-9ab2-953b9a4e82ec", "11");
pub(crate) const CLIP_CHANNEL_GROUP_VECTOR_SERIALIZER: XmlRecordDefinition =
    XmlRecordDefinition::new(
        "ClipChannelGroupVectorSerializer",
        "a3127a8c-95d4-456e-a7f5-171b3f922426",
        "1",
    );
pub(crate) const AUDIO_SEQUENCE_SOURCE: XmlRecordDefinition = XmlRecordDefinition::new(
    "AudioSequenceSource",
    "e8d4cc83-38cb-491f-9d94-e5f7e3b205ee",
    "7",
);
pub(crate) const SECONDARY_CONTENT: XmlRecordDefinition = XmlRecordDefinition::new(
    "SecondaryContent",
    "f9d004b5-cb04-4e2f-af6f-64fadc2c4be9",
    "1",
);
pub(crate) const VIDEO_SEQUENCE_SOURCE: XmlRecordDefinition = XmlRecordDefinition::new(
    "VideoSequenceSource",
    "4752dfa9-7a7e-4a3b-a25b-cafde1a8d036",
    "3",
);
pub(crate) const CLIP_CHANNEL_VECTOR_SERIALIZER: XmlRecordDefinition = XmlRecordDefinition::new(
    "ClipChannelVectorSerializer",
    "333d203b-3a53-4195-8894-fc7523ff3dc7",
    "1",
);
pub(crate) const SEQUENCE: XmlRecordDefinition =
    XmlRecordDefinition::new("Sequence", "6a15d903-8739-11d5-af2d-9b7855ad8974", "12");
pub(crate) const CLIP_CHANNEL_SERIALIZER: XmlRecordDefinition = XmlRecordDefinition::new(
    "ClipChannelSerializer",
    "5c89aa7a-89a6-4483-becd-f2b1def42316",
    "1",
);
pub(crate) const VIDEO_TRACK_GROUP: XmlRecordDefinition = XmlRecordDefinition::new(
    "VideoTrackGroup",
    "9e9abf7a-0918-49c2-91ae-991b5dde77bb",
    "13",
);
pub(crate) const AUDIO_TRACK_GROUP: XmlRecordDefinition = XmlRecordDefinition::new(
    "AudioTrackGroup",
    "9b9238b9-53a8-4cc3-b03f-b36246d052e6",
    "6",
);
pub(crate) const DATA_TRACK_GROUP: XmlRecordDefinition = XmlRecordDefinition::new(
    "DataTrackGroup",
    "b714b71d-6838-48dd-9b77-db19088ced7e",
    "1",
);
pub(crate) const VIDEO_CLIP_TRACK: XmlRecordDefinition = XmlRecordDefinition::new(
    "VideoClipTrack",
    "f68dcd81-8805-11d5-af2d-9bfa89d4ddd4",
    "1",
);
pub(crate) const VIDEO_FILTER_COMPONENT: XmlRecordDefinition = XmlRecordDefinition::new(
    "VideoFilterComponent",
    "d10da199-beea-4dd1-b941-ed3a78766d50",
    "7",
);
/// `Component` element version inside written [`VIDEO_FILTER_COMPONENT`]
/// records, shared by the intrinsic Motion and standard-effect writers. This
/// older generation matches `abstract_slideshow` (Premiere 12.1). AME 2026
/// rendered it for Motion (`premiere_isolated_motion_rotation_linear`);
/// the exported Gaussian Blur records still have no Adobe reopen/render proof.
pub(crate) const VIDEO_FILTER_COMPONENT_BODY_VERSION: &str = "5";
pub(crate) const VIDEO_COMPONENT_PARAM: XmlRecordDefinition = XmlRecordDefinition::new(
    "VideoComponentParam",
    "fe47129e-6c94-4fc0-95d5-c056a517aaf3",
    "9",
);
pub(crate) const TIME_REMAPPING: XmlRecordDefinition =
    XmlRecordDefinition::new("TimeRemapping", "ace5148e-9c9b-40ed-9f82-64cb67308464", "2");
pub(crate) const TIME_COMPONENT_PARAM: XmlRecordDefinition = XmlRecordDefinition::new(
    "TimeComponentParam",
    "278ae1f9-ab7b-4dff-a53c-21029a399a9d",
    "8",
);
pub(crate) const VIDEO_BOOL_COMPONENT_PARAM: XmlRecordDefinition = XmlRecordDefinition::new(
    "VideoComponentParam",
    "cc12343e-f113-4d3b-ae05-b287db77d461",
    "9",
);
pub(crate) const VIDEO_FILTER_AMOUNT_PARAM: XmlRecordDefinition = XmlRecordDefinition::new(
    "VideoComponentParam",
    "a4ff2d6e-7ac2-44f8-9d52-17d9ca50e542",
    "9",
);
// Popup parameter class of standard effects (Gaussian Blur Blur Dimensions).
pub(crate) const VIDEO_POPUP_PARAM: XmlRecordDefinition = XmlRecordDefinition::new(
    "VideoComponentParam",
    "6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8",
    "9",
);
// Colour parameter class of standard effects (Tint Map Black To and Map White
// To), whose value is a u64 of four 16-bit ARGB channels; the corpus Tints
// (Premiere 12.1) save version 9 and Premiere 26.5.1 version 10.
pub(crate) const VIDEO_COLOR_PARAM: XmlRecordDefinition = XmlRecordDefinition::new(
    "VideoComponentParam",
    "0fde4e9f-f895-4ba3-b0fe-9a6feafda583",
    "9",
);
// Video track popup class (`ParameterControlType` 13): the Matte of every
// corpus Track Matte Key, whose value is a track `ID`.
pub(crate) const VIDEO_TRACK_POPUP_PARAM: XmlRecordDefinition = XmlRecordDefinition::new(
    "VideoComponentParam",
    "2f2eb0a3-318c-4a93-99fc-f1d319edc864",
    "9",
);
pub(crate) const POINT_COMPONENT_PARAM: XmlRecordDefinition = XmlRecordDefinition::new(
    "PointComponentParam",
    "ca81d347-309b-44d2-acc7-1c572efb973c",
    "3",
);
pub(crate) const VIDEO_COMPONENT_CHAIN: XmlRecordDefinition = XmlRecordDefinition::new(
    "VideoComponentChain",
    "0970e08a-f58f-4108-b29a-1a717b8e12e2",
    "3",
);
pub(crate) const AUDIO_CLIP_TRACK: XmlRecordDefinition = XmlRecordDefinition::new(
    "AudioClipTrack",
    "097f6203-99ae-11d5-84f2-8cf14bde7040",
    "7",
);
pub(crate) const AUDIO_MIX_TRACK: XmlRecordDefinition =
    XmlRecordDefinition::new("AudioMixTrack", "4b1d8400-e89e-11d5-abc4-a1a13b1e80a0", "4");
pub(crate) const STEREO_TO_STEREO_PAN_PROCESSOR: XmlRecordDefinition = XmlRecordDefinition::new(
    "StereoToStereoPanProcessor",
    "7bf86a01-efbe-11d5-abc4-c1ce2b1e9090",
    "1",
);
pub(crate) const DEFAULT_PAN_PROCESSOR: XmlRecordDefinition = XmlRecordDefinition::new(
    "DefaultPanProcessor",
    "33a94282-ee2c-11d5-abc4-c1cd7f9e3c10",
    "2",
);
pub(crate) const AUDIO_TRACK_INLET: XmlRecordDefinition = XmlRecordDefinition::new(
    "AudioTrackInlet",
    "be3af080-e8c6-11d5-abc4-a1c6d5dee670",
    "4",
);
pub(crate) const AUDIO_FADER: XmlRecordDefinition =
    XmlRecordDefinition::new("AudioFader", "1a38c583-ed5c-11d5-abc4-c1cbf61ec590", "3");
pub(crate) const AUDIO_METER: XmlRecordDefinition =
    XmlRecordDefinition::new("AudioMeter", "72ea4700-f615-11d5-abc4-c186585e63e0", "2");
// Premiere 26.5.1 clip Volume and Channel Volume.
pub(crate) const AUDIO_FILTER_COMPONENT: XmlRecordDefinition = XmlRecordDefinition::new(
    "AudioFilterComponent",
    "d77a90a0-6c9e-44bf-9b20-de8c21168fe1",
    "4",
);
// Both parameter classes use the same XML tag.
pub(crate) const SCALAR_PARAM: XmlRecordDefinition = XmlRecordDefinition::new(
    "AudioComponentParam",
    "a714635e-a628-4b27-9d59-77eba47dbc1a",
    "10",
);
pub(crate) const BOOL_PARAM: XmlRecordDefinition = XmlRecordDefinition::new(
    "AudioComponentParam",
    "32657501-3aa4-445f-a49b-d09ecb9fa1ae",
    "10",
);
pub(crate) const VIDEO_STREAM: XmlRecordDefinition =
    XmlRecordDefinition::new("VideoStream", "a36e4719-3ec6-4a0c-ab11-8b4aab377aa5", "22");
pub(crate) const MEDIA: XmlRecordDefinition =
    XmlRecordDefinition::new("Media", "7a5c103e-f3ac-4391-b6b4-7cc3d2f9a7ff", "30");
pub(crate) const VIDEO_MEDIA_SOURCE: XmlRecordDefinition = XmlRecordDefinition::new(
    "VideoMediaSource",
    "e64ddf74-8fac-4682-8aa8-0e0ca2248949",
    "2",
);
pub(crate) const MARKERS: XmlRecordDefinition =
    XmlRecordDefinition::new("Markers", "bee50706-b524-416c-9f03-b596ce5f6866", "4");
pub(crate) const SUB_CLIP: XmlRecordDefinition =
    XmlRecordDefinition::new("SubClip", "e0c58dc9-dbdd-4166-aef7-5db7e3f22e84", "6");
pub(crate) const VIDEO_CLIP_TRACK_ITEM: XmlRecordDefinition = XmlRecordDefinition::new(
    "VideoClipTrackItem",
    "368b0406-29e3-4923-9fcd-094fbf9a1089",
    "8",
);
pub(crate) const VIDEO_TRANSITION_TRACK_ITEM: XmlRecordDefinition = XmlRecordDefinition::new(
    "VideoTransitionTrackItem",
    "3eeaed31-f78e-4144-b8e8-077656517181",
    "5",
);
pub(crate) const AUDIO_CLIP_TRACK_ITEM: XmlRecordDefinition = XmlRecordDefinition::new(
    "AudioClipTrackItem",
    "064ec682-9ba6-11d5-af2d-9ca32c7d6164",
    "11",
);
pub(crate) const LINK: XmlRecordDefinition =
    XmlRecordDefinition::new("Link", "149d4ea5-a7d4-4b34-9bb7-16d783904bf2", "1");
pub(crate) const AUDIO_STREAM: XmlRecordDefinition =
    XmlRecordDefinition::new("AudioStream", "0b5cf52f-2b85-4863-890b-8844b64ecfe9", "8");
pub(crate) const AUDIO_MEDIA_SOURCE: XmlRecordDefinition = XmlRecordDefinition::new(
    "AudioMediaSource",
    "f588da05-fc2a-4fbc-9383-74d653b379e3",
    "2",
);

pub(crate) const PREMIERE_DATA: &str = "PremiereData";
pub(crate) const OBJECT_ID: &str = "ObjectID";
pub(crate) const OBJECT_UID: &str = "ObjectUID";
pub(crate) const OBJECT_REF: &str = "ObjectRef";
pub(crate) const OBJECT_UREF: &str = "ObjectURef";
pub(crate) const NAME: &str = "Name";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MediaPathField {
    FilePath,
    ActualMediaFilePath,
}

impl MediaPathField {
    pub(crate) fn tag(self) -> &'static str {
        match self {
            Self::FilePath => "FilePath",
            Self::ActualMediaFilePath => "ActualMediaFilePath",
        }
    }
}

// Fixed writer output values shared by several records or too opaque to
// inline.

// Premiere writes track visibility properties only for the first two tracks
// of each kind.
pub(crate) const INITIAL_VISIBLE_TRACK_COUNT: usize = 2;

pub(crate) const BUILD_VERSION: &str = "26.3.0x93 - 20/07/2026 10:07:43";

pub(crate) const VIDEO_MEDIA: &str = "228cda18-3625-4d2d-951e-348879e4ed93";
pub(crate) const AUDIO_MEDIA: &str = "80b8e3d5-6dca-4195-aefb-cb5f407ab009";
pub(crate) const DATA_MEDIA: &str = "d8143ffe-eec4-4d2a-a909-d5f7bf094dc5";
pub(crate) const STEREO: &str = r#"[{"channellabel":100},{"channellabel":101}]"#;
pub(crate) const MONO: &str = r#"[{"channellabel":0}]"#;
/// `StartKeyframe` time of a static, unanimated parameter value.
pub(crate) const STATIC_KEYFRAME_TIME: &str = "-91445760000000000";
pub(crate) const ZERO_GUID: &str = "00000000-0000-0000-0000-000000000000";

pub(crate) const ACTION: &str = "copy";
pub(crate) const AMM_CURRENT_SOLO: &str = "[]";
pub(crate) const MEDIA_CLIP_LABEL_COLOR: &str = "11405886";
pub(crate) const SEQUENCE_CLIP_LABEL_NAME: &str = "BE.Prefs.LabelColors.5";
pub(crate) const MEDIA_CLIP_LABEL_NAME: &str = "BE.Prefs.LabelColors.0";
pub(crate) const AUDIO_TRACK_VERSION: &str = "12";
pub(crate) const BY_GUID: &str = "byGUID";
pub(crate) const CHANNEL_TYPE: &str = "1";
pub(crate) const CLIPS_VERSION: &str = "1";
pub(crate) const CLIP_VERSION: &str = "18";
pub(crate) const CODEC_TYPE: &str = "1635148593";
/// `VideoStream.CodecType` that Premiere 26.5.1 saves for HEVC media: the
/// big-endian `HEVC` code (`oracle/M2/hdr/facts.md`, three Main 10 masters).
pub(crate) const HEVC_CODEC_TYPE: &str = "1212503619";
pub(crate) const COLOR_MANAGEMENT_SETTINGS: &str =
    r#"{"autoToneMapEnabled":true,"enableLogColorManagement":2,"lutInterpolationMethod":1}"#;
pub(crate) const PROJECT_COLOR_MANAGEMENT_SETTINGS: &str =
    r#"{"enableLogColorManagement":2,"graphicsWhiteLuminance":203,"lutInterpolationMethod":1}"#;
pub(crate) const IMMERSIVE_VIDEO_VR_CONFIGURATION: &str = r#"{"ambisonicsHRIR":"","ambisonicsMonitoringType":0,"capturedHorizontalView":360,"capturedVerticalView":180,"fieldOfHorizontalView":108,"fieldOfVerticalView":108,"projectionType":0,"stereoscopicEye":0,"stereoscopicType":0,"version":3}"#;
pub(crate) const PIXEL_ASPECT_RATIO: &str = "1,1";

/// A native `PixelAspectRatio`: `num,den`, two positive integers. Premiere
/// saves square pixels as `1,1` and also as an equal pair such as `1920,1920`
/// or `1000000,1000000`; the writer emits [`PIXEL_ASPECT_RATIO`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PixelAspectRatio {
    numerator: u64,
    denominator: u64,
}

impl PixelAspectRatio {
    /// The `PixelAspectRatio` `value` of the record `identity`.
    pub(crate) fn parse(value: &str, identity: &str) -> crate::error::Result<Self> {
        let invalid = || {
            crate::error::unsupported(format!(
                "{identity}: invalid PixelAspectRatio {value:?}; expected two positive integers num,den"
            ))
        };
        let positive = |field: &str| match field.parse::<u64>() {
            // `parse` alone also accepts a leading `+`.
            Ok(value) if value > 0 && field.bytes().all(|byte| byte.is_ascii_digit()) => Ok(value),
            _ => Err(invalid()),
        };
        let (numerator, denominator) = value.split_once(',').ok_or_else(invalid)?;
        Ok(Self {
            numerator: positive(numerator)?,
            denominator: positive(denominator)?,
        })
    }

    /// Whether the pixels are square, which they are when both integers are
    /// equal.
    pub(crate) fn is_square(self) -> bool {
        self.numerator == self.denominator
    }
}

impl std::fmt::Display for PixelAspectRatio {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}:{}", self.numerator, self.denominator)
    }
}

pub(crate) const SEQUENCE_ITEM_LABEL_NAME: &str = "BE.Prefs.LabelColors.5";
pub(crate) const MEDIA_ITEM_LABEL_NAME: &str = "BE.Prefs.LabelColors.0";
pub(crate) const COMPONENT_CHAIN_VERSION: &str = "3";
pub(crate) const COMPONENT_OWNER_VERSION: &str = "1";
pub(crate) const CONTENT_VERSION: &str = "10";
pub(crate) const ENCODING: &str = "base64";
// One Premiere tick interval at the scaffold's 48 kHz audio sample rate.
pub(crate) const AUDIO_TICKS_PER_SAMPLE: &str = "5292000";
pub(crate) const ROOT_BIN_ID: &str = "1000000";
pub(crate) const IN_USE: &str = "false";
pub(crate) const MEDIA_IMPLEMENTATION_ID: &str = "1fa18bfa-255c-44b1-ad73-56bcd99fceaf";
pub(crate) const MZ_PROJECT_APPLICATION_ID: &str = "Pro";
pub(crate) const MZ_PROJECT_WORKSPACE_NAME: &str = "Learning";
pub(crate) const MZ_SEQUENCE_EDITING_MODE_GUID: &str = "9678af98-a7b7-4bdb-b477-7ac9c8df4a4e";
pub(crate) const MZ_SEQUENCE_PREVIEW_RENDERING_CLASS_ID: &str = "1061109567";
pub(crate) const MZ_SEQUENCE_PREVIEW_RENDERING_PRESET_CODEC: &str = "1634755443";
pub(crate) const MZ_SEQUENCE_PREVIEW_RENDERING_PRESET_PATH: &str =
    r"EncoderPresets\SequencePreview\9678af98-a7b7-4bdb-b477-7ac9c8df4a4e\QuickTime.epr";
pub(crate) const BALANCE_NAME: &str = "Balance";
pub(crate) const BYPASS_NAME: &str = "Bypass";
pub(crate) const LEVEL_NAME: &str = "Level";
pub(crate) const MUTE_NAME: &str = "Mute";
pub(crate) const CHANNEL_VOLUME_MATCH_NAME: &str = "Internal Channel Volume Stereo";
/// Channel Volume parameter names after `Bypass`; projects up to Premiere 13
/// end both with a space.
pub(crate) const CHANNEL_VOLUME_NAMES: [&str; 2] = ["Left", "Right"];
pub(crate) const LEGACY_CHANNEL_VOLUME_NAMES: [&str; 2] = ["Left ", "Right "];
/// Unnamed parameters that follow Left and Right in a Premiere 26.5.1
/// Channel Volume.
pub(crate) const CHANNEL_VOLUME_EXTRA_PARAMS: usize = 30;
pub(crate) const ROOT_BIN_NAME: &str = "Root Bin";
pub(crate) const VOLUME_NAME: &str = "Volume";
pub(crate) const NEXT_ID: &str = "1000001";
pub(crate) const NEXT_PANNER_ID: &str = "4294967279";
pub(crate) const NODE_VERSION: &str = "1";
pub(crate) const PAN_PROCESSOR_VERSION: &str = "3";
pub(crate) const PREVIEW_FORMAT_IDENTIFIER: &str = "fc3cd4d9-d839-8259-9276-05c5000000ea";
pub(crate) const PROJECT_ITEM_VERSION: &str = "1";
pub(crate) const PROPERTIES_VERSION: &str = "1";
pub(crate) const RANGE_LOCKED: &str = "false";
pub(crate) const SCRATCH_DISK_SAME_AS_PROJECT: &str = "SameAsProject";
pub(crate) const TL_SQAV_DIVIDER_POSITION: &str = "0.5";
pub(crate) const TL_SQ_TIME_PER_PIXEL: &str = "0.90634441087613293";
pub(crate) const TL_SQ_TRACK_EXPANDED: &str = "0";
pub(crate) const TL_SQ_TRACK_EXPANDED_HEIGHT: &str = "41";
pub(crate) const TRACKS_VERSION: &str = "1";
pub(crate) const TRACK_GROUP_VERSION: &str = "1";
pub(crate) const TRACK_VERSION: &str = "4";
pub(crate) const UNITS_STRING: &str = "dB";
pub(crate) const UPPER_BOUND: &str = "5.6234130859375";

#[cfg(test)]
mod tests {
    use super::PixelAspectRatio;

    #[test]
    fn pixel_aspect_ratio_is_square_only_for_an_equal_pair_of_positive_integers() {
        let parse = |value: &str| PixelAspectRatio::parse(value, "VideoStream:1");
        for square in ["1,1", "1280,1280", "1920,1920", "1000000,1000000"] {
            assert!(parse(square).unwrap().is_square(), "{square}");
        }
        // HDV 1440x1080 in a 1920 frame, DVCPRO HD 1280x1080, a doubled width.
        for (anamorphic, shown) in [("1920,1440", "1920:1440"), ("3,2", "3:2"), ("2,1", "2:1")] {
            let ratio = parse(anamorphic).unwrap();
            assert!(!ratio.is_square(), "{anamorphic}");
            assert_eq!(ratio.to_string(), shown);
        }
        for malformed in [
            "",
            "1",
            "1,",
            ",1",
            "0,0",
            "1,0",
            "-1,-1",
            "+1,+1",
            " 1,1",
            "1.0,1.0",
            "1,1,1",
            "a,b",
            "18446744073709551616,1",
        ] {
            let error = parse(malformed).unwrap_err().to_string();
            assert!(
                error.contains("VideoStream:1: invalid PixelAspectRatio"),
                "{malformed:?}: {error}"
            );
        }
    }
}
