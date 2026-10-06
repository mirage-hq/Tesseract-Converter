//! Font transport is byte-preserving; After Effects does not activate package fonts.

use std::{collections::BTreeSet, fs, path::Path};

use sha2::{Digest, Sha256};

use crate::{
    adapter::AepConversionError, export_document::fonts::ArchiveFonts, writer::text::FontFormat,
};

pub(in crate::adapter::export) fn prepare(
    fonts: &ArchiveFonts,
    names: &BTreeSet<String>,
    directory: &Path,
) -> Result<Vec<String>, AepConversionError> {
    let mut files = BTreeSet::new();
    let mut manifest = Vec::new();
    for (face, bytes, format) in fonts.used_faces(names) {
        let sha256 = format!("{:x}", Sha256::digest(bytes));
        let extension = if bytes.starts_with(b"ttcf") {
            "ttc"
        } else {
            match format {
                FontFormat::TrueType => "ttf",
                FontFormat::Cff => "otf",
            }
        };
        let file = format!("fonts/{sha256}.{extension}");
        if files.insert(file.clone()) {
            let path = directory.join(&file);
            fs::create_dir_all(directory.join("fonts")).map_err(|error| {
                AepConversionError::io("create font package directory", &path, error)
            })?;
            fs::write(&path, bytes)
                .map_err(|error| AepConversionError::io("write packaged font", &path, error))?;
        }
        manifest.push(serde_json::json!({
            "file": file,
            "family": face.family_name,
            "style": face.style_name,
            "postscript_name": face.postscript_name,
            "sha256": sha256,
        }));
    }
    if !files.is_empty() {
        let path = directory.join("fonts/manifest.json");
        let file = fs::File::create(&path)
            .map_err(|error| AepConversionError::io("create font manifest", &path, error))?;
        serde_json::to_writer_pretty(file, &manifest).map_err(|error| {
            AepConversionError::io("write font manifest", &path, std::io::Error::other(error))
        })?;
        files.insert("fonts/manifest.json".into());
    }
    Ok(files.into_iter().collect())
}
