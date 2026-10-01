use fx_schema::animator::AnimationGraphError;

/// Failure to build a canonical editable composition while importing Premiere.
#[derive(Debug, thiserror::Error)]
pub(crate) enum EditableBuildError {
    #[error("invalid input: {field} {reason}")]
    InvalidInput {
        field: &'static str,
        reason: &'static str,
    },
    #[error("invalid input: {field} {reason}")]
    AnimationGraph {
        field: &'static str,
        reason: &'static str,
        #[source]
        source: Box<AnimationGraphError>,
    },
    #[error(transparent)]
    Composition(#[from] fx_schema::ValidationError),
}

impl EditableBuildError {
    pub(crate) const fn invalid_input(field: &'static str, reason: &'static str) -> Self {
        Self::InvalidInput { field, reason }
    }
}

/// Failure to create the former private project-mutation host.
#[derive(Debug, thiserror::Error)]
pub(crate) enum CreationError {
    #[error("invalid input: video_metadata.display_dimensions width and height must be non-zero")]
    MissingDimensions,
}

/// Failure to inspect, validate, or publish a conversion in either direction.
#[derive(Debug, thiserror::Error)]
pub(crate) enum BuildError {
    #[error("linked composition import failed: {0}")]
    LinkedImport(#[source] anyhow::Error),
    #[error("{context}: {source}")]
    Context {
        context: String,
        #[source]
        source: Box<BuildError>,
    },
    #[error("unsupported conversion: {0}")]
    Unsupported(String),
    #[error("missing media: {0}")]
    MissingMedia(String),
    #[error(transparent)]
    Premiere(#[from] crate::format::FormatError),
    #[error("invalid Tesseract package: {0}")]
    Tesseract(#[from] tesseract_file::TesseractFileError),
    #[error("linked After Effects import failed: {0}")]
    AfterEffects(#[from] aftereffects_file::AepConversionError),
    #[error("conversion I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("{context}: {source}")]
    IoAt {
        context: String,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid native number: {0}")]
    Number(#[from] std::num::ParseIntError),
    #[error("invalid conversion JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid media map: {0}")]
    MediaMap(#[from] fx_conv::MediaMapError),
    #[error("invalid editable document: {0}")]
    Document(#[from] fx_schema::EditableFxDocumentError),
    #[error("could not build editable document: {0}")]
    Mutation(#[from] EditableBuildError),
    #[error("could not create project mutation host: {0}")]
    Creation(#[from] CreationError),
    #[error("invalid MP4 media: {0}")]
    Mp4(#[from] media_transcode::inspect::InspectError),
    #[error("invalid audio media: {0}")]
    Audio(#[from] symphonia::core::errors::Error),
    #[error("{0} path must be UTF-8")]
    Path(&'static str),
}

pub(crate) type Result<T> = std::result::Result<T, BuildError>;
pub(crate) fn unsupported(message: impl Into<String>) -> BuildError {
    BuildError::Unsupported(message.into())
}
macro_rules! ensure {
    ($condition:expr, $($message:tt)*) => {
        if !$condition { return Err($crate::error::unsupported(format!($($message)*))); }
    };
}
pub(crate) use ensure;
