//! The linked After Effects compositions of one sequence.
//!
//! Each linked AEP is read once and bound to the bytes whose hash media
//! resolution verified; each linked media record then selects its
//! composition by the native Dynamic Link GUID in that exact file, never by a
//! name. Every video placement imports the composition's editable picture
//! with its own FX identities, through the existing After Effects import and
//! media preflight. The media of all pictures is packaged under one asset
//! namespace per resolved AEP and stays valid while this value lives.
//!
//! A caller-supplied importer ([`LinkedCompositionResolver`]) replaces that
//! import: it converts each placement's composition itself, and its assets are
//! packaged as it returns them. Its pictures take the same placement, canvas,
//! duration and omission rules.

use crate::{
    error::{ensure, unsupported, BuildError, Result},
    format::{MediaId, PrMedia},
    linked_import::{LinkedComposition, LinkedCompositionResolver},
    schema::PrAfterEffectsComposition,
};
use aftereffects_file::{
    AepConversionError, AfterEffects, DynamicLinkImportError, LinkedMedia, LinkedPicture,
    LinkedPictureTarget, PreparedAfterEffectsImport,
};
use fx_schema::{AssetId, Duration, Layer, LayerData, LayerId, Time};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use tesseract_file::{AssetKind, TesseractFileBuilder};

/// Assets that a caller's importer returned, packaged as it returned them.
type SuppliedAssets = Vec<(AssetId, PathBuf, AssetKind)>;

/// The linked compositions of one sequence and the media of their pictures.
#[derive(Default)]
pub(crate) struct LinkedCompositions<'a, 'm> {
    /// Each resolved AEP once, in first-appearance order. Its position names
    /// its asset namespace, so equal item IDs of two AEPs never share an asset.
    sources: Vec<LinkedSource>,
    /// The source, native composition and Premiere canvas of each linked
    /// media record.
    links: BTreeMap<MediaId, Link>,
    media: LinkedMedia,
    /// The caller's importer, which converts every placement's composition
    /// in place of the built-in import.
    resolver: Option<&'a mut LinkedCompositionResolver<'a>>,
    /// Outer Premiere-scoped substitutions, applied after each AEP resolves its originals.
    media_map: Option<&'m fx_conv::ValidatedMediaMap>,
    /// The assets of the caller's pictures.
    supplied_assets: SuppliedAssets,
}

#[derive(Clone, Copy)]
struct Link {
    source: usize,
    composition: PrAfterEffectsComposition,
    /// The canvas that Premiere links the composition as.
    canvas: [u32; 2],
}

struct LinkedSource {
    path: PathBuf,
    /// The prepared AEP, or why it supplies no linked composition; `None`
    /// when the caller's importer reads it.
    prepared: Option<std::result::Result<PreparedAfterEffectsImport, String>>,
}

impl<'a, 'm> LinkedCompositions<'a, 'm> {
    /// The linked compositions of a sequence whose pictures `resolver`
    /// imports, or the built-in import when there is none.
    pub(crate) fn new(
        resolver: Option<&'a mut LinkedCompositionResolver<'a>>,
        media_map: Option<&'m fx_conv::ValidatedMediaMap>,
    ) -> Self {
        Self {
            resolver,
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
                let prepared = match self.resolver {
                    Some(_) => None,
                    None => Some(prepare(path, hash)?),
                };
                self.sources.push(LinkedSource {
                    path: path.to_owned(),
                    prepared,
                });
                self.sources.len() - 1
            }
        };
        if let Some(prepared) = &self.sources[index].prepared {
            let prepared = match prepared {
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
        }
        self.links.insert(
            id.clone(),
            Link {
                source: index,
                composition,
                canvas,
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
        let prepared = match &self.sources[link.source].prepared {
            Some(Ok(prepared)) => prepared,
            Some(Err(error)) => {
                return Err(unsupported(format!(
                    "linked source preparation failed: {error}"
                )))
            }
            None => {
                return Err(unsupported(
                    "caller-supplied linked import cannot provide native media inspection",
                ))
            }
        };
        let resolved = prepared
            .resolve_composition(&link.composition.guid_bytes())
            .map_err(|error| unsupported(format!("resolved link {media} changed: {error}")))?;
        Ok(resolved.inspect_media(media_map)?)
    }

    /// Imports the editable picture of one placement of the linked `media`,
    /// which shows the composition up to `source_end`, as a child of
    /// `parent`, with identities from `first_id` onward. `Ok(Err(reason))`
    /// is a composition that forms no editable picture of the placement,
    /// which omits it; its media is not packaged.
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
            Some(Ok(prepared)) => prepared,
            Some(Err(error)) => {
                return Ok(Err(format!("linked source preparation failed: {error}")))
            }
            None => return self.import_supplied(link, parent, first_id, source_end),
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

    /// [`Self::picture`] of `link` from the caller's importer.
    fn import_supplied(
        &mut self,
        link: Link,
        parent: LayerId,
        first_id: u64,
        source_end: Time,
    ) -> Result<std::result::Result<LinkedPicture, String>> {
        let path = &self.sources[link.source].path;
        let guid = link.composition.dynamic_link_guid();
        let resolver = self
            .resolver
            .as_mut()
            .ok_or_else(|| unsupported("linked AEP importer was not supplied"))?;
        let supplied = match resolver(path, link.composition, first_id) {
            Ok(supplied) => supplied,
            Err(error) if is_unsupported_import(&error) => {
                return Ok(Err(format!(
                    "linked After Effects composition GUID {guid} in {path:?} forms no editable picture: {error:#}"
                )));
            }
            Err(error) => return Err(BuildError::LinkedImport(error)),
        };
        let subject = format!("the linked composition supplied for GUID {guid}");
        let bounds = SuppliedBounds {
            path,
            canvas: link.canvas,
            source_end,
        };
        Ok(
            match supplied_picture(supplied, &subject, bounds, parent, first_id)? {
                Ok((picture, assets)) => {
                    self.supplied_assets.extend(assets);
                    Ok(picture)
                }
                Err(reason) => Err(reason),
            },
        )
    }

    /// Packages the media of every imported picture into `builder`: the
    /// built-in pictures' media, returned because it must outlive the
    /// archive written from it, and the caller's assets as it returned them.
    pub(crate) fn package(
        self,
        builder: TesseractFileBuilder,
    ) -> Result<(TesseractFileBuilder, LinkedMedia)> {
        let mut builder = self.media.add_to(builder)?;
        for (id, path, kind) in self.supplied_assets {
            builder = builder.add_asset(id.as_str(), path, kind)?;
        }
        Ok((builder, self.media))
    }
}

/// Reads the AEP at `path` once, bound to the bytes that media resolution
/// hashed as `hash`. `Ok(Err(reason))` is an AEP with no supported linked
/// compositions.
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

/// Whether `error`, from importing a composition, is source content that
/// forms no editable picture, which omits its placements, rather than an
/// operational or integrity failure, which stops the conversion.
fn is_unsupported(error: &AepConversionError) -> bool {
    matches!(
        error,
        AepConversionError::Read(_) | AepConversionError::Document(_)
    )
}

/// Whether `error`, from preparing or selecting a linked composition, is an
/// unsupported profile, identity or source ([`is_unsupported`]).
fn is_unsupported_link(error: &DynamicLinkImportError) -> bool {
    match error {
        DynamicLinkImportError::Input(error) => is_unsupported(error),
        DynamicLinkImportError::UnsupportedProfile { .. }
        | DynamicLinkImportError::UnsupportedGuid
        | DynamicLinkImportError::MissingComposition(_) => true,
    }
}

/// Whether a caller's importer failed on unsupported source content, by the
/// rules of the built-in import. Any other failure is operational.
fn is_unsupported_import(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<DynamicLinkImportError>()
        .map(is_unsupported_link)
        .or_else(|| {
            error
                .downcast_ref::<AepConversionError>()
                .map(is_unsupported)
        })
        .unwrap_or(false)
}

/// Why `subject`, a linked composition of `canvas` in the AEP at `path`, is
/// omitted when Premiere links it as `premiere`: its placement geometry would
/// change.
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

/// Why a placement that shows `subject`, a linked composition of `duration`,
/// up to `source_end` is omitted: the composition ends earlier. Premiere's
/// tick rounding leaves one millisecond.
fn short_source(subject: &str, source_end: Time, duration: Duration) -> Option<String> {
    (source_end.as_millis() > duration.as_millis().saturating_add(1)).then(|| {
        format!(
            "{subject} lasts {} ms, shorter than the source range to {} ms that the clip shows",
            duration.as_millis(),
            source_end.as_millis()
        )
    })
}

/// What a placement requires of the content that a caller supplies for it.
#[derive(Clone, Copy)]
struct SuppliedBounds<'p> {
    /// The AEP that the caller imported the content from.
    path: &'p Path,
    /// The canvas that Premiere links the composition as.
    canvas: [u32; 2],
    /// The latest source time that the placement shows.
    source_end: Time,
}

/// The picture of `supplied`, `subject`'s content for one placement with
/// identities from `first_id`, as a child of `parent`, and its assets.
/// `Ok(Err(reason))` is content outside the placement's `bounds`; content
/// that breaks the importer's contract is an error.
fn supplied_picture(
    supplied: LinkedComposition,
    subject: &str,
    bounds: SuppliedBounds<'_>,
    parent: LayerId,
    first_id: u64,
) -> Result<std::result::Result<(LinkedPicture, SuppliedAssets), String>> {
    ensure!(
        supplied.next_id > first_id,
        "linked importer did not advance its ID range"
    );
    let dimensions = supplied.document.dimensions();
    let canvas = [dimensions.width, dimensions.height];
    if let Some(reason) = canvas_mismatch(subject, bounds.path, canvas, bounds.canvas) {
        return Ok(Err(reason));
    }
    if let Some(reason) = short_source(subject, bounds.source_end, supplied.document.duration()) {
        return Ok(Err(reason));
    }
    let composition = supplied.document.composition();
    let [root] = composition.layers() else {
        return Err(unsupported(
            "linked importer must return one composition group",
        ));
    };
    let LayerData::Group(root) = root.data() else {
        return Err(unsupported(
            "linked composition root is not editable group content",
        ));
    };
    let mut root = root.clone();
    root.parent = Some(parent);
    let picture = LinkedPicture {
        root: Layer::from_data(&LayerData::Group(root))?,
        animations: composition.dynamics().entries().to_vec(),
        motion_blur: composition.motion_blur(),
        next_id: supplied.next_id,
        diagnostics: Vec::new(),
    };
    Ok(Ok((picture, supplied.assets)))
}
