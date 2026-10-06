//! Tesseract-to-Premiere validation and safe package publication.

use crate::{
    audio_media::{inspect_audio_media, SourceSound},
    convert::{active_asset_id, embedded_sound_asset, video_data},
    error::{unsupported, BuildError, Result},
    format::{FrameRate, PrProjectFile, PremiereProjectXml},
    hash::{hash, hash_reader},
    image_media::{inspect_export_image_media, ImageFormat},
    media::{admitted_container, unsupported_media_reason, MediaFacts},
    publication::publish_file,
    schema::records::MediaPathField,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    path::{Path, PathBuf},
};
use tesseract_file::TesseractFile;

mod losses;
pub(crate) mod prepared;

fn output_is_fresh(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
        Ok(_) => Err(unsupported(
            "output package already exists; choose a fresh directory",
        )),
    }
}

fn absolute_output_path(output: &Path, current_directory: &Path) -> Result<PathBuf> {
    let name = output
        .file_name()
        .filter(|name| !name.is_empty())
        .ok_or_else(|| unsupported("output package needs a directory name"))?;
    let raw_parent = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent = if raw_parent.is_absolute() {
        raw_parent.to_owned()
    } else {
        current_directory.join(raw_parent)
    }
    .canonicalize()?;
    if !parent.is_dir() {
        return Err(unsupported("output package parent is not a directory"));
    }
    let output = parent.join(name);
    if output.to_str().is_none() {
        return Err(unsupported("output path must be UTF-8"));
    }
    Ok(output)
}

/// Convert and validate the current document as a `frame_rate` sequence, then
/// optionally publish its package.
#[cfg(test)]
pub(crate) fn save_tesseract_as_premiere(
    source: &Path,
    output: &Path,
    frame_rate: FrameRate,
    check: bool,
) -> Result<fx_conv::ConversionReport<crate::Omission>> {
    save_tesseract_as_premiere_with_progress(
        source,
        output,
        frame_rate,
        check,
        fx_conv::Progress::default(),
    )
}

pub(crate) fn save_tesseract_as_premiere_with_progress(
    source: &Path,
    output: &Path,
    frame_rate: FrameRate,
    check: bool,
    progress: fx_conv::Progress<'_>,
) -> Result<fx_conv::ConversionReport<crate::Omission>> {
    let source = source.canonicalize()?;
    if source.to_str().is_none() {
        return Err(unsupported("input path must be UTF-8"));
    }
    let output = absolute_output_path(output, Path::new("."))?;
    output_is_fresh(&output)?;
    let source_hash = hash(&source)?;
    let file = TesseractFile::open(&source)?;
    let prepared = prepared::prepare_with_progress(
        &file,
        file.project(),
        &crate::PremiereExportOptions {
            frame_rate: Some(frame_rate),
        },
        progress,
    )?;
    progress.stage("serializing and validating Premiere package");
    let (mut project, omissions) = prepared.into_native()?;
    bind_media(&mut project, &file, &output)?;

    // The XML is temporary. Check drops it; execution writes these exact bytes.
    let xml = PremiereProjectXml::new(&project)?;
    if hash(&source)? != source_hash {
        return Err(unsupported(
            "source changed while converting the Premiere project",
        ));
    }
    if !check {
        progress.stage("writing and publishing Premiere package");
        publish_package(&source, &source_hash, &file, &output, &project, &xml)?;
    }
    let artifacts = std::iter::once(fx_conv::Artifact::project("project.prproj"))
        .chain(
            project
                .media
                .values()
                .filter(|media| !media.is_generator())
                .map(|media| fx_conv::Artifact::media(Path::new("media").join(&media.name))),
        )
        .collect();
    Ok(fx_conv::ConversionReport {
        diagnostics: omissions,
        artifacts,
    })
}

fn sequence(project: &PrProjectFile) -> Result<&crate::format::PrSequence> {
    project
        .single_sequence()
        .ok_or_else(|| unsupported("Premiere conversion requires exactly one sequence"))
}

/// Inspects each asset that `document`, the baked form of the archive's
/// project, exports once, and checks its bytes against the archive digest.
/// Media that export omits is skipped: video that `exported_video_layers`
/// leaves out, and stills that `exported_image_layers` leaves out, in nests too.
fn inspect_media(
    file: &TesseractFile,
    document: &fx_schema::EditableFxCompositionDocument,
) -> Result<BTreeMap<String, MediaFacts>> {
    let mut media = BTreeMap::new();
    let composition = document.composition();
    let dimensions = document.dimensions();
    let canvas = [dimensions.width, dimensions.height];
    let layers =
        crate::convert::exported_video_layers(composition.layers(), composition.dynamics(), canvas)
            .into_iter()
            .chain(crate::convert::exported_image_layers(
                composition.layers(),
                composition.dynamics(),
                canvas,
            ));
    for layer in layers {
        let video = video_data(layer)?;
        let asset_id = match (&video, layer.data()) {
            // Export packages the replacement footage that the renderer shows.
            (Some(video), _) => active_asset_id(&video.source).as_str(),
            (None, fx_schema::LayerData::Image(image)) => {
                let fx_schema::ImageSource::Asset(source) = &image.source;
                source.asset_id.as_str()
            }
            (None, _) => continue,
        };
        if media.contains_key(asset_id) {
            continue;
        }
        // Bound inspection before conversion rejects an oversized document.
        crate::format::PrSequence::validate_occurrence_count(media.len() + 1)?;
        let facts = (|| -> Result<_> {
            let asset = file.asset(asset_id)?;
            let facts = if matches!(layer.data(), fx_schema::LayerData::Image(_)) {
                if asset.descriptor().kind != tesseract_file::AssetKind::Image {
                    return Err(unsupported("packaged still has conflicting asset kind"));
                }
                // A semantic-loss outcome must never conceal a damaged entry.
                if hash_reader(asset.open()?)? != asset.descriptor().sha256 {
                    return Err(unsupported("packaged media bytes failed their source hash"));
                }
                let facts =
                    inspect_export_image_media(asset.open()?, asset.descriptor().byte_length)?;
                let extension = Path::new(&asset.descriptor().path)
                    .extension()
                    .and_then(|value| value.to_str());
                match &facts {
                    MediaFacts::Still(image) => {
                        if extension.and_then(ImageFormat::from_extension) != Some(image.format) {
                            return Err(unsupported(
                                "packaged still file extension does not match its image data",
                            ));
                        }
                        if !image
                            .format
                            .matches_content_type(&asset.descriptor().content_type)
                        {
                            return Err(unsupported(
                                "packaged still content type does not match its image data",
                            ));
                        }
                    }
                    MediaFacts::UnsupportedStill(_) => {
                        if extension.and_then(ImageFormat::from_extension)
                            != Some(ImageFormat::OpenExr)
                            || !ImageFormat::OpenExr
                                .matches_content_type(&asset.descriptor().content_type)
                        {
                            return Err(unsupported(
                                "packaged EXR declaration does not match its image data",
                            ));
                        }
                    }
                    _ => {
                        return Err(unsupported(
                            "image inspection returned conflicting media kind",
                        ))
                    }
                }
                facts
            } else {
                // Name the container before reading bytes, as import does.
                crate::video_format::validate_video_file_name(Path::new(&asset.descriptor().path))?;
                let container =
                    crate::media::MediaContainer::from_path(Path::new(&asset.descriptor().path))
                        .ok_or_else(|| unsupported("unsupported packaged video container"))?;
                if asset.descriptor().kind != container.asset_kind()
                    || asset.descriptor().content_type != container.content_type()
                {
                    return Err(unsupported(
                        "packaged video kind or content type conflicts with its container",
                    ));
                }
                crate::media::inspect_export_video_media(
                    asset.open()?,
                    asset.open()?,
                    asset.descriptor().byte_length,
                )?
            };
            if hash_reader(asset.open()?)? != asset.descriptor().sha256 {
                return Err(unsupported("packaged media bytes failed their source hash"));
            }
            Ok(facts)
        })()
        .map_err(|source| BuildError::Context {
            context: format!(
                "asset {asset_id:?}, layer {} ({:?})",
                layer.id(),
                layer.name()
            ),
            source: Box::new(source),
        })?;
        media.insert(asset_id.to_owned(), facts);
    }
    Ok(media)
}

/// Inspects the sound of each asset that an audio layer of the baked
/// `document` plays (`AudioSource::active_asset_id`), at the top level or in
/// a nest (`exported_audio_layers`), or that the audible video of a top-level
/// clip that export writes plays (`exported_clip_videos`, `embedded_sound_asset`).
/// A picture source without sound has no entry. Sound that conversion does not
/// support is recorded for the converter to report; failing to read an asset or
/// verify its bytes stops the export.
fn inspect_audio(
    file: &TesseractFile,
    document: &fx_schema::EditableFxCompositionDocument,
    video: &BTreeMap<String, MediaFacts>,
) -> Result<BTreeMap<String, SourceSound>> {
    let mut audio = BTreeMap::new();
    let mut inspected = BTreeSet::new();
    let composition = document.composition();
    let dimensions = document.dimensions();
    let canvas = [dimensions.width, dimensions.height];
    let clips =
        crate::convert::exported_clip_videos(composition.layers(), composition.dynamics(), canvas);
    let sounds =
        crate::convert::exported_audio_layers(composition.layers(), composition.dynamics(), canvas);
    // Keep root embedded sound independent of picture admission, while using
    // admitted stage clocks and recursive independent-audio traversal.
    let pictures = composition
        .layers()
        .iter()
        .filter(|layer| !matches!(layer.data(), fx_schema::LayerData::Audio(_)))
        .map(|layer| clips.get(&layer.id()).copied().unwrap_or(layer));
    for layer in sounds.chain(pictures) {
        let picture = video_data(layer)?;
        let asset_id = match layer.data() {
            // An enabled enhancement plays its output; the inactive asset is
            // never read, and the active one has no fallback.
            fx_schema::LayerData::Audio(sound) => sound.source.active_asset_id().as_str(),
            _ => match picture
                .as_deref()
                .and_then(|picture| embedded_sound_asset(picture, composition.dynamics()))
            {
                Some(asset_id) => asset_id,
                None => continue,
            },
        };
        if !inspected.insert(asset_id.to_owned()) {
            continue;
        }
        // Bound inspection before conversion rejects an oversized document.
        crate::format::PrSequence::validate_occurrence_count(inspected.len())?;
        let facts = (|| -> Result<_> {
            let asset = file.asset(asset_id)?;
            // Video inspection already checked the bytes of a picture source.
            // Check the others first, so a damaged entry never reads as
            // unsupported sound.
            if !video.contains_key(asset_id)
                && hash_reader(asset.open()?)? != asset.descriptor().sha256
            {
                return Err(unsupported("packaged media bytes failed their source hash"));
            }
            let extension = Path::new(&asset.descriptor().path)
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or_default();
            // An audio-only use of a movie still has the picture Premiere
            // measures; a file without a supported picture keeps the exact rule.
            let picture = || match video.get(asset_id) {
                Some(facts) => crate::audio_media::PictureClock::of(facts),
                None => crate::media::inspect_video_media(
                    asset.open().ok()?,
                    asset.open().ok()?,
                    asset.descriptor().byte_length,
                )
                .ok()
                .and_then(|video| crate::audio_media::PictureClock::of(&MediaFacts::Video(video))),
            };
            match inspect_audio_media(asset.open()?, asset.descriptor().byte_length, extension) {
                Ok(Some(facts)) => Ok(Some(
                    match crate::audio_media::padded_to_picture(&facts, picture())? {
                        Some(picture_ticks) => SourceSound::PaddedToPicture {
                            file_ticks: facts.intrinsic_ticks,
                            stream: crate::schema::PrAudioStream {
                                prepared_clock: None,
                                intrinsic_ticks: picture_ticks,
                                ..facts
                            },
                        },
                        None => SourceSound::Supported(facts),
                    },
                )),
                Ok(None) => Ok(None),
                Err(error) => unsupported_media_reason(error)
                    .map(|reason| Some(SourceSound::Unsupported(reason))),
            }
        })()
        .map_err(|source| BuildError::Context {
            context: format!(
                "asset {asset_id:?}, layer {} ({:?})",
                layer.id(),
                layer.name()
            ),
            source: Box::new(source),
        })?;
        if let Some(facts) = facts {
            audio.insert(asset_id.to_owned(), facts);
        }
    }
    Ok(audio)
}

// Distinct assets may share a source basename. Bind one portable package
// path per media while preserving payload and occurrence identity.
fn bind_media(project: &mut PrProjectFile, file: &TesseractFile, output: &Path) -> Result<()> {
    let order: Vec<_> = sequence(project)?
        .media_in_order()
        .into_iter()
        .cloned()
        .collect();
    let mut used = BTreeSet::new();
    for id in order {
        // Generator media has no packaged file to bind.
        if project.media[&id].is_generator() {
            continue;
        }
        let asset_id = id.as_str();
        let descriptor = file
            .metadata()
            .assets
            .get(asset_id)
            .ok_or_else(|| unsupported(format!("asset {asset_id:?} is not packaged")))?;
        let original = Path::new(&descriptor.path)
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| unsupported("packaged video must have a UTF-8 name"))?;
        let extension = Path::new(original)
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        let media = &project.media[&id];
        let packaged = admitted_container(media, Path::new(original)).is_some_and(|container| {
            descriptor.kind == container.asset_kind()
                && container.matches_content_type(&descriptor.content_type)
        });
        if !packaged {
            return Err(unsupported(if media.is_still() {
                "writer requires a packaged PNG, JPEG, or OpenEXR image asset matching its native kind and content type"
            } else {
                "writer requires packaged media with a matching kind, extension, and content type"
            }));
        }
        let mut chosen = original.to_owned();
        let mut suffix = 1;
        // ASCII names avoid filesystem-dependent Unicode normalization collisions.
        while !chosen.is_ascii() || used.contains(&chosen.to_lowercase()) {
            chosen = format!("premiere-media-{suffix:03}.{extension}");
            suffix += 1;
        }
        used.insert(chosen.to_lowercase());
        let media = project
            .media
            .get_mut(&id)
            .expect("sequence media was validated");
        let relative = format!("./media/{chosen}");
        let absolute = output.join("media").join(&chosen);
        media.name = chosen;
        media.relative_path = Some(relative.clone());
        media.relative_paths = vec![relative];
        media.absolute_paths = vec![
            (MediaPathField::ActualMediaFilePath, absolute.clone()),
            (MediaPathField::FilePath, absolute),
        ];
    }
    Ok(())
}

fn copy_native_media(
    file: &TesseractFile,
    directory: &Path,
    project: &PrProjectFile,
) -> Result<()> {
    copy_native_media_except(file, directory, project, &BTreeSet::new())
}

fn copy_native_media_except(
    file: &TesseractFile,
    directory: &Path,
    project: &PrProjectFile,
    foreign_media: &BTreeSet<crate::format::MediaId>,
) -> Result<()> {
    fs::create_dir(directory.join("media"))?;
    for (asset_id, media) in &project.media {
        if media.is_generator() || foreign_media.contains(asset_id) {
            continue;
        }
        let asset = file.asset(asset_id.as_str())?;
        let target = directory.join("media").join(&media.name);
        let mut output_file = File::create_new(&target)?;
        std::io::copy(&mut asset.open()?, &mut output_file)?;
        output_file.sync_all()?;
        // Inspection verified the archive entry against this digest.
        if hash(&target)? != asset.descriptor().sha256 {
            return Err(unsupported(
                "packaged media changed after converting the Premiere project",
            ));
        }
    }
    Ok(())
}

fn publish_package(
    source: &Path,
    source_hash: &str,
    file: &TesseractFile,
    output: &Path,
    project: &PrProjectFile,
    xml: &PremiereProjectXml,
) -> Result<()> {
    output_is_fresh(output)?;
    if hash(source)? != source_hash {
        return Err(unsupported(
            "source changed after converting the Premiere project",
        ));
    }
    let parent = output
        .parent()
        .ok_or_else(|| unsupported("output parent missing"))?;
    let temporary = tempfile::Builder::new()
        .prefix(".conversion-premiere-")
        .tempdir_in(parent)?;
    copy_native_media(file, temporary.path(), project)?;
    xml.write_new(&temporary.path().join("project.prproj"))?;
    if hash(source)? != source_hash {
        return Err(unsupported("source changed during the Premiere build"));
    }
    publish(
        temporary.path(),
        output,
        project
            .media
            .values()
            .filter(|media| !media.is_generator())
            .map(|media| &media.name),
    )
}

// create_dir reserves the destination; publish_file never replaces an existing entry.
// Remove only files/directories created here on failure, never recursively delete.
fn publish<'a>(
    temporary: &Path,
    output: &Path,
    names: impl Iterator<Item = &'a String>,
) -> Result<()> {
    fs::create_dir(output)?;
    let media = output.join("media");
    let mut files = Vec::new();
    let mut created_media = false;
    let result = (|| -> std::result::Result<(), std::io::Error> {
        fs::create_dir(&media)?;
        created_media = true;
        for name in names {
            let target = media.join(name);
            publish_file(&temporary.join("media").join(name), &target, fs::hard_link)?;
            files.push(target);
        }
        let target = output.join("project.prproj");
        publish_file(&temporary.join("project.prproj"), &target, fs::hard_link)?;
        files.push(target);
        Ok(())
    })();
    if let Err(error) = result {
        for file in files.iter().rev() {
            let _ = fs::remove_file(file);
        }
        if created_media {
            let _ = fs::remove_dir(&media);
        }
        let _ = fs::remove_dir(output);
        return Err(error.into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publication_failure_removes_only_files_already_linked() {
        let root = tempfile::tempdir().unwrap();
        let staged = root.path().join("staged");
        fs::create_dir_all(staged.join("media")).unwrap();
        fs::write(staged.join("media/first.mp4"), b"original").unwrap();
        let output = root.path().join("out");
        let names = ["first.mp4".to_owned(), "missing.mp4".to_owned()];
        let result = publish(&staged, &output, names.iter());
        assert!(
            matches!(result, Err(crate::error::BuildError::Io(ref error)) if error.kind() == std::io::ErrorKind::NotFound)
        );
        assert!(!output.exists());
        assert_eq!(
            fs::read(staged.join("media/first.mp4")).unwrap(),
            b"original"
        );
    }

    #[test]
    fn bare_and_dot_relative_outputs_resolve_to_one_absolute_destination() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        assert_eq!(
            absolute_output_path(Path::new("out"), &root).unwrap(),
            root.join("out")
        );
        assert_eq!(
            absolute_output_path(Path::new("./out"), &root).unwrap(),
            root.join("out")
        );
    }

    #[test]
    fn publication_preserves_a_destination_created_after_validation() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("existing");
        fs::create_dir(&output).unwrap();
        fs::write(output.join("keep"), b"unrelated").unwrap();
        assert!(publish(dir.path(), &output, std::iter::empty()).is_err());
        assert_eq!(fs::read(output.join("keep")).unwrap(), b"unrelated");
    }
}
