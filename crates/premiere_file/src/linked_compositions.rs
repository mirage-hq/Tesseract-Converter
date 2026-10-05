//! The linked After Effects compositions of one sequence.
//!
//! Each linked AEP is read once and bound to the bytes whose hash media
//! resolution verified; each linked media record then selects its
//! composition by the native Dynamic Link GUID in that exact file, never by a
//! name. Every video placement imports the composition's editable picture
//! with its own FX identities, through the existing After Effects import and
//! media preflight. The media of all pictures is packaged under one asset
//! namespace per resolved AEP and stays valid while this value lives.

use crate::{
    error::{ensure, unsupported, Result},
    format::{MediaId, PrMedia},
    schema::PrAfterEffectsComposition,
};
use aftereffects_file::{
    AepConversionError, AfterEffects, DynamicLinkImportError, LinkedAudio, LinkedMedia,
    LinkedPicture, LinkedPictureTarget, PreparedAfterEffectsImport,
};
use fx_schema::{Duration, LayerId, Time};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use tesseract_file::TesseractFileBuilder;

/// The linked compositions of one sequence and the media of their pictures.
#[derive(Default)]
pub(crate) struct LinkedCompositions<'m> {
    /// Each resolved AEP once, in first-appearance order. Its position names
    /// its asset namespace, so equal item IDs of two AEPs never share an asset.
    sources: Vec<LinkedSource>,
    links: BTreeMap<MediaId, Link>,
    media: LinkedMedia,
    /// Outer Premiere-scoped substitutions, applied after each AEP resolves its originals.
    media_map: Option<&'m fx_conv::ValidatedMediaMap>,
}

#[derive(Clone, Copy)]
struct Link {
    source: usize,
    composition: PrAfterEffectsComposition,
}

struct LinkedSource {
    path: PathBuf,
    /// The prepared AEP, or why it supplies no linked composition.
    prepared: std::result::Result<PreparedAfterEffectsImport, String>,
}

impl<'m> LinkedCompositions<'m> {
    pub(crate) fn new(media_map: Option<&'m fx_conv::ValidatedMediaMap>) -> Self {
        Self {
            media_map,
            ..Self::default()
        }
    }

    /// Selects the composition of the linked `media` record `id` in the AEP
    /// at `path`, whose bytes have SHA-256 `hash`. `Ok(Err(reason))` is an
    /// unsupported link, which omits the record's placements; an I/O or
    /// integrity failure is an error.
    pub(crate) fn link(
        &mut self,
        id: &MediaId,
        media: &PrMedia,
        path: &Path,
        hash: &str,
    ) -> Result<std::result::Result<(), String>> {
        let (Some(composition), Some(video)) = (media.after_effects_composition(), &media.video)
        else {
            return Err(unsupported(format!(
                "{id}: media is not a linked After Effects composition"
            )));
        };
        let canvas = [video.width, video.height];
        let index = match self.sources.iter().position(|source| source.path == path) {
            Some(index) => index,
            None => {
                self.sources.push(LinkedSource {
                    path: path.to_owned(),
                    prepared: prepare(path, hash)?,
                });
                self.sources.len() - 1
            }
        };
        let prepared = match &self.sources[index].prepared {
            Ok(prepared) => prepared,
            Err(reason) => return Ok(Err(reason.clone())),
        };
        let guid = composition.dynamic_link_guid();
        let resolved = match prepared.resolve_composition(&composition.guid_bytes()) {
            Ok(resolved) => resolved,
            Err(error) => {
                return Ok(Err(format!(
                    "linked After Effects project {path:?} has no composition for Dynamic Link GUID {guid}: {error}; the composition is never chosen by name"
                )))
            }
        };
        let subject = format!(
            "linked After Effects composition {} ({:?}, GUID {guid})",
            resolved.composition_id(),
            resolved.name()
        );
        if let Some(reason) = canvas_mismatch(&subject, path, resolved.dimensions(), canvas) {
            return Ok(Err(reason));
        }
        self.links.insert(
            id.clone(),
            Link {
                source: index,
                composition,
            },
        );
        Ok(Ok(()))
    }

    /// Whether `media` names a resolved composition.
    pub(crate) fn contains(&self, media: &MediaId) -> bool {
        self.links.contains_key(media)
    }

    /// Inspects the exact Dynamic Link composition selected by `media`.
    pub(crate) fn inspect_media(
        &self,
        media: &MediaId,
        media_map: Option<&fx_conv::ValidatedMediaMap>,
    ) -> Result<fx_conv::MediaPreflight> {
        let link = *self
            .links
            .get(media)
            .ok_or_else(|| unsupported(format!("{media} names no resolved linked composition")))?;
        let prepared = self.sources[link.source]
            .prepared
            .as_ref()
            .map_err(|error| unsupported(format!("linked source preparation failed: {error}")))?;
        let resolved = prepared
            .resolve_composition(&link.composition.guid_bytes())
            .map_err(|error| unsupported(format!("resolved link {media} changed: {error}")))?;
        Ok(resolved.inspect_media(media_map)?)
    }

    /// Imports the editable picture of one placement of the linked `media`.
    /// `Ok(Err(reason))` omits an unsupported picture without packaging its media.
    pub(crate) fn picture(
        &mut self,
        media: &MediaId,
        parent: LayerId,
        first_id: u64,
        source_end: Time,
    ) -> Result<std::result::Result<LinkedPicture, String>> {
        let link = *self
            .links
            .get(media)
            .ok_or_else(|| unsupported(format!("{media} names no resolved linked composition")))?;
        let prepared = match &self.sources[link.source].prepared {
            Ok(prepared) => prepared,
            Err(error) => return Ok(Err(format!("linked source preparation failed: {error}"))),
        };
        let resolved = prepared
            .resolve_composition(&link.composition.guid_bytes())
            .map_err(|error| unsupported(format!("resolved link {media} changed: {error}")))?;
        let subject = format!(
            "linked After Effects composition {} ({:?})",
            resolved.composition_id(),
            resolved.name()
        );
        if let Some(reason) = short_source(&subject, source_end, resolved.duration()) {
            return Ok(Err(reason));
        }
        let asset_namespace = format!("premiere-aep-{}", link.source + 1);
        let target = LinkedPictureTarget {
            parent,
            first_id,
            asset_namespace: &asset_namespace,
        };
        match resolved.import_picture_with_media_map(target, &mut self.media, self.media_map) {
            Ok(picture) => Ok(Ok(picture)),
            Err(error) if is_unsupported(&error) => {
                Ok(Err(format!("{subject} forms no editable picture: {error}")))
            }
            Err(error) => Err(error.into()),
        }
    }

    /// Resolve the same composition for an independent audio occurrence.
    pub(crate) fn audio(
        &mut self,
        media: &MediaId,
        parent: LayerId,
        first_id: u64,
        source_end: Time,
    ) -> Result<std::result::Result<LinkedAudio, String>> {
        let link = *self
            .links
            .get(media)
            .ok_or_else(|| unsupported(format!("{media} names no resolved linked composition")))?;
        let prepared = match &self.sources[link.source].prepared {
            Ok(prepared) => prepared,
            Err(error) => return Ok(Err(format!("linked source preparation failed: {error}"))),
        };
        let resolved = prepared
            .resolve_composition(&link.composition.guid_bytes())
            .map_err(|error| unsupported(format!("resolved link {media} changed: {error}")))?;
        let subject = format!(
            "linked After Effects composition {} ({:?})",
            resolved.composition_id(),
            resolved.name()
        );
        if let Some(reason) = short_source(&subject, source_end, resolved.duration()) {
            return Ok(Err(reason));
        }
        let asset_namespace = format!("premiere-aep-{}", link.source + 1);
        match resolved.import_audio_with_media_map(
            parent,
            first_id,
            &asset_namespace,
            &mut self.media,
            self.media_map,
        ) {
            Ok(audio) => Ok(Ok(audio)),
            Err(error) if is_unsupported(&error) => {
                Ok(Err(format!("{subject} forms no editable sound: {error}")))
            }
            Err(error) => Err(error.into()),
        }
    }

    /// Packages media and returns its owner, which must outlive archive writing.
    pub(crate) fn package(
        self,
        builder: TesseractFileBuilder,
    ) -> Result<(TesseractFileBuilder, LinkedMedia)> {
        let builder = self.media.add_to(builder)?;
        Ok((builder, self.media))
    }
}

/// Reads the AEP once, bound to the bytes that media resolution hashed.
fn prepare(
    path: &Path,
    hash: &str,
) -> Result<std::result::Result<PreparedAfterEffectsImport, String>> {
    let prepared = match AfterEffects.prepare_linked_import(path) {
        Ok(prepared) => prepared,
        // The file was just hashed, so a failure to read it is operational.
        Err(DynamicLinkImportError::Input(error)) if !is_unsupported(&error) => {
            return Err(error.into())
        }
        Err(error) => {
            return Ok(Err(format!(
                "linked After Effects project {path:?} is not supported: {error}"
            )))
        }
    };
    let prepared_hash: String = prepared
        .source_sha256()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    ensure!(
        prepared_hash == hash,
        "linked After Effects project {path:?} changed while it was inspected"
    );
    Ok(Ok(prepared))
}

/// Unsupported content omits a placement; operational failures stop conversion.
fn is_unsupported(error: &AepConversionError) -> bool {
    matches!(
        error,
        AepConversionError::Read(_) | AepConversionError::Document(_)
    )
}

fn canvas_mismatch(
    subject: &str,
    path: &Path,
    canvas: [u32; 2],
    premiere: [u32; 2],
) -> Option<String> {
    (canvas != premiere).then(|| {
        format!(
            "{subject} is {}x{} in {path:?}, but Premiere links it as {}x{}; placement geometry would change",
            canvas[0], canvas[1], premiere[0], premiere[1]
        )
    })
}

/// Premiere's tick rounding leaves one millisecond at a composition's end.
fn short_source(subject: &str, source_end: Time, duration: Duration) -> Option<String> {
    (source_end.as_millis() > duration.as_millis().saturating_add(1)).then(|| {
        format!(
            "{subject} lasts {} ms, shorter than the source range to {} ms that the clip shows",
            duration.as_millis(),
            source_end.as_millis()
        )
    })
}
