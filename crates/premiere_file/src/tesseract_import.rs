use crate::{
    error::{ensure, unsupported, BuildError, Result},
    format::PrProjectFile,
    hash::hash,
    publication::publish_file,
    tesseract_output::{
        convert_premiere_sequence_with_media_map, convert_premiere_sequence_with_progress,
        PendingTesseractFile,
    },
    Omission,
};
use fx_conv::{Progress, ValidatedMediaMap};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

const PROJECT_FILENAME: &str = "project.tsrct";

/// One validated Premiere timeline and its publication data.
#[derive(Debug)]
pub(crate) struct TesseractImport {
    input: PathBuf,
    destination: PathBuf,
    original_hash: String,
    target: String,
    project: PendingTesseractFile,
    media_relink: Option<crate::ValidatedMediaRelink>,
    pub(crate) omissions: Vec<Omission>,
}

impl TesseractImport {
    /// Convert one selected sequence with the built-in linked-composition import.
    #[cfg(test)]
    pub(crate) fn convert(input: &Path, output: &Path, selection: Option<&str>) -> Result<Self> {
        Self::convert_with_progress(input, output, selection, Progress::default())
    }

    #[cfg(test)]
    pub(crate) fn convert_with_progress(
        input: &Path,
        output: &Path,
        selection: Option<&str>,
        progress: Progress<'_>,
    ) -> Result<Self> {
        Self::convert_with_options(
            input,
            output,
            &crate::PremiereImportOptions {
                sequence: selection.map(str::to_owned),
            },
            false,
            None,
            None,
            progress,
        )
    }

    #[cfg(all(test, feature = "ffmpeg-library"))]
    pub(crate) fn convert_with_media_relink(
        input: &Path,
        output: &Path,
        selection: Option<&str>,
        relink: &crate::ValidatedMediaRelink,
        progress: Progress<'_>,
    ) -> Result<Self> {
        Self::convert_with_media_relink_and_map(input, output, selection, relink, None, progress)
    }

    #[cfg(all(test, feature = "ffmpeg-library"))]
    pub(crate) fn convert_with_media_relink_and_map(
        input: &Path,
        output: &Path,
        selection: Option<&str>,
        relink: &crate::ValidatedMediaRelink,
        media_map: Option<&ValidatedMediaMap>,
        progress: Progress<'_>,
    ) -> Result<Self> {
        Self::convert_with_options(
            input,
            output,
            &crate::PremiereImportOptions {
                sequence: selection.map(str::to_owned),
            },
            false,
            media_map,
            Some(relink),
            progress,
        )
    }

    pub(crate) fn convert_with_options(
        input: &Path,
        output: &Path,
        options: &crate::PremiereImportOptions,
        allow_film_impact_pop: bool,
        media_map: Option<&ValidatedMediaMap>,
        media_relink: Option<&crate::ValidatedMediaRelink>,
        progress: Progress<'_>,
    ) -> Result<Self> {
        ensure!(
            !output.exists() && !output.is_symlink(),
            "output already exists; choose a fresh directory"
        );
        let name = output
            .file_name()
            .ok_or_else(|| unsupported("output needs a directory name"))?;
        let parent = output
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .canonicalize()?;
        ensure!(parent.is_dir(), "output parent is not a directory");
        let destination = parent.join(name);
        ensure!(destination.to_str().is_some(), "output path must be UTF-8");
        let input = input.canonicalize()?;
        ensure!(input.to_str().is_some(), "input path must be UTF-8");

        // Verify one source identity after validation and again immediately
        // before publishing the staged archive.
        let original_hash = hash(&input)?;
        progress.stage("reading Premiere project");
        let (project, mut omissions) = PrProjectFile::load_import_with_media_relink(
            &input,
            options.sequence.as_deref(),
            media_relink,
        )?;
        let (mut sequences, media) = project.into_parts();
        let mut sequence = sequences
            .pop()
            .ok_or_else(|| unsupported("selected timeline is unavailable"))?;
        ensure!(
            sequences.is_empty(),
            "Premiere import must resolve exactly one sequence"
        );
        let sequence_name = sequence.name.clone();
        let target = sequence.id.clone().unwrap_or_default();
        if let Some(media_map) = media_map {
            ensure!(
                !target.is_empty(),
                "prepared media requires a stable sequence ID"
            );
            media_map.validate_for(&input, "premiere", &target)?;
        }
        // Inspect native uses before editable conversion can omit them. Only
        // safely omitted source content may bypass the video admission gate;
        // path, identity, I/O and malformed-media failures remain fatal.
        let native_preflight = crate::tesseract_output::inspect_native_premiere_media_for_import(
            &input,
            &sequence,
            media_map,
            media_relink,
        )?;
        crate::tesseract_output::require_import_video_admission(&native_preflight, &input)?;
        if !allow_film_impact_pop {
            crate::convert::omit_pop_emulation(&mut sequence, &mut omissions);
        }
        let converted = if let Some(media_map) = media_map {
            convert_premiere_sequence_with_media_map(
                &input,
                sequence,
                Arc::new(media),
                &mut omissions,
                media_map,
                progress,
            )
        } else {
            convert_premiere_sequence_with_progress(
                &input,
                sequence,
                Arc::new(media),
                &mut omissions,
                progress,
            )
        };
        let project = converted
            .map_err(|source| BuildError::Context {
                context: format!("timeline {sequence_name:?}; no output published"),
                source: Box::new(source),
            })?
            .ok_or_else(|| {
                unsupported(format!(
                    "selected timeline is not convertible; no output published:\n{}",
                    omissions
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join("\n")
                ))
            })?;
        ensure!(
            hash(&input)? == original_hash,
            "source changed during import conversion"
        );
        if let Some(media_map) = media_map {
            media_map.validate_for(&input, "premiere", &target)?;
        }
        if let Some(relink) = media_relink {
            relink.validate_for(&input, &target)?;
        }
        Ok(Self {
            input,
            destination,
            original_hash,
            target,
            project,
            media_relink: media_relink.cloned(),
            omissions,
        })
    }

    pub(crate) fn artifacts(&self) -> Vec<fx_conv::Artifact> {
        vec![fx_conv::Artifact::project(PROJECT_FILENAME)]
    }

    pub(crate) fn write(self) -> Result<()> {
        self.write_with_validation(None)
    }

    pub(crate) fn write_with_media_map(self, media_map: &ValidatedMediaMap) -> Result<()> {
        self.write_with_validation(Some(media_map))
    }

    fn write_with_validation(self, media_map: Option<&ValidatedMediaMap>) -> Result<()> {
        let parent = self
            .destination
            .parent()
            .ok_or_else(|| unsupported("output parent missing"))?;
        let staged = tempfile::Builder::new()
            .prefix(".conversion-tesseract-")
            .tempdir_in(parent)?;
        let staged_project = staged.path().join(PROJECT_FILENAME);
        self.project
            .write_to_staging(&staged_project)
            .map_err(|source| BuildError::Context {
                context: format!("project {PROJECT_FILENAME:?}; no output published"),
                source: Box::new(source),
            })?;
        ensure!(
            hash(&self.input)? == self.original_hash,
            "source changed during the Tesseract import build"
        );
        if let Some(media_map) = media_map {
            media_map.validate_for(&self.input, "premiere", &self.target)?;
        }
        if let Some(relink) = &self.media_relink {
            relink.validate_for(&self.input, &self.target)?;
        }
        publish_project(&staged_project, &self.destination, |source, target| {
            fs::hard_link(source, target)
        })
    }
}

fn publish_project(
    staged: &Path,
    destination: &Path,
    link: impl FnOnce(&Path, &Path) -> std::io::Result<()>,
) -> Result<()> {
    fs::create_dir(destination).map_err(|source| BuildError::IoAt {
        context: "output already exists or cannot be created".into(),
        source,
    })?;
    let target = destination.join(PROJECT_FILENAME);
    if let Err(error) = publish_file(staged, &target, link) {
        // publish_file removes its own partial copy. A colliding target
        // belongs to someone else and must not be deleted here.
        let _ = fs::remove_dir(destination);
        return Err(BuildError::Context {
            context: "could not publish Tesseract import".into(),
            source: Box::new(error.into()),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn publication_failure_does_not_delete_a_foreign_collision() {
        let root = tempfile::tempdir().unwrap();
        let staged = root.path().join("staged");
        let destination = root.path().join("output");
        std::fs::write(&staged, b"converted").unwrap();
        let result = super::publish_project(&staged, &destination, |_, target| {
            std::fs::write(target, b"foreign").unwrap();
            Err(std::io::ErrorKind::AlreadyExists.into())
        });
        assert!(result.is_err());
        assert_eq!(
            std::fs::read(destination.join(super::PROJECT_FILENAME)).unwrap(),
            b"foreign"
        );
    }
}
