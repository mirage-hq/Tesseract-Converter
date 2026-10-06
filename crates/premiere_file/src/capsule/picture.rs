//! Capsule-native picture snapshots and publication resources; no GUIDs/replay.

use super::*;
use aftereffects_file::graphic_template::GraphicPicture;
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::Write,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tesseract_file::{TesseractFile, TesseractFileBuilder};

/// A qualified container and its instance-only native template values.
/// Normalized media stays owned here until host publication finishes.
#[derive(Debug)]
pub(crate) struct PictureSource {
    container: PathBuf,
    hash: String,
    staging: tempfile::TempDir,
    media_context: PathBuf,
    template: SavedGraphicTemplate,
    composition: u32,
    text: Vec<SavedGraphicText>,
    pictures: Mutex<Vec<GraphicPicture>>,
}

impl PictureSource {
    pub(crate) fn prepare(
        container: PathBuf,
        file: File,
        saved: &SavedCapsule,
    ) -> Result<(Arc<Self>, Vec<String>), CapsuleError> {
        let hash = source_hash(&container)?;
        let (source, diagnostics) = prepare_archive(file, false, &container, &hash, saved)?;
        source.verify()?;
        Ok((Arc::new(source), diagnostics))
    }

    pub(crate) fn import(
        &self,
        first_id: u64,
        namespace: &str,
    ) -> Result<GraphicPicture, CapsuleError> {
        self.verify()?;
        with_archive(
            File::open(&self.container)?,
            false,
            |zip, index, graphic| {
                if graphic {
                    let data = read_member(zip, index)?;
                    with_archive(Cursor::new(data), true, |zip, index, graphic| {
                        if graphic {
                            return Err(invalid("recursive aegraphic containers are unsupported"));
                        }
                        self.import_from_archive(zip, index, first_id, namespace)
                    })
                } else {
                    self.import_from_archive(zip, index, first_id, namespace)
                }
            },
        )
    }

    fn import_from_archive<R: Read + Seek>(
        &self,
        zip: &mut zip::ZipArchive<R>,
        index: usize,
        first_id: u64,
        namespace: &str,
    ) -> Result<GraphicPicture, CapsuleError> {
        let aep_name = zip
            .by_index_raw(index)?
            .enclosed_name()
            .ok_or_else(|| invalid("unsafe selected AEP member"))?;
        let expected = self
            .media_context
            .strip_prefix(self.staging.path())
            .map_err(|_| invalid("selected AEP staging path escaped its owner"))?;
        if aep_name != expected {
            return Err(invalid(
                "saved Capsule selected AEP member changed during import",
            ));
        }
        let base = aep_name.parent().unwrap_or(Path::new("")).to_owned();
        let mut staged = BTreeSet::new();
        let mut stage = |relative: &Path| -> std::io::Result<()> {
            if !staged.insert(relative.to_owned()) {
                return Ok(());
            }
            stage_collected_member(zip, self.staging.path(), &base, relative)
        };
        Ok(self.template.import_editable_picture_with_collected_media(
            &self.media_context,
            self.composition,
            first_id,
            namespace,
            &self.text,
            &mut stage,
        )?)
    }

    pub(crate) fn keep_picture(&self, picture: GraphicPicture) {
        self.pictures
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push(picture);
    }

    pub(crate) fn verify(&self) -> Result<(), CapsuleError> {
        if source_hash(&self.container)? != self.hash {
            return Err(invalid(
                "saved Capsule container changed during import/publication",
            ));
        }
        for picture in self
            .pictures
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .iter()
        {
            picture
                .verify_sources()
                .map_err(GraphicTemplateError::from)?;
        }
        Ok(())
    }

    pub(crate) fn package(
        &self,
        mut builder: TesseractFileBuilder,
    ) -> Result<TesseractFileBuilder, crate::error::BuildError> {
        for picture in self
            .pictures
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .iter()
        {
            for (id, path, kind) in &picture.assets {
                builder = builder.add_asset(id.as_str(), path, *kind)?;
            }
        }
        Ok(builder)
    }

    pub(crate) fn verify_packaged(&self, archive: &TesseractFile) -> Result<(), CapsuleError> {
        for picture in self
            .pictures
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .iter()
        {
            picture
                .verify_packaged(archive)
                .map_err(GraphicTemplateError::from)?;
        }
        self.verify()
    }
}

fn source_hash(path: &Path) -> Result<String, CapsuleError> {
    let mut reader = File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn stage_collected_member<R: Read + Seek>(
    zip: &mut zip::ZipArchive<R>,
    staging: &Path,
    base: &Path,
    relative: &Path,
) -> std::io::Result<()> {
    if relative
        .components()
        .any(|component| !matches!(component, std::path::Component::Normal(_)))
        || relative.components().next()
            != Some(std::path::Component::Normal(std::ffi::OsStr::new(
                "(Footage)",
            )))
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "unsafe native collected media path",
        ));
    }
    let member_path = base.join(relative);
    let name = member_path
        .to_str()
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "non-UTF8 collected media path",
            )
        })?
        .replace('\\', "/");
    let mut member = match zip.by_name(&name) {
        Ok(member) => member,
        Err(zip::result::ZipError::FileNotFound) => return Ok(()),
        Err(error) => return Err(std::io::Error::other(error)),
    };
    if member.size() > MAX_EXPANDED_MEMBER_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "consumed Capsule media exceeds 64 MiB expanded member",
        ));
    }
    let destination = staging.join(&member_path);
    match std::fs::symlink_metadata(&destination) {
        Ok(metadata) if metadata.file_type().is_file() => {
            std::fs::remove_file(&destination)?;
        }
        Ok(_) => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "collected media staging destination is not a regular file",
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut output = File::options()
        .create_new(true)
        .write(true)
        .open(&destination)?;
    let count = match std::io::copy(
        &mut member.by_ref().take(MAX_EXPANDED_MEMBER_BYTES + 1),
        &mut output,
    ) {
        Ok(count) => count,
        Err(error) => {
            drop(output);
            let _ = std::fs::remove_file(&destination);
            return Err(error);
        }
    };
    if count > MAX_EXPANDED_MEMBER_BYTES {
        drop(output);
        let _ = std::fs::remove_file(&destination);
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "consumed Capsule media exceeds actual expansion bound",
        ));
    }
    Ok(())
}

fn prepare_archive<R: Read + Seek>(
    reader: R,
    nested: bool,
    container: &Path,
    hash: &str,
    saved: &SavedCapsule,
) -> Result<(PictureSource, Vec<String>), CapsuleError> {
    with_archive(reader, nested, |zip, index, graphic| {
        let data = read_member(zip, index)?;
        if graphic {
            return prepare_archive(Cursor::new(data), true, container, hash, saved);
        }
        let aep_name = zip
            .by_index_raw(index)?
            .enclosed_name()
            .ok_or_else(|| invalid("unsafe selected AEP member"))?;
        let template = SavedGraphicTemplate::decode(&data)?;
        let (template, composition, mut text, mut diagnostics) =
            saved.resolve_instance(&template)?;
        // Several declared UUIDs must not choose conflicting values for one
        // source property. Keep that template property, not an invented winner.
        let mut targets = BTreeSet::new();
        let mut duplicate = BTreeSet::new();
        for value in &text {
            let target = (value.composition_id, value.layer_id);
            if !targets.insert(target) {
                duplicate.insert(target);
            }
        }
        text.retain(|value| {
            if duplicate.contains(&(value.composition_id, value.layer_id)) {
                diagnostics.push(format!("controller {} has multiple saved overrides for one native Text target; template Text retained", value.controller_uuid));
                false
            } else { true }
        });
        let staging = tempfile::tempdir()?;
        let media_context = staging.path().join(&aep_name);
        if let Some(parent) = media_context.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut aep = File::options()
            .create_new(true)
            .write(true)
            .open(&media_context)?;
        aep.write_all(&data)?;
        drop(aep);
        Ok((
            PictureSource {
                container: container.to_owned(),
                hash: hash.to_owned(),
                staging,
                media_context,
                template,
                composition,
                text,
                pictures: Mutex::new(Vec::new()),
            },
            diagnostics,
        ))
    })
}

#[cfg(test)]
mod tests;

/// Owns every Capsule source reachable from the selected timeline, including nests.
#[derive(Debug, Default)]
pub(crate) struct PictureResources(pub(crate) Vec<Arc<PictureSource>>);

impl PictureResources {
    pub(crate) fn collect(sequence: &crate::schema::PrSequence) -> Self {
        fn visit(
            sequence: &crate::schema::PrSequence,
            seen: &mut BTreeSet<usize>,
            sources: &mut Vec<Arc<PictureSource>>,
        ) {
            for track in &sequence.video_tracks {
                for item in &track.items {
                    if let crate::schema::PrVideoItem::Capsule(capsule) = item {
                        if seen.insert(Arc::as_ptr(&capsule.source) as usize) {
                            sources.push(Arc::clone(&capsule.source));
                        }
                    }
                }
                for nest in &track.nests {
                    visit(&nest.sequence, seen, sources);
                }
            }
        }
        let mut sources = Vec::new();
        visit(sequence, &mut BTreeSet::new(), &mut sources);
        Self(sources)
    }

    pub(crate) fn package(
        &self,
        mut builder: TesseractFileBuilder,
    ) -> Result<TesseractFileBuilder, crate::error::BuildError> {
        for source in &self.0 {
            builder = source.package(builder)?;
        }
        Ok(builder)
    }
    pub(crate) fn verify(&self) -> Result<(), CapsuleError> {
        for source in &self.0 {
            source.verify()?;
        }
        Ok(())
    }
    pub(crate) fn verify_packaged(&self, archive: &TesseractFile) -> Result<(), CapsuleError> {
        for source in &self.0 {
            source.verify_packaged(archive)?;
        }
        Ok(())
    }
}
