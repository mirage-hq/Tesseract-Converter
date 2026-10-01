//! Premiere schema, graph, reader, and writer.

/// Failure to decode, validate, or encode a Premiere project.
#[derive(Debug, thiserror::Error)]
pub(crate) enum FormatError {
    #[error("invalid Premiere project: {0}")]
    Invalid(String),
    #[error("Premiere project I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid Premiere XML: {0}")]
    Xml(#[from] roxmltree::Error),
    #[error("{record}: invalid Premiere XML shape: {source}")]
    Decode {
        record: String,
        #[source]
        source: quick_xml::DeError,
    },
    #[error("Premiere XML encoding failed: {0}")]
    Encode(#[from] quick_xml::SeError),
    #[error("Premiere XML is not UTF-8: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),
    #[error("Premiere project allocation failed while building {context}: {source}")]
    Allocation {
        context: &'static str,
        #[source]
        source: std::collections::TryReserveError,
    },
}

pub(crate) type Result<T> = std::result::Result<T, FormatError>;

pub(crate) fn invalid(message: impl Into<String>) -> FormatError {
    FormatError::Invalid(message.into())
}

macro_rules! ensure_valid {
    ($condition:expr, $($message:tt)*) => {
        if !$condition { return Err($crate::format::invalid(format!($($message)*))); }
    };
}

pub(crate) use ensure_valid;

mod graph;
mod reader;
pub(crate) mod shape_payload;
mod text_payload;
mod writer;

pub use crate::schema::{
    FrameRate, MediaId, PrGraphic, PrMedia, PrProjectFile, PrSequence, PrVideoItem,
    PrVideoOccurrence,
};
use graph::{cyclic_sequences, sequences, Graph, Located, Record};
#[cfg(test)]
pub(crate) use reader::read_xml;
pub(crate) use writer::PremiereProjectXml;

#[cfg(test)]
pub(crate) fn inspect_project(
    xml: &str,
    sequence: Option<&str>,
) -> crate::error::Result<PrSequence> {
    Ok(inspect_project_with_media(xml, sequence)?
        .into_parts()
        .0
        .remove(0))
}

#[cfg(test)]
pub(crate) fn inspect_project_with_media(
    xml: &str,
    selection: Option<&str>,
) -> crate::error::Result<PrProjectFile> {
    Ok(inspect_project_with_omissions(xml, selection)?.0)
}

#[cfg(test)]
pub(crate) fn inspect_project_with_omissions(
    xml: &str,
    selection: Option<&str>,
) -> crate::error::Result<(PrProjectFile, Vec<crate::Omission>)> {
    let mut media = std::collections::BTreeMap::new();
    let mut omissions = Vec::new();
    let graph = Graph::parse(xml)?;
    // Loading reads with the same topology; its own reports stay out of this list.
    let cyclic = sequences(&graph, &mut Vec::new())
        .map(|topology| cyclic_sequences(&topology))
        .unwrap_or_default();
    let sequence = reader::read_sequence(&graph, selection, &cyclic, &mut media, &mut omissions)?;
    // Omitted occurrences may have loaded media that the surviving sequence no longer uses.
    let referenced: std::collections::BTreeSet<_> =
        sequence.media_in_order().into_iter().cloned().collect();
    media.retain(|id, _| referenced.contains(id));
    let project = PrProjectFile::from_sequences(vec![sequence], media);
    project.validate()?;
    Ok((project, omissions))
}

#[cfg(test)]
mod tests;
