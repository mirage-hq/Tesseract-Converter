//! File-bound Dynamic Link selection for the independently observed native profiles.

use std::{
    collections::{BTreeMap, BTreeSet, HashSet, btree_map::Entry},
    fs,
    path::{Path, PathBuf},
};

use fx_conv::{
    Artifact, ConversionMode, ConversionReport, MediaPreflight as MediaPreflightReport,
    ValidatedMediaMap,
};
use fx_schema::{
    AssetId, Duration, Layer, LayerId, MotionBlurSettings, animator::AnimationGraphEntry,
};
use sha2::{Digest, Sha256};
use tesseract_file::{AssetKind, TesseractFile, TesseractFileBuilder, TesseractFileError};
use thiserror::Error;

use super::{
    AepConversionError, AfterEffects, OUTPUT_NAME, fresh_destination, import_builder, media,
    native_media_inventory, native_media_references, read_input, selected_composition_id,
    write_project_checked,
};
use crate::{
    ImportDiagnostic,
    expression_samples::ExpressionSamples,
    structure::{ItemKind, ProjectItem, StructuralProject, read_project_with_header},
    structure_document::{
        AssetNamespace, Destination, MediaAssetRequest, StructuralConversion, composition_duration,
        to_linked_picture,
    },
};

/// Header profiles whose Dynamic Link GUID layout has native identity
/// evidence: the first four header bytes and the complete producer word.
///
/// - AE 26.5x89 (macOS): H-IDENTITY-01 pins two SAME-name comps, IDs 1 and
///   16, with Premiere's actual GUID payloads and independently rendered
///   selections (`tests/fixtures/hybrid/identity`).
/// - AE 26.3x87: a private package, retained outside Git, whose Premiere
///   26.3.0 ImporterPrefs GUIDs name three item IDs that its independently
///   produced AE validation receipt and the AEP's own items also name.
/// - Co-Editor format 96/subtype 6: five native Premiere ImporterPrefs GUIDs
///   match the pinned AEP's item IDs/names. The reduced native-derived fixture
///   retains that header and selected editable siblings (`hybrid/format96`).
///
/// Do not widen this to other producers without renewed native evidence.
const DYNAMIC_LINK_PROFILES: [([u8; 4], u32); 3] = [
    ([0, 97, 0, 10], 0x0f92_8659),
    ([0, 97, 0, 7], 0x0f91_8657),
    ([0, 96, 0, 6], 0x0f8a_0656),
];

/// A link could not be resolved safely; callers must not substitute a name or root.
#[derive(Debug, Error)]
pub enum DynamicLinkImportError {
    /// Input framing, bounds or filesystem validation failed.
    #[error(transparent)]
    Input(#[from] AepConversionError),
    /// No native identity evidence exists for this producer/header profile.
    #[error("unsupported Dynamic Link AEP profile: format {format}, producer {producer:#010x}")]
    UnsupportedProfile {
        /// Native binary revision.
        format: u8,
        /// Complete producer word, not just the marketing version.
        producer: u32,
    },
    /// Only the observed nonzero item-ID / zero-suffix GUID layout is supported.
    #[error("unsupported Dynamic Link composition GUID layout")]
    UnsupportedGuid,
    /// The exact selected file contains no composition with that identity.
    #[error("Dynamic Link item {0} is absent or is not a composition in this AEP")]
    MissingComposition(u32),
}

/// Immutable parsed input, bound to an absolute source path and its exact bytes.
///
/// Preparing reads/parses once, without Adobe, media resolution or publication.
/// GUID support is deliberately limited to the header profiles with native
/// identity evidence ([`DYNAMIC_LINK_PROFILES`]). This is not a generic
/// item-ID-to-GUID conversion API.
#[derive(Debug)]
pub struct PreparedAfterEffectsImport {
    input: PathBuf,
    source_sha256: [u8; 32],
    project: StructuralProject,
}

/// An exact composition borrowed from one prepared file, never a transferable ID.
#[derive(Debug)]
pub struct ResolvedAfterEffectsComposition<'a> {
    source: &'a PreparedAfterEffectsImport,
    item: &'a ProjectItem,
}

/// Where the picture of one Dynamic Link placement joins a host FX document.
#[derive(Debug, Clone, Copy)]
pub struct LinkedPictureTarget<'a> {
    /// Host Group that owns the composition's root Group.
    pub parent: LayerId,
    /// First layer, item and effect identity that the picture may use; the
    /// host uses none at or after it. Generated keyframe ids embed these.
    pub first_id: u64,
    /// Archive asset-id prefix of this AEP in the host: one per resolved AEP,
    /// shared by all of its pictures and distinct from every other asset id.
    pub asset_namespace: &'a str,
}

/// The editable picture of one Dynamic Link placement of a composition.
#[derive(Debug)]
pub struct LinkedPicture {
    /// The composition's root Group, a child of the target parent. Its audio is
    /// muted: Premiere plays a link's sound only through its audio track items.
    pub root: Layer,
    /// The animation entries of `root`'s subtree, for the host's graph.
    pub animations: Vec<AnimationGraphEntry>,
    /// The composition's motion blur; the picture's layers keep their switches.
    pub motion_blur: MotionBlurSettings,
    /// The first identity after the picture's own.
    pub next_id: u64,
    /// Approximation and omission notes of this import.
    pub diagnostics: Vec<ImportDiagnostic>,
}

/// The editable sound of one independent Dynamic Link audio placement.
#[derive(Debug)]
pub struct LinkedAudio {
    /// The composition's root Group. Visual layers are hidden, not audible layers.
    pub root: Layer,
    /// Animation entries whose clocks remain in the composition hierarchy.
    pub animations: Vec<AnimationGraphEntry>,
    /// First unused layer, item or effect identity.
    pub next_id: u64,
    /// Approximation and omission notes of the composition import.
    pub diagnostics: Vec<ImportDiagnostic>,
}

#[derive(Clone, Copy)]
enum LinkedContent {
    Picture,
    Audio,
}

/// The archive media of every linked picture or sound in one host document: each asset
/// once, the normalized files that back them, and the identity of every local
/// file that the pictures were read from.
#[derive(Debug, Default)]
pub struct LinkedMedia {
    assets: BTreeMap<AssetId, (MediaAssetRequest, PathBuf, AssetKind)>,
    /// The SHA-256, as lowercase hex, of each local file as conversion read it.
    sources: BTreeMap<PathBuf, String>,
    /// Higher-priority authored/alias paths that were missing when conversion
    /// chose relocated or collected footage; each must stay missing.
    relocated: BTreeSet<PathBuf>,
    normalized: Vec<tempfile::NamedTempFile>,
}

impl LinkedMedia {
    /// Packages every linked asset in the host's archive. The files stay valid
    /// while this value lives: keep it until that archive is written.
    pub fn add_to(
        &self,
        mut builder: TesseractFileBuilder,
    ) -> Result<TesseractFileBuilder, TesseractFileError> {
        for (id, (_, path, kind)) in &self.assets {
            builder = builder.add_asset(id.as_str(), path, *kind)?;
        }
        Ok(builder)
    }

    /// Checks that every local file of the pictures still has the bytes that
    /// conversion read, at the location that it chose: higher-priority missing
    /// footage paths must stay missing. This reads the files again; it is no
    /// filesystem lock, so check again after the archive is written.
    pub fn verify_sources(&self) -> Result<(), AepConversionError> {
        for authored in &self.relocated {
            if !fs::metadata(authored).is_err_and(|error| media::is_missing(&error)) {
                return Err(AepConversionError::MediaChanged(authored.clone()));
            }
        }
        for (path, sha256) in &self.sources {
            if media::sha256_file(path)? != *sha256 {
                return Err(AepConversionError::MediaChanged(path.clone()));
            }
        }
        Ok(())
    }

    /// Checks that `archive`, written from [`Self::add_to`], packaged each
    /// linked file that is its own asset with the bytes that conversion read.
    /// A normalized copy is checked through its source ([`Self::verify_sources`]).
    pub fn verify_packaged(&self, archive: &TesseractFile) -> Result<(), AepConversionError> {
        for (id, (_, path, _)) in &self.assets {
            if let Some(sha256) = self.sources.get(path)
                && archive.asset(id.as_str())?.descriptor().sha256 != *sha256
            {
                return Err(AepConversionError::MediaChanged(path.clone()));
            }
        }
        Ok(())
    }

    /// Keeps what one picture's `preflight` resolved for its archive assets
    /// `used`: each asset once, only the normalized files that back the newly
    /// kept ones, and the digest of every local file that it read. Returns the
    /// preflight's notes.
    fn record(
        &mut self,
        mut preflight: media::MediaPreflight<'_>,
        used: &[MediaAssetRequest],
    ) -> Result<Vec<ImportDiagnostic>, AepConversionError> {
        let mut kept = HashSet::new();
        for asset in preflight.used_assets(used)? {
            match self.assets.get(&asset.request.logical_id) {
                Some((existing, ..)) if existing != asset.request => {
                    return Err(AepConversionError::Input(
                        "conflicting linked media identity",
                    ));
                }
                Some(_) => {}
                None => {
                    kept.insert(asset.path.to_owned());
                    self.assets.insert(
                        asset.request.logical_id.clone(),
                        (asset.request.clone(), asset.path.to_owned(), asset.kind),
                    );
                }
            }
            self.add_source(asset.source)?;
        }
        for source in preflight.vector_sources() {
            self.add_source(source)?;
        }
        let diagnostics = std::mem::take(&mut preflight.diagnostics);
        // A repeated picture normalizes its media again, but the first copy
        // backs the kept asset; the others are deleted as they drop here.
        self.normalized.extend(
            preflight
                .into_normalized()
                .into_iter()
                .filter(|file| kept.contains(file.path())),
        );
        Ok(diagnostics)
    }

    fn add_source(&mut self, source: &media::SourceFile) -> Result<(), AepConversionError> {
        let sha256 = match (&source.decoded_sha256, self.sources.get(&source.path)) {
            (Some(sha256), _) | (None, Some(sha256)) => sha256.clone(),
            (None, None) => media::sha256_file(&source.path)?,
        };
        match self.sources.entry(source.path.clone()) {
            Entry::Occupied(entry) if *entry.get() != sha256 => {
                return Err(AepConversionError::MediaChanged(source.path.clone()));
            }
            Entry::Occupied(_) => {}
            Entry::Vacant(entry) => {
                entry.insert(sha256);
            }
        }
        self.relocated.extend(source.missing_paths.iter().cloned());
        Ok(())
    }
}

impl AfterEffects {
    /// Snapshots a source for the experimentally verified Dynamic Link profiles.
    ///
    /// Unknown profiles fail explicitly; ordinary numeric-composition import is
    /// unchanged. All parser size/depth/identity bounds still apply. This does
    /// not resolve Premiere occurrences, remap FX IDs, or prove render fidelity.
    pub fn prepare_linked_import(
        &self,
        input: &Path,
    ) -> Result<PreparedAfterEffectsImport, DynamicLinkImportError> {
        let input = std::path::absolute(input)
            .map_err(|source| AepConversionError::io("resolve AEP source path", input, source))?;
        let bytes = read_input(&input)?;
        let (project, head) = read_project_with_header(&bytes).map_err(AepConversionError::from)?;
        let header = head.encode();
        let profile = (
            [header[0], header[1], header[2], header[3]],
            head.producer_version_word(),
        );
        if !DYNAMIC_LINK_PROFILES.contains(&profile) {
            return Err(DynamicLinkImportError::UnsupportedProfile {
                format: head.format_version(),
                producer: head.producer_version_word(),
            });
        }
        Ok(PreparedAfterEffectsImport {
            input,
            source_sha256: Sha256::digest(&bytes).into(),
            project,
        })
    }
}

impl PreparedAfterEffectsImport {
    /// Path whose directory supplies the existing importer's media context.
    pub fn source_path(&self) -> &Path {
        &self.input
    }

    /// Exact source-byte identity; equal GUIDs in different files are not one source.
    pub fn source_sha256(&self) -> &[u8; 32] {
        &self.source_sha256
    }

    /// Resolves RFC/network-order UUID bytes within this immutable source only.
    ///
    /// Display names and item encounter order are never consulted. The parser
    /// already rejects duplicate and zero native IDs. An unknown suffix or a
    /// footage/folder/missing target is an error, not a default-composition hint.
    pub fn resolve_composition(
        &self,
        guid: &[u8; 16],
    ) -> Result<ResolvedAfterEffectsComposition<'_>, DynamicLinkImportError> {
        let id = u32::from_be_bytes([guid[0], guid[1], guid[2], guid[3]]);
        if id == 0 || guid[4..].iter().any(|byte| *byte != 0) {
            return Err(DynamicLinkImportError::UnsupportedGuid);
        }
        let item = self
            .project
            .item(id)
            .filter(|item| matches!(item.kind, ItemKind::Composition(_)))
            .ok_or(DynamicLinkImportError::MissingComposition(id))?;
        Ok(ResolvedAfterEffectsComposition { source: self, item })
    }

    fn verify_source(&self) -> Result<(), AepConversionError> {
        let bytes = read_input(&self.input)?;
        let observed: [u8; 32] = Sha256::digest(&bytes).into();
        if observed != self.source_sha256 {
            return Err(AepConversionError::SourceChanged(self.input.clone()));
        }
        Ok(())
    }
}

/// Editable conversion parts; retain this value until the consuming archive is
/// written so normalized media files remain available.
pub struct ImportedAfterEffectsComposition {
    document: Option<fx_schema::EditableFxCompositionDocument>,
    pub next_id: u64,
    pub assets: Vec<(fx_schema::AssetId, PathBuf, tesseract_file::AssetKind)>,
    pub diagnostics: Vec<ImportDiagnostic>,
    _normalized: Vec<tempfile::NamedTempFile>,
}

impl ImportedAfterEffectsComposition {
    /// Moves the typed document once, without an intermediate archive or JSON pass.
    pub fn take_document(
        &mut self,
    ) -> Result<fx_schema::EditableFxCompositionDocument, AepConversionError> {
        self.document.take().ok_or(AepConversionError::Input(
            "linked document already consumed",
        ))
    }
}

impl ResolvedAfterEffectsComposition<'_> {
    /// Convert this actual composition with caller-reserved IDs and asset names.
    /// Ordinary AE import uses the same converter with IDs starting at one.
    /// Linked picture content is muted; Premiere's explicit audio tracks own sound.
    ///
    /// This is the caller-supplied entry point of the picture that
    /// [`Self::import_picture`] imports: the same conversion, media preflight and
    /// best-effort diagnostics, with asset ids `<asset_prefix>aep-local-item-<id>`
    /// and a root Group without a parent. The caller owns the assets' lifetime
    /// and freshness: keep this value until the consuming archive is written.
    pub fn import_editable_picture(
        &self,
        first_id: u64,
        asset_prefix: &str,
    ) -> Result<ImportedAfterEffectsComposition, AepConversionError> {
        if first_id == 0 {
            return Err(AepConversionError::Input(
                "invalid linked import identity range",
            ));
        }
        self.source.verify_source()?;
        let namespace = format!("{asset_prefix}{}", AssetNamespace::STANDALONE.as_str());
        let (converted, mut preflight) =
            self.convert_content(None, first_id, &namespace, None, LinkedContent::Picture)?;
        let mut assets = Vec::new();
        let mut kept = HashSet::new();
        for asset in preflight.used_assets(&converted.assets)? {
            kept.insert(asset.path.to_owned());
            assets.push((
                asset.request.logical_id.clone(),
                asset.path.to_owned(),
                asset.kind,
            ));
        }
        let mut diagnostics = converted.diagnostics;
        diagnostics.append(&mut preflight.diagnostics);
        let normalized = preflight
            .into_normalized()
            .into_iter()
            .filter(|file| kept.contains(file.path()))
            .collect();
        self.source.verify_source()?;
        Ok(ImportedAfterEffectsComposition {
            document: Some(converted.document),
            next_id: converted.next_id,
            assets,
            diagnostics,
            _normalized: normalized,
        })
    }

    /// Item identity suitable only for this selected source (not a global ID).
    pub fn composition_id(&self) -> u32 {
        self.item.id
    }

    /// Diagnostic label; multiple resolved compositions may share it.
    pub fn name(&self) -> &str {
        &self.item.name
    }

    /// The composition canvas in pixels, as the AEP stores it.
    pub fn dimensions(&self) -> [u32; 2] {
        match &self.item.kind {
            ItemKind::Composition(composition) => {
                [composition.width, composition.height].map(u32::from)
            }
            _ => unreachable!("resolution admits only composition items"),
        }
    }

    /// The composition's duration, which the root Group of its picture spans.
    pub fn duration(&self) -> Duration {
        match &self.item.kind {
            ItemKind::Composition(composition) => composition_duration(composition.duration_secs),
            _ => unreachable!("resolution admits only composition items"),
        }
    }

    /// The immutable source context that owns this selection.
    pub fn source(&self) -> &PreparedAfterEffectsImport {
        self.source
    }

    /// Imports the selected snapshot through the existing editable media pipeline.
    ///
    /// Check/Write share lowering and source freshness checks. Write checks the
    /// source again after staging and before publishing; failure drops owned
    /// staging without reserving the destination. No Adobe expression sidecar
    /// is supplied: ordinary expression limitations remain diagnosed. Existing
    /// media preflight/archive validation apply; this is not a filesystem lock
    /// or an atomic snapshot of every external media file.
    pub fn import_to_tesseract(
        &self,
        output: &Path,
        mode: ConversionMode,
    ) -> Result<ConversionReport<ImportDiagnostic>, AepConversionError> {
        let destination = fresh_destination(output)?;
        self.source.verify_source()?;
        let prepared = import_builder(
            &self.source.input,
            &self.source.project,
            Some(self.item.id),
            &ExpressionSamples::default(),
        )?;
        if mode.is_check() {
            self.source.verify_source()?;
        } else {
            write_project_checked(prepared.builder, &destination, || {
                self.source.verify_source()
            })?;
        }
        Ok(ConversionReport {
            diagnostics: prepared.diagnostics,
            artifacts: vec![Artifact::project(OUTPUT_NAME)],
        })
    }

    /// Inspects the exact GUID-selected linked composition without binding the map
    /// to the inner AEP root; the outer format validates the map scope.
    pub fn inspect_media(
        &self,
        media_map: Option<&ValidatedMediaMap>,
    ) -> Result<MediaPreflightReport, AepConversionError> {
        let target = selected_composition_id(&self.source.project, Some(self.item.id))?;
        let (references, unassessed) =
            native_media_inventory(&self.source.project, Some(self.item.id))?;
        let mut resolver = media::MediaPreflight::with_project(
            &self.source.input,
            media_map,
            &self.source.project,
        );
        let mut inspected = Vec::with_capacity(references.len());
        for reference in references {
            inspected.push(resolver.inspect(
                &reference.request,
                reference.source_id.to_string(),
                reference.source_name,
                reference.references,
            ));
        }
        Ok(MediaPreflightReport {
            format: "after-effects".into(),
            target,
            media: inspected,
            unassessed,
        })
    }

    /// Imports the selected snapshot as the editable picture of one Dynamic
    /// Link placement, through the same conversion and media preflight as
    /// [`Self::import_to_tesseract`], and records its assets in `media`.
    ///
    /// Every numeric identity comes from `target.first_id` onward, so each
    /// placement of a composition needs its own range. The host owns source
    /// freshness before publishing: compare [`PreparedAfterEffectsImport::source_sha256`],
    /// and check `media` with [`LinkedMedia::verify_sources`] and
    /// [`LinkedMedia::verify_packaged`].
    pub fn import_picture(
        &self,
        target: LinkedPictureTarget<'_>,
        media: &mut LinkedMedia,
    ) -> Result<LinkedPicture, AepConversionError> {
        self.import_picture_with_media_map(target, media, None)
    }

    /// Imports a linked picture with caller-validated outer-scope replacements.
    pub fn import_picture_with_media_map(
        &self,
        target: LinkedPictureTarget<'_>,
        media: &mut LinkedMedia,
        media_map: Option<&ValidatedMediaMap>,
    ) -> Result<LinkedPicture, AepConversionError> {
        let (converted, preflight) = self.convert_content(
            Some(target.parent),
            target.first_id,
            target.asset_namespace,
            media_map,
            LinkedContent::Picture,
        )?;
        let mut diagnostics = converted.diagnostics;
        diagnostics.append(&mut media.record(preflight, &converted.assets)?);
        let composition = converted.document.composition();
        let [root] = composition.layers() else {
            panic!("a converted composition has exactly one root Group");
        };
        Ok(LinkedPicture {
            root: root.clone(),
            animations: composition.dynamics().entries().to_vec(),
            motion_blur: composition.motion_blur(),
            next_id: converted.next_id,
            diagnostics,
        })
    }

    /// Imports sound independently of a link's muted picture placement.
    ///
    /// The host reserves identities from `first_id`, supplies the source clock
    /// through `parent`, and applies the Premiere audio item's own gain. Asset
    /// names share the picture's namespace, so both placements package a source
    /// only once. Visual layers stay hidden to preserve graph references.
    pub fn import_audio_with_media_map(
        &self,
        parent: LayerId,
        first_id: u64,
        asset_namespace: &str,
        media: &mut LinkedMedia,
        media_map: Option<&ValidatedMediaMap>,
    ) -> Result<LinkedAudio, AepConversionError> {
        let (converted, preflight) = self.convert_content(
            Some(parent),
            first_id,
            asset_namespace,
            media_map,
            LinkedContent::Audio,
        )?;
        let mut diagnostics = converted.diagnostics;
        diagnostics.append(&mut media.record(preflight, &converted.assets)?);
        let composition = converted.document.composition();
        let [root] = composition.layers() else {
            panic!("a converted composition has exactly one root Group");
        };
        Ok(LinkedAudio {
            root: root.clone(),
            animations: composition.dynamics().entries().to_vec(),
            next_id: converted.next_id,
            diagnostics,
        })
    }

    /// Shares native selection, preflight and asset naming between the two
    /// independently placed streams of a Dynamic Link composition.
    fn convert_content<'a>(
        &'a self,
        parent: Option<LayerId>,
        first_id: u64,
        asset_namespace: &str,
        media_map: Option<&'a ValidatedMediaMap>,
        content: LinkedContent,
    ) -> Result<(StructuralConversion, media::MediaPreflight<'a>), AepConversionError> {
        if AssetId::new(asset_namespace).is_err() {
            return Err(AepConversionError::Input(
                "linked asset namespace is not a flat asset id",
            ));
        }
        let mut preflight = media::MediaPreflight::with_project(
            &self.source.input,
            media_map,
            &self.source.project,
        );
        for reference in native_media_references(&self.source.project, Some(self.item.id))? {
            preflight.require(&reference.request)?;
        }
        let converted = to_linked_picture(
            &self.source.project,
            self.item.id,
            &mut |request| preflight.resolve_media(request),
            match content {
                LinkedContent::Picture => Destination::LinkedPicture {
                    parent,
                    first_id,
                    asset_namespace: AssetNamespace::new(asset_namespace),
                },
                LinkedContent::Audio => Destination::LinkedAudio {
                    parent,
                    first_id,
                    asset_namespace: AssetNamespace::new(asset_namespace),
                },
            },
        )?;
        if let Some(error) = preflight.failure.take() {
            return Err(error);
        }
        Ok((converted, preflight))
    }
}

#[cfg(test)]
mod tests;
