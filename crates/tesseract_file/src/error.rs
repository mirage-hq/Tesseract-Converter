use std::path::PathBuf;

/// Errors produced while reading or writing a `.tsrct` file.
#[derive(Debug, thiserror::Error)]
pub enum TesseractFileError {
    #[error("failed to access {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid ZIP archive: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[cfg(target_arch = "wasm32")]
    #[error("invalid ZIP archive: {0}")]
    AsyncZip(#[from] async_zip::error::ZipError),
    #[error("invalid metadata.json: {0}")]
    MetadataJson(#[from] serde_json::Error),
    #[error("invalid .tsrct file: {0}")]
    Invalid(String),
    #[error("invalid editable project.json: {0}")]
    EditableFxDocument(#[from] fx_schema::EditableFxDocumentError),
}

pub(crate) trait IoContext<T> {
    fn at(self, path: impl Into<PathBuf>) -> Result<T, TesseractFileError>;
}

impl<T> IoContext<T> for Result<T, std::io::Error> {
    fn at(self, path: impl Into<PathBuf>) -> Result<T, TesseractFileError> {
        self.map_err(|source| TesseractFileError::Io {
            path: path.into(),
            source,
        })
    }
}
