//! Best-effort structural import. Fidelity losses are diagnostics, not fatal errors.

use std::collections::{HashMap, HashSet};

use fx_conv::{Progress, ProgressPhase};
use fx_schema::animator::{AnimationGraphEntry, KeyframeId, PropertyKeyframeEasing};
use fx_schema::{CompositionId, Duration, Time};
use fx_schema::{
    Dimensions, EditableFxCompositionDocument, GroupLayer, LayerData as FxLayer, LayerId,
    TimeRangeProperty, TimeRemapExtrapolation, TimeRemapKeyframe, TimeRemapProperty, Transform,
};

use crate::{
    diagnostic::{ImportDiagnostic, Limitation},
    document::DocumentError,
    expression_samples::ExpressionSamples,
    structure::{Composition, ItemKind, Layer, ProjectItem, StructuralProject},
};

mod adjustment;
mod alpha_projection;
mod alpha_stack;
mod animation;
mod animation_budget;
mod assembly;
mod basic_text;
mod camera_normalization;
mod compositing;
mod control_links;
mod directional_plane;
mod effects;
mod expression_evaluations;
mod foreign_inverse_matte;
mod fractal_blend;
mod geometry2;
pub(crate) mod graphic_template;
mod inverse_matte_transform;
mod layer_styles;
mod linear_wipe;
mod masks;
mod matte_text_paints;
mod media;
mod mirror;
mod posterize_time;
mod preserve_transparency;
mod radial_wipe;
#[cfg(test)]
mod scalar_script;
mod self_inverse_matte;
mod set_matte;
mod shadow_plane;
pub(crate) mod shapes;
mod split2;
pub(crate) mod text;
mod transform;
mod twirl_plane;
mod vegas;
pub(crate) use animation::editable_native_keys;
pub(crate) use media::{
    AssetNamespace, MediaAssetKind, MediaAssetRequest, MediaResolution, asset_request_for_source,
};

// Stay below JSON reader recursion limits, independently of binary RIFX depth.
const MAX_GROUP_DEPTH: usize = 24;
const MAX_TIME_SECS: f64 = 1_000_000_000.0;

/// A valid editable structural document, with every known loss made explicit.
pub struct StructuralConversion {
    /// Groups preserve occurrences; unsupported leaf contents are named placeholders.
    pub document: EditableFxCompositionDocument,
    /// Approximation and omission notes, also returned by check-only conversion.
    pub diagnostics: Vec<ImportDiagnostic>,
    pub(crate) assets: Vec<MediaAssetRequest>,
    /// The first generated identity that this conversion did not use.
    pub(crate) next_id: u64,
    #[cfg(test)]
    animation_budget_used: usize,
    #[cfg(test)]
    committed_animation_bytes: usize,
}

/// Where converted content lives.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Destination<'a> {
    /// Its own document, with identities from 1 and standalone asset ids.
    Document,
    /// The picture of a Premiere Dynamic Link placement: the composition's
    /// Group under `parent` (or a root that its host re-parents) in a host
    /// document that uses no identity at or after `first_id` and no asset of
    /// `asset_namespace`. Premiere plays a linked composition's sound only
    /// through its audio track items, so the picture's audio is muted, and it
    /// has no preview background: a link keeps the composition's alpha.
    LinkedPicture {
        parent: Option<LayerId>,
        first_id: u64,
        asset_namespace: AssetNamespace<'a>,
    },
    /// An independent Premiere audio placement. Keep the composition's audio
    /// clocks and editable gain, but suppress every visual layer.
    LinkedAudio {
        parent: Option<LayerId>,
        first_id: u64,
        asset_namespace: AssetNamespace<'a>,
    },
}

impl Destination<'_> {
    fn is_linked(self) -> bool {
        !matches!(self, Self::Document)
    }
}

/// Selects a composition and expands its layer structure without requiring visual fidelity.
pub fn to_structural_fx_document(
    project: &StructuralProject,
    composition_id: Option<u32>,
) -> Result<StructuralConversion, DocumentError> {
    to_structural_fx_document_with_assets(project, composition_id, &mut |_| false)
}

/// Resolves only media reached by this conversion, before emitting asset references.
pub(crate) fn to_structural_fx_document_with_assets(
    project: &StructuralProject,
    composition_id: Option<u32>,
    asset_available: &mut dyn FnMut(&MediaAssetRequest) -> bool,
) -> Result<StructuralConversion, DocumentError> {
    to_structural_fx_document_with_assets_and_expressions(
        project,
        composition_id,
        asset_available,
        &ExpressionSamples::default(),
    )
}

/// Imports explicitly captured native expression values without invoking Adobe.
pub(crate) fn to_structural_fx_document_with_assets_and_expressions(
    project: &StructuralProject,
    composition_id: Option<u32>,
    asset_available: &mut dyn FnMut(&MediaAssetRequest) -> bool,
    expression_samples: &ExpressionSamples,
) -> Result<StructuralConversion, DocumentError> {
    let mut resolve_media = |request: &MediaAssetRequest| {
        if asset_available(request) {
            MediaResolution::Asset
        } else {
            MediaResolution::Unavailable
        }
    };
    to_structural_fx_document_with_media_and_expressions(
        project,
        composition_id,
        &mut resolve_media,
        expression_samples,
    )
}

/// Resolves reached local sources to either packaged assets or immutable vector content.
pub(crate) fn to_structural_fx_document_with_media_and_expressions(
    project: &StructuralProject,
    composition_id: Option<u32>,
    media_resolver: &mut dyn FnMut(&MediaAssetRequest) -> MediaResolution,
    expression_samples: &ExpressionSamples,
) -> Result<StructuralConversion, DocumentError> {
    to_structural_fx_document_with_media_and_expressions_and_progress(
        project,
        composition_id,
        media_resolver,
        expression_samples,
        Progress::default(),
    )
}

pub(crate) fn to_structural_fx_document_with_media_and_expressions_and_progress(
    project: &StructuralProject,
    composition_id: Option<u32>,
    media_resolver: &mut dyn FnMut(&MediaAssetRequest) -> MediaResolution,
    expression_samples: &ExpressionSamples,
    progress: Progress<'_>,
) -> Result<StructuralConversion, DocumentError> {
    to_structural_fx_document_with_budget(
        project,
        composition_id,
        media_resolver,
        animation_budget::AnimationBudget::default(),
        expression_samples,
        Destination::Document,
        progress,
    )
}

/// Imports one composition as the picture of a Dynamic Link placement
/// ([`Destination::LinkedPicture`]), with the standalone document's limits.
pub(crate) fn to_linked_picture(
    project: &StructuralProject,
    composition_id: u32,
    media_resolver: &mut dyn FnMut(&MediaAssetRequest) -> MediaResolution,
    destination: Destination<'_>,
) -> Result<StructuralConversion, DocumentError> {
    to_structural_fx_document_with_budget(
        project,
        Some(composition_id),
        media_resolver,
        animation_budget::AnimationBudget::default(),
        &ExpressionSamples::default(),
        destination,
        Progress::default(),
    )
}

#[cfg(test)]
fn to_structural_fx_document_with_animation_limit(
    project: &StructuralProject,
    composition_id: Option<u32>,
    limit: usize,
) -> Result<StructuralConversion, DocumentError> {
    to_structural_fx_document_with_budget(
        project,
        composition_id,
        &mut |_| MediaResolution::Unavailable,
        animation_budget::AnimationBudget::with_limit(limit),
        &ExpressionSamples::default(),
        Destination::Document,
        Progress::default(),
    )
}

fn to_structural_fx_document_with_budget(
    project: &StructuralProject,
    composition_id: Option<u32>,
    media_resolver: &mut dyn FnMut(&MediaAssetRequest) -> MediaResolution,
    animation_budget: animation_budget::AnimationBudget,
    expression_samples: &ExpressionSamples,
    destination: Destination<'_>,
    progress: Progress<'_>,
) -> Result<StructuralConversion, DocumentError> {
    convert_with_text_overrides(
        project,
        composition_id,
        media_resolver,
        animation_budget,
        expression_samples,
        destination,
        progress,
        &[],
    )
}

pub(crate) fn to_graphic_picture(
    project: &StructuralProject,
    composition_id: u32,
    media_resolver: &mut dyn FnMut(&MediaAssetRequest) -> MediaResolution,
    destination: Destination<'_>,
    text: &[crate::graphic_template::SavedGraphicText],
) -> Result<StructuralConversion, DocumentError> {
    convert_with_text_overrides(
        project,
        Some(composition_id),
        media_resolver,
        animation_budget::AnimationBudget::default(),
        &ExpressionSamples::default(),
        destination,
        Progress::default(),
        text,
    )
}

#[allow(clippy::too_many_arguments)]
fn convert_with_text_overrides(
    project: &StructuralProject,
    composition_id: Option<u32>,
    media_resolver: &mut dyn FnMut(&MediaAssetRequest) -> MediaResolution,
    animation_budget: animation_budget::AnimationBudget,
    expression_samples: &ExpressionSamples,
    destination: Destination<'_>,
    progress: Progress<'_>,
    text: &[crate::graphic_template::SavedGraphicText],
) -> Result<StructuralConversion, DocumentError> {
    let compositions: Vec<_> = project
        .items
        .iter()
        .filter(|item| matches!(item.kind, ItemKind::Composition(_)))
        .collect();
    let selected = if let Some(id) = composition_id {
        compositions
            .iter()
            .copied()
            .find(|item| item.id == id)
            .ok_or(DocumentError::CompositionSelection(id))?
    } else {
        match compositions.len() {
            0 => return Err(DocumentError::NoComposition),
            1 => compositions[0],
            count => return Err(DocumentError::AmbiguousCompositionSelection { count }),
        }
    };
    let ItemKind::Composition(comp) = &selected.kind else {
        return Err(DocumentError::NoComposition);
    };
    expression_samples.validate_conversion_scope(selected.id, compositions.len())?;
    let camera_normalizations = camera_normalization::find(project, comp.width, comp.height);
    let (parent, first_id, asset_namespace) = match destination {
        Destination::Document => (None, 1, AssetNamespace::STANDALONE),
        Destination::LinkedPicture {
            parent,
            first_id,
            asset_namespace,
        }
        | Destination::LinkedAudio {
            parent,
            first_id,
            asset_namespace,
        } => (parent, first_id, asset_namespace),
    };
    let mut converter = Converter {
        text_overrides: text
            .iter()
            .map(|value| ((value.composition_id, value.layer_id), value))
            .collect(),
        expression_samples,
        expression_evaluations: expression_evaluations::ExpressionEvaluations::default(),
        items: project.items.iter().map(|item| (item.id, item)).collect(),
        camera_normalizations,
        diagnostics: Vec::new(),
        next_id: first_id,
        linked: destination.is_linked(),
        asset_namespace,
        stack: Vec::new(),
        visited_compositions: HashSet::new(),
        animations: Vec::new(),
        animation_budget,
        #[cfg(test)]
        committed_inline_remap_bytes: 0,
        unavailable_cutouts: 0,
        overrides: Vec::new(),
        media_resolver,
        assets: Vec::new(),
        shape_budget: shapes::OutputBudget::default(),
        mapped_shape_expressions: HashSet::new(),
        root_progress: progress.phase("convert AEP root layers", "root layers", comp.layers.len()),
    };
    if project.format_version != 97 {
        converter.warn(Limitation::Version, None, None, format!("producer format {} is parsed using known record layouts; AE26 is the primary implementation reference", project.format_version));
    }
    converter.warn(Limitation::ProjectMetadata, Some(selected.id), None,
        "only the selected composition graph is expanded; project folders, unused items, footage bytes/proxies, color management, render settings, markers and editor metadata are not exported; unnamed file sources use alias basenames, not sequence/layer-bound display names".into());
    let duration = converter.duration(selected.id, comp.duration_secs);
    let root_id = converter.allocate_id()?;
    let name = selected.name.to_string();
    let mut root = group(
        root_id,
        name.clone(),
        parent,
        TimeRangeProperty::new(Time::ZERO, duration),
    );
    root.description = format!(
        "AEP composition {}. Structural import; see docs/after-effects-support.md. No visual fidelity established.",
        selected.id
    );
    root.layers = stored_layers(converter.composition_layers(selected, root_id, 0)?)?;
    progress.stage("assemble Tesseract document");
    if matches!(destination, Destination::LinkedAudio { .. }) {
        hide_visuals(&mut root)?;
    }
    if matches!(destination, Destination::LinkedPicture { .. }) {
        let mut muted = HashSet::new();
        mute_audio(&mut root, &mut muted)?;
        if !muted.is_empty()
            && let Err(error) = converter.discard_animations(|entry| muted.contains(&entry.target))
        {
            converter.warn(
                Limitation::ExpansionLimit,
                Some(selected.id),
                None,
                format!(
                    "discarded muted-audio animation could not release its exact reservation: {error}; the allowance remains conservatively charged"
                ),
            );
        }
    }
    let width = comp.width.max(1);
    let height = comp.height.max(1);
    if width != comp.width || height != comp.height {
        converter.warn(
            Limitation::CompositionSettings,
            Some(selected.id),
            None,
            "zero canvas dimension replaced with one pixel".into(),
        );
    }
    #[cfg(test)]
    let (animation_budget_used, committed_animation_bytes) = {
        let committed_entries = converter
            .animations
            .iter()
            .map(|entry| {
                animation_budget::committed_entry_serialized_bytes(entry)
                    .expect("a committed animation entry must remain serializable")
            })
            .try_fold(0_usize, usize::checked_add)
            .expect("committed animation accounting must not overflow");
        let committed = committed_entries
            .checked_add(converter.committed_inline_remap_bytes)
            .expect("committed animation accounting must not overflow");
        let used = converter.animation_budget.used();
        assert!(
            committed <= used,
            "committed animation bytes {committed} exceed charged bytes {used}"
        );
        (used, committed)
    };
    let motion_blur = match compositing::motion_blur(&comp.record) {
        Ok(settings) => Some(settings),
        Err(error) => {
            converter.warn(
                Limitation::CompositionSettings,
                Some(selected.id),
                None,
                format!("invalid motion-blur settings replaced by disabled defaults: {error}"),
            );
            None
        }
    };
    let fx = assembly::composition(
        CompositionId::new("main"),
        name,
        fx_schema::AnimationGraph::from_entries(std::mem::take(&mut converter.animations))?,
        stored_layers(vec![FxLayer::Group(root)])?,
        motion_blur,
    )?;
    let document = EditableFxCompositionDocument::new(
        Dimensions::new(u32::from(width), u32::from(height)),
        duration,
        (!converter.linked).then(|| comp.record.background_color()),
        fx,
    )?;
    // Never silently discard a captured Shape identity without a committed mapping.
    for sample in expression_samples.properties() {
        if matches!(
            sample.property(),
            crate::expression_samples::PropertyIdentity::Shape { .. }
        ) && !converter.mapped_shape_expressions.contains(&(
            sample.composition_id(),
            sample.layer_id(),
            sample.property().clone(),
        )) && converter
            .visited_compositions
            .contains(&sample.composition_id())
        {
            converter.warn(
                Limitation::Properties,
                Some(sample.composition_id()),
                Some(sample.layer_id()),
                format!("captured native Shape expression target {:?} has no editable expression mapping; authored fallback retained", sample.property()),
            );
        }
    }
    for error in expression_samples.errors() {
        if converter
            .visited_compositions
            .contains(&error.composition_id())
        {
            converter.warn(Limitation::Properties, Some(error.composition_id()), Some(error.layer_id()),
                format!("AE expression evaluation failed for {:?}: {}; existing unsupported-expression fallback retained", error.property(), error.message()));
        }
    }
    Ok(StructuralConversion {
        document,
        diagnostics: converter.diagnostics,
        assets: converter.assets,
        next_id: converter.next_id,
        #[cfg(test)]
        animation_budget_used,
        #[cfg(test)]
        committed_animation_bytes,
    })
}

fn suppress_replaced_expression_warnings(warnings: &mut Vec<String>, evaluated: &[String]) {
    warnings.retain(|warning| {
        !evaluated.iter().any(|message| {
            message.rsplit_once(": ").is_some_and(|(name, provenance)| {
                (provenance.starts_with("AE-evaluated expression approximated with ")
                    || provenance.starts_with("converter-evaluated expression sampled")
                    || provenance.starts_with("converter-evaluated expression lowered into ")
                    || provenance.starts_with("AE-evaluated expression lowered into "))
                    && warning.starts_with(&format!("{name}: enabled AE expression"))
            })
        })
    });
}

/// Reserve a contiguous run of generated IDs; failure leaves the cursor unchanged.
fn reserve_ids(next_id: &mut u64, count: u64) -> Option<u64> {
    if count == 0 {
        return None;
    }
    let end = next_id.checked_add(count)?;
    let first = *next_id;
    *next_id = end;
    Some(first)
}

/// Finish fresh authoring DTOs through the portable schema's checked boundary.
fn stored_layers(layers: Vec<FxLayer>) -> Result<Vec<fx_schema::Layer>, serde_json::Error> {
    layers.iter().map(fx_schema::Layer::from_data).collect()
}

struct Converter<'a> {
    text_overrides: HashMap<(u32, u32), &'a crate::graphic_template::SavedGraphicText>,
    expression_samples: &'a ExpressionSamples,
    expression_evaluations: expression_evaluations::ExpressionEvaluations,
    items: HashMap<u32, &'a ProjectItem>,
    camera_normalizations: HashMap<u32, camera_normalization::CompositionNormalization>,
    diagnostics: Vec<ImportDiagnostic>,
    next_id: u64,
    /// Whether the root composition is a Dynamic Link picture
    /// ([`Destination::LinkedPicture`]) rather than a document of its own.
    linked: bool,
    asset_namespace: AssetNamespace<'a>,
    stack: Vec<u32>,
    visited_compositions: HashSet<u32>,
    animations: Vec<fx_schema::animator::AnimationGraphEntry>,
    animation_budget: animation_budget::AnimationBudget,
    #[cfg(test)]
    committed_inline_remap_bytes: usize,
    /// Drawn content whose pixels are wrong because a Roto Brush cutout was
    /// omitted: unsegmented paint, or a consumer left unmasked. A matte sample
    /// that adds to this count has no faithful alpha. Undrawn branches restore it.
    unavailable_cutouts: usize,
    overrides: Vec<crate::essential::Override>,
    media_resolver: &'a mut dyn FnMut(&MediaAssetRequest) -> MediaResolution,
    assets: Vec<MediaAssetRequest>,
    shape_budget: shapes::OutputBudget,
    mapped_shape_expressions: HashSet<(u32, u32, crate::expression_samples::PropertyIdentity)>,
    root_progress: ProgressPhase<'a>,
}

impl Converter<'_> {
    fn warn(
        &mut self,
        limitation: Limitation,
        composition_id: Option<u32>,
        layer_id: Option<u32>,
        message: String,
    ) {
        self.diagnostics.push(ImportDiagnostic {
            limitation,
            composition_id,
            layer_id,
            message,
        });
    }

    fn warn_animation_denial(
        &mut self,
        denials_before: usize,
        composition_id: u32,
        layer_id: u32,
        property: &str,
    ) {
        if self.animation_budget.denials() > denials_before {
            self.warn(
                Limitation::ExpansionLimit,
                Some(composition_id),
                Some(layer_id),
                format!(
                    "{property} animation omitted at the generated-animation allowance; static content was retained"
                ),
            );
        }
    }

    #[cfg(test)]
    fn note_committed_remap(&mut self, remap: &TimeRemapProperty) {
        let bytes = animation_budget::committed_remap_serialized_bytes(remap)
            .expect("a committed time remap must remain serializable");
        self.committed_inline_remap_bytes = self
            .committed_inline_remap_bytes
            .checked_add(bytes)
            .expect("committed remap accounting must not overflow");
    }

    #[cfg(not(test))]
    fn note_committed_remap(&mut self, _remap: &TimeRemapProperty) {}

    fn release_animation_entries(
        &mut self,
        entries: &[AnimationGraphEntry],
    ) -> Result<(), animation_budget::ReservationError> {
        let mut bytes = 0_usize;
        for entry in entries {
            bytes = bytes
                .checked_add(animation_budget::committed_entry_reservation_bytes(entry)?)
                .ok_or(animation_budget::ReservationError::SizeOverflow)?;
        }
        self.animation_budget.release(bytes)
    }

    fn discard_animations(
        &mut self,
        mut discard: impl FnMut(&AnimationGraphEntry) -> bool,
    ) -> Result<(), animation_budget::ReservationError> {
        let mut retained = Vec::with_capacity(self.animations.len());
        let mut discarded = Vec::new();
        for entry in std::mem::take(&mut self.animations) {
            if discard(&entry) {
                discarded.push(entry);
            } else {
                retained.push(entry);
            }
        }
        self.animations = retained;
        self.release_animation_entries(&discarded)
    }

    fn allocate_id(&mut self) -> Result<LayerId, DocumentError> {
        if let Some(id) = reserve_ids(&mut self.next_id, 1) {
            return Ok(id.into());
        }
        self.warn(
            Limitation::ExpansionLimit,
            self.stack.last().copied(),
            None,
            "generated layer identifier space exhausted; import stopped".into(),
        );
        Err(DocumentError::InvalidInput {
            field: "generated layer identifier",
            reason: "counter overflow while expanding compositions and helper layers",
        })
    }

    fn duration(&mut self, id: u32, seconds: f64) -> Duration {
        if !representable_duration(seconds) {
            self.warn(
                Limitation::Timing,
                Some(id),
                None,
                format!(
                    "invalid/unrepresentable composition duration {seconds}s replaced with 1 second"
                ),
            );
        } else if fractional_millis(seconds) {
            self.warn(
                Limitation::Timing,
                Some(id),
                None,
                format!("composition duration {seconds}s rounded to FX milliseconds"),
            );
        }
        composition_duration(seconds)
    }

    fn composition_layers(
        &mut self,
        item: &ProjectItem,
        parent: LayerId,
        depth: usize,
    ) -> Result<Vec<FxLayer>, DocumentError> {
        let ItemKind::Composition(comp) = &item.kind else {
            return Ok(Vec::new());
        };
        if self.stack.contains(&item.id) {
            self.warn(
                Limitation::Cycle,
                Some(item.id),
                None,
                "recursive precomposition edge replaced by an empty Group; other branches remain"
                    .into(),
            );
            return Ok(Vec::new());
        }
        if depth + 2 > MAX_GROUP_DEPTH {
            self.warn(Limitation::ExpansionLimit, Some(item.id), None, format!("precomposition expansion stopped at depth {MAX_GROUP_DEPTH}; branch remains an empty Group"));
            return Ok(Vec::new());
        }
        let mut comp = std::borrow::Cow::Borrowed(comp);
        let overrides: Vec<_> = self
            .overrides
            .iter()
            .filter(|value| value.source_comp_id == item.id)
            .cloned()
            .collect();
        for property_override in &overrides {
            if let crate::essential::OverrideValue::Media { source_id } = &property_override.value
                && !self.items.get(source_id).is_some_and(|source| {
                    matches!(source.kind, ItemKind::Footage | ItemKind::Composition(_))
                })
            {
                self.warn(
                    Limitation::MissingReference,
                    Some(item.id),
                    Some(property_override.source_layer_id),
                    format!("Essential media replacement source {source_id} is missing or not an AV item; original source retained"),
                );
                continue;
            }
            let Some(layer) = comp
                .to_mut()
                .layers
                .iter_mut()
                .find(|layer| layer.record.id() == property_override.source_layer_id)
            else {
                self.warn(
                    Limitation::MissingReference,
                    Some(item.id),
                    Some(property_override.source_layer_id),
                    "Essential Property target layer missing; override omitted".into(),
                );
                continue;
            };
            let warnings = match crate::essential::apply(layer, property_override) {
                Ok(warnings) => warnings,
                Err(warning) => vec![warning],
            };
            for warning in warnings {
                self.warn(
                    Limitation::Properties,
                    Some(item.id),
                    Some(property_override.source_layer_id),
                    format!("Essential Property {:?}: {}", warning.kind, warning.message),
                );
            }
        }
        for warning in &comp.essential_properties.warnings {
            self.warn(
                Limitation::Properties,
                Some(item.id),
                None,
                format!("Essential Graphics {:?}: {}", warning.kind, warning.message),
            );
        }
        if !self.visited_compositions.insert(item.id) {
            self.warn(Limitation::IndependentCopies, Some(item.id), None, "reused precomposition expanded into an independent editable Group with fresh FX IDs; shared-edit linkage is lost".into());
        }
        let background_note = if depth == 0 && self.linked {
            "the Dynamic Link picture keeps the composition's alpha, so its preview background is not materialized; alpha fidelity is unverified"
        } else if depth == 0 {
            "AE preview RGB background is materialized as opaque FX canvas content for RGB output; transparency is lost where no native background layer exists, and alpha fidelity is unverified"
        } else {
            "nested preview background is omitted because FX Groups have no fixed canvas"
        };
        self.warn(Limitation::CompositionSettings, Some(item.id), None, format!("source fps={}, pixel aspect={:?}, display start={}s; FX host uses square pixels without per-Group fps, display start, work area or renderer controls; {background_note}; root shutter settings are imported, nested shutter settings inherit the root", comp.frame_rate, comp.pixel_aspect, comp.display_start_secs));
        let camera_normalization = self.camera_normalizations.get(&item.id).copied();
        if let Some(normalization) = camera_normalization {
            self.warn(
                Limitation::GroupBounds,
                Some(item.id),
                Some(normalization.camera_layer_id),
                format!(
                    "canonical generated root camera removed and precomposition origin [{}, {}] inverse-normalized into editable FX coordinates; this converter convention is structural evidence, not general AE camera fidelity",
                    normalization.offset[0], normalization.offset[1]
                ),
            );
        } else {
            self.warn(Limitation::GroupBounds, Some(item.id), None, format!("source canvas {}x{} becomes a child-bounded Group; fixed-canvas/source-duration clipping and local 3D projection are not reproduced", comp.width, comp.height));
            if camera_normalization::has_generated_camera_name(&comp) {
                self.warn(
                    Limitation::Properties,
                    Some(item.id),
                    None,
                    "camera named 'FX root projection' does not match the complete canonical generated-camera record/properties/root zoom; it remains an explicit unsupported camera placeholder and no inverse origin normalization is applied".into(),
                );
            }
        }
        self.stack.push(item.id);
        let layer_indices = index_layers(&comp.layers);
        let evaluated = self.expression_evaluations.evaluate(
            &self.items,
            item.id,
            &comp,
            self.expression_samples,
            &overrides,
        );
        let expression_samples = &evaluated.samples;
        for approximation in &evaluated.approximations {
            for note in &approximation.key_notes {
                self.warn(
                    Limitation::Properties,
                    Some(item.id),
                    Some(approximation.layer_id),
                    format!(
                        "{:?}: AE expression input keys approximated: {note}",
                        approximation.property
                    ),
                );
            }
            for api in &approximation.apis {
                self.warn(Limitation::Properties, Some(item.id), Some(approximation.layer_id),
                    format!("{:?}: AE expression {api} approximated with a deterministic random API; random sequence/kernel differs from Adobe; baked source-frame values do not establish native fidelity", approximation.property));
            }
        }
        for error in expression_samples
            .errors()
            .iter()
            .filter(|error| error.composition_id() == item.id)
        {
            if !self.expression_samples.errors().contains(error) {
                self.warn(Limitation::Properties, Some(item.id), Some(error.layer_id()), format!("{:?}: converter expression evaluation unsupported ({}); existing native fallback retained", error.property(), error.message()));
            }
        }
        let solo = comp.layers.iter().any(|layer| layer.record.flags().solo);
        let context = LayerContext {
            comp_id: item.id,
            comp: &comp,
            parent,
            depth,
            solo,
            layer_indices: &layer_indices,
            camera_normalization,
            expression_samples,
        };
        let mut layers = Vec::new();
        let mut source_indices = Vec::new();
        let mut adjustment_guides = Vec::new();
        for (source_index, layer) in comp.layers.iter().enumerate() {
            if camera_normalization
                .is_some_and(|normalization| layer.record.id() == normalization.camera_layer_id)
            {
                if depth == 0 {
                    self.root_progress.update(source_index + 1);
                }
                continue;
            }
            if layer.record.flags().adjustment_layer {
                let (adjustment, mut guides) = self.adjustment_layer(&context, layer)?;
                layers.push(FxLayer::Adjustment(adjustment));
                adjustment_guides.append(&mut guides);
            } else {
                layers.push(FxLayer::Group(self.layer(
                    &context,
                    layer,
                    LayerPurpose::Ordinary,
                )?));
            }
            source_indices.push(source_index);
            if depth == 0 {
                self.root_progress.update(source_index + 1);
            }
        }
        let alpha_sources: Vec<_> = source_indices
            .iter()
            .copied()
            .zip(layers.iter().map(FxLayer::id))
            .filter(|(index, _)| matches!(comp.layers[*index].record.blend_mode(), 17 | 19))
            .collect();
        let emitted_end = self.next_id;
        self.apply_mattes(&context, &source_indices, &mut layers)?;
        self.apply_set_mattes(&context, &source_indices, &mut layers)?;
        self.apply_preserve_transparency(&context, &source_indices, &mut layers)?;
        if adjustment_guides.is_empty() {
            self.apply_posterize_time(&context, &source_indices, emitted_end, &mut layers)?;
        }
        if adjustment_guides.is_empty() {
            self.apply_split2(&context, &source_indices, &mut layers)?;
        }
        layers.append(&mut adjustment_guides);
        self.apply_alpha_stack(&context, &alpha_sources, &mut layers)?;
        self.stack.pop();
        Ok(layers)
    }

    fn apply_mattes(
        &mut self,
        context: &LayerContext<'_>,
        source_indices: &[usize],
        layers: &mut Vec<FxLayer>,
    ) -> Result<(), DocumentError> {
        let count = layers.len();
        let comp = context.comp;
        let emitted_indices: HashMap<_, _> = source_indices
            .iter()
            .copied()
            .enumerate()
            .map(|(emitted, source)| (source, emitted))
            .collect();
        let mut links = Vec::with_capacity(count);
        for &source_index in source_indices {
            let link = match compositing::matte_source(comp, source_index) {
                Ok(Some((source, mode))) => emitted_indices
                    .get(&source)
                    .copied()
                    .map(|source| (source, mode))
                    .or_else(|| {
                        self.warn(
                            Limitation::TrackMatte,
                            Some(context.comp_id),
                            Some(comp.layers[source_index].record.id()),
                            "matte source absent from converted siblings; matte omitted".into(),
                        );
                        None
                    }),
                Ok(None) => None,
                Err(message) => {
                    self.warn(
                        Limitation::TrackMatte,
                        Some(context.comp_id),
                        Some(comp.layers[source_index].record.id()),
                        message,
                    );
                    None
                }
            };
            links.push(link);
        }
        // Source resolution has the same bounded depth as runtime evaluation.
        // Drop invalid edges before creating a helper that could consume pixels.
        let valid: Vec<_> = (0..count)
            .map(|start| {
                let mut current = start;
                let mut seen = HashSet::new();
                for _ in 0..MAX_GROUP_DEPTH {
                    if !seen.insert(current) {
                        return false;
                    }
                    let Some((source, _)) = links[current] else {
                        return true;
                    };
                    current = source;
                }
                false
            })
            .collect();
        for (index, valid) in valid.into_iter().enumerate() {
            if !valid && links[index].take().is_some() {
                self.warn(
                    Limitation::TrackMatte,
                    Some(context.comp_id),
                    Some(comp.layers[source_indices[index]].record.id()),
                    "cyclic or over-depth matte dependency omitted".into(),
                );
            }
        }
        let referenced: HashSet<_> = links.iter().flatten().map(|(source, _)| *source).collect();
        let mut helpers = HashMap::new();
        let mut cutout_providers = HashSet::new();
        // Matte samples are not paint. Only a drawn consumer that loses its
        // matte below adds to the enclosing composition's unavailable count.
        let cutouts_before_samples = self.unavailable_cutouts;
        for (index, &source_index) in source_indices.iter().enumerate() {
            if !referenced.contains(&index) {
                continue;
            }
            let source = &comp.layers[source_index];
            // FX always consumes a matte provider from ordinary paint and hides
            // disabled providers even during sampling. An independent helper
            // preserves AE's independent video switch and visible provider copy.
            if source.record.flags().adjustment_layer {
                self.warn(
                    Limitation::TrackMatte,
                    Some(context.comp_id),
                    Some(source.record.id()),
                    "Adjustment layers used as matte providers have recursive stack semantics that cannot be sampled as an independent FX matte; provider omitted and siblings retained".into(),
                );
                continue;
            }
            let cutouts_before = self.unavailable_cutouts;
            let mut helper = self.layer(
                context,
                source,
                LayerPurpose::MatteSample(set_matte::MatteSampleStage::AllEffects),
            )?;
            let mut muted = HashSet::new();
            mute_audio(&mut helper, &mut muted)?;
            if !muted.is_empty()
                && let Err(error) = self.discard_animations(|entry| muted.contains(&entry.target))
            {
                self.warn(
                    Limitation::ExpansionLimit,
                    Some(context.comp_id),
                    Some(source.record.id()),
                    format!(
                        "discarded matte-helper audio animation could not release its exact reservation: {error}; the allowance remains conservatively charged"
                    ),
                );
            }
            // This independent All Effects copy is itself sampled as ordinary
            // provider content. Preserve its existing bounded Set Matte gate
            // before publishing the helper ID used by the native track matte.
            let mut helper_layers = vec![FxLayer::Group(helper)];
            self.apply_set_mattes(context, &[source_index], &mut helper_layers)?;
            let Some(FxLayer::Group(mut helper)) = helper_layers.pop() else {
                unreachable!("Set Matte lowering retains one Group helper")
            };
            helper.name.push_str(" (matte source)");
            helper
                .description
                .push_str("; independent matte sample copy");
            // Neither raw full-frame nor emptied alpha is the omitted segmentation.
            if self.unavailable_cutouts > cutouts_before {
                self.warn(Limitation::TrackMatte, Some(context.comp_id), Some(source.record.id()), "matte provider alpha depends on an omitted Roto Brush (ADBE Samurai) cutout; consumers are left unmasked and this provider copy is hidden and unlinked".into());
                helper.is_hidden = true;
                cutout_providers.insert(index);
                layers.push(FxLayer::Group(helper));
                continue;
            }
            self.warn(Limitation::TrackMatte, Some(context.comp_id), Some(source.record.id()), "independent editable provider copy preserves separate video visibility; subsequent edits are not linked to the paint copy".into());
            helpers.insert(index, (helper.id, layers.len()));
            layers.push(FxLayer::Group(helper));
        }
        self.unavailable_cutouts = cutouts_before_samples;
        // A provider sampled through its own matte inherits that matte's
        // unavailable alpha. The validated links are acyclic and bounded.
        let mut pending: Vec<_> = cutout_providers.iter().copied().collect();
        while let Some(unavailable) = pending.pop() {
            for (consumer, link) in links.iter().enumerate() {
                if link.is_some_and(|(source, _)| source == unavailable)
                    && let Some((_, helper_index)) = helpers.remove(&consumer)
                {
                    if let FxLayer::Group(helper) = &mut layers[helper_index] {
                        helper.is_hidden = true;
                    }
                    self.warn(
                        Limitation::TrackMatte,
                        Some(context.comp_id),
                        Some(comp.layers[source_indices[consumer]].record.id()),
                        format!(
                            "matte provider alpha depends, through its own track matte from layer {}, on an omitted Roto Brush (ADBE Samurai) cutout; consumers are left unmasked and this provider copy is hidden and unlinked",
                            comp.layers[source_indices[unavailable]].record.id()
                        ),
                    );
                    cutout_providers.insert(consumer);
                    pending.push(consumer);
                }
            }
        }
        for (index, link) in links.into_iter().enumerate() {
            let Some((source, mode)) = link else {
                continue;
            };
            if cutout_providers.contains(&source) {
                let consumer = &comp.layers[source_indices[index]];
                self.warn(
                    Limitation::TrackMatte,
                    Some(context.comp_id),
                    Some(consumer.record.id()),
                    format!(
                        "track matte from layer {} omitted: its alpha depends on an omitted Roto Brush cutout; layer left unmasked",
                        comp.layers[source_indices[source]].record.id()
                    ),
                );
                if paints_visuals(consumer.record.flags(), context.solo) {
                    self.unavailable_cutouts += 1;
                }
                continue;
            }
            let Some(&(id, _)) = helpers.get(&source) else {
                continue;
            };
            let matte = fx_schema::TrackMatte { mode, layer: id };
            match &mut layers[index] {
                FxLayer::Group(layer) => layer.track_matte = Some(matte.clone()),
                FxLayer::Adjustment(layer) => layer.track_matte = Some(matte.clone()),
                _ => {}
            }
            if let Some(&(_, helper_index)) = helpers.get(&index)
                && let FxLayer::Group(helper) = &mut layers[helper_index]
            {
                helper.track_matte = Some(matte);
            }
        }
        Ok(())
    }

    fn apply_preserve_transparency(
        &mut self,
        context: &LayerContext<'_>,
        source_indices: &[usize],
        layers: &mut Vec<FxLayer>,
    ) -> Result<(), DocumentError> {
        let mut helpers: HashMap<Vec<usize>, LayerId> = HashMap::new();
        for (emitted_index, &source_index) in source_indices.iter().enumerate() {
            let source_layer = &context.comp.layers[source_index];
            if !source_layer.record.flags().preserve_transparency {
                continue;
            }
            let selection = match preserve_transparency::providers(
                context.comp,
                source_index,
                context.solo,
            ) {
                Ok(selection) => selection,
                Err(message) => {
                    self.warn(
                        Limitation::LayerSwitches,
                        Some(context.comp_id),
                        Some(source_layer.record.id()),
                        format!(
                            "Preserve Underlying Transparency was not lowered: {message}; ordinary source-over compositing retained"
                        ),
                    );
                    continue;
                }
            };
            if selection
                .indices
                .iter()
                .any(|provider| !source_indices.contains(provider))
            {
                self.warn(
                    Limitation::LayerSwitches,
                    Some(context.comp_id),
                    Some(source_layer.record.id()),
                    "Preserve Underlying Transparency depends on a source layer omitted from this converted stack; ordinary source-over compositing retained".into(),
                );
                continue;
            }
            let helper_id = if let Some(id) = helpers.get(&selection.indices).copied() {
                id
            } else {
                if context.depth + 1 >= MAX_GROUP_DEPTH {
                    self.warn(
                        Limitation::ExpansionLimit,
                        Some(context.comp_id),
                        Some(source_layer.record.id()),
                        "Preserve Underlying Transparency provider would exceed the nesting-depth limit; ordinary source-over compositing retained".into(),
                    );
                    continue;
                }
                let id = self.allocate_id()?;
                let mut helper = group(
                    id,
                    "Preserve Underlying Transparency source".into(),
                    Some(context.parent),
                    TimeRangeProperty::new(
                        Time::ZERO,
                        self.duration(context.comp_id, context.comp.duration_secs),
                    ),
                );
                helper.description = "Independent editable alpha sample of overlapping prior native siblings; bounded approximation of AE Preserve Underlying Transparency for opaque interiors"
                    .into();
                let sample_context = LayerContext {
                    expression_samples: context.expression_samples,
                    comp_id: context.comp_id,
                    comp: context.comp,
                    parent: helper.id,
                    depth: context.depth + 1,
                    solo: context.solo,
                    layer_indices: context.layer_indices,
                    camera_normalization: context.camera_normalization,
                };
                let mut samples = Vec::with_capacity(selection.indices.len());
                for &provider_index in &selection.indices {
                    let provider = &context.comp.layers[provider_index];
                    samples.push(FxLayer::Group(self.layer(
                        &sample_context,
                        provider,
                        LayerPurpose::Ordinary,
                    )?));
                }
                self.apply_mattes(&sample_context, &selection.indices, &mut samples)?;
                self.apply_set_mattes(&sample_context, &selection.indices, &mut samples)?;
                for mut sample in samples {
                    let mut muted = HashSet::new();
                    if let FxLayer::Group(group) = &mut sample {
                        mute_audio(group, &mut muted)?;
                    }
                    if !muted.is_empty()
                        && let Err(error) =
                            self.discard_animations(|entry| muted.contains(&entry.target))
                    {
                        self.warn(
                            Limitation::ExpansionLimit,
                            Some(context.comp_id),
                            Some(source_layer.record.id()),
                            format!(
                                "discarded preserve-transparency sample audio animation could not release its exact reservation: {error}; the allowance remains conservatively charged"
                            ),
                        );
                    }
                    helper.layers.push(fx_schema::Layer::from_data(&sample)?);
                }
                let id = helper.id;
                layers.push(FxLayer::Group(helper));
                helpers.insert(selection.indices.clone(), id);
                id
            };
            let matte = fx_schema::TrackMatte {
                mode: fx_schema::TrackMatteType::Alpha,
                layer: helper_id,
            };
            match &mut layers[emitted_index] {
                FxLayer::Group(layer) => layer.track_matte = Some(matte),
                FxLayer::Adjustment(_) => {
                    self.warn(
                        Limitation::LayerSwitches,
                        Some(context.comp_id),
                        Some(source_layer.record.id()),
                        "Preserve Underlying Transparency on an Adjustment cannot use the bounded ordinary-layer alpha sample; ordinary compositing retained".into(),
                    );
                    continue;
                }
                _ => continue,
            }
            self.warn(
                Limitation::IndependentCopies,
                Some(context.comp_id),
                Some(source_layer.record.id()),
                format!(
                    "Preserve Underlying Transparency approximated with FX alpha matte helper {helper_id}; {} overlapping prior siblings were independently sampled and subsequent edits are not linked. Opaque interiors match SourceAtop alpha, but partially transparent/antialiased edges can gain alpha because ordinary SourceOver after masking is not exact AE SourceAtop",
                    selection.indices.len()
                ),
            );
        }
        Ok(())
    }

    fn layer(
        &mut self,
        context: &LayerContext<'_>,
        layer: &Layer,
        purpose: LayerPurpose,
    ) -> Result<GroupLayer, DocumentError> {
        self.layer_with_effect_gate(context, layer, purpose)
            .map(|imported| imported.0)
    }

    fn layer_with_effect_gate(
        &mut self,
        context: &LayerContext<'_>,
        layer: &Layer,
        purpose: LayerPurpose,
    ) -> Result<(GroupLayer, Option<fx_schema::PercentageProperty>), DocumentError> {
        let mut adjustment_opacity = None;
        let mut native_effect_ordinals = Vec::new();
        let mut fractal_blends = Vec::new();
        let LayerContext {
            comp_id,
            comp,
            parent,
            depth,
            solo,
            layer_indices,
            camera_normalization,
            expression_samples,
        } = *context;
        let ancestors = self.transform_ancestors(context, layer);
        let content_depth = context.depth + ancestors.len() + 2;
        let record = &layer.record;
        let source_id = record.source_id();
        let layer_id = record.id();
        let flags = record.flags();
        let id = self.allocate_id()?;
        let name = layer.name.to_string();
        let mut result = group(
            id,
            name,
            Some(parent),
            TimeRangeProperty::new(Time::ZERO, self.duration(comp_id, comp.duration_secs)),
        );
        result.is_hidden = !purpose.samples_matte() && (flags.guide_layer || (solo && !flags.solo));
        // Matte sampling ignores the provider's own switches; ordinary paint
        // contributes only when AE draws it.
        let contributes = purpose.samples_matte() || paints_visuals(flags, solo);
        let cutouts_on_entry = self.unavailable_cutouts;
        let content_id = self.allocate_id()?;
        let remap_eligible = content_depth < MAX_GROUP_DEPTH;
        let remap_checkpoint = self.animation_budget.checkpoint();
        let remap_denials = self.animation_budget.denials();
        let (authored_remap, remap_warnings) = if remap_eligible {
            animation::time_remap(layer, comp, content_id, &mut self.animation_budget)
        } else {
            if animation::has_authored_time_remap(layer) {
                self.warn(
                    Limitation::ExpansionLimit,
                    Some(comp_id),
                    Some(layer_id),
                    "authored remap omitted before construction at the nesting depth limit; affine timing retained"
                        .into(),
                );
            }
            (None, Vec::new())
        };
        let remap_budget_denied = self.animation_budget.denials() > remap_denials;
        self.warn_animation_denial(remap_denials, comp_id, layer_id, "ADBE Time Remapping");
        for message in remap_warnings {
            self.warn(Limitation::Timing, Some(comp_id), Some(layer_id), message);
        }
        let apply_authored_remap = authored_remap.is_some();
        let source = self.items.get(&source_id).copied();
        let ordinary_av_source =
            record.layer_type() == 0 && !flags.null_layer && !flags.adjustment_layer;
        let still_image = ordinary_av_source && source.is_some_and(is_still_image);
        // Still media and native Solid sources are constant rasters, not sampled
        // timelines. Their layer lifetime can include time before startTime;
        // clipping a nonexistent source clock would discard visible content.
        let static_raster_source = ordinary_av_source
            && source.is_some_and(|source| is_still_image(source) || is_solid_source(source));
        let retain_affine_source_clock =
            purpose != LayerPurpose::Adjustment && !static_raster_source;
        let timing = self.timing(
            comp_id,
            comp,
            layer,
            id,
            retain_affine_source_clock && !apply_authored_remap && !remap_budget_denied,
        );
        let mut content = group(
            content_id,
            "Source content clock".into(),
            Some(id),
            timing.active_range,
        );
        if let Some(playback) = timing.playback {
            content.playback = remapped_playback(timing.active_range, playback);
        }
        content.is_hidden = timing.hide_content;
        let range = timing.active_range;
        let mut clip = None;
        if let Some(remap) = authored_remap {
            let gate_denials = self.animation_budget.denials();
            let gate = (|| {
                let mut estimate = animation_budget::TimeRemapEstimate::default();
                for (index, time) in [range.start, range.end()].into_iter().enumerate() {
                    let id = animation_budget::GeneratedKeyframeIdSize::new(format_args!(
                        "aep-clip-{}-{index}",
                        content.id
                    ))
                    .map_err(|error| error.to_string())?;
                    estimate
                        .push_key(&id, time, time, PropertyKeyframeEasing::Linear)
                        .map_err(|error| error.to_string())?;
                }
                let reservation = estimate
                    .reservation_bytes(
                        TimeRemapExtrapolation::Inactive,
                        TimeRemapExtrapolation::Inactive,
                    )
                    .map_err(|error| error.to_string())?;
                self.animation_budget
                    .reserve(reservation)
                    .map_err(|error| error.to_string())?;
                let gate_keys = [range.start, range.end()]
                    .into_iter()
                    .enumerate()
                    .map(|(index, time)| TimeRemapKeyframe {
                        id: KeyframeId::new(format!("aep-clip-{}-{index}", content.id)),
                        time,
                        value: time,
                        easing: PropertyKeyframeEasing::Linear,
                    })
                    .collect();
                let gate = TimeRemapProperty::new(
                    gate_keys,
                    TimeRemapExtrapolation::Inactive,
                    TimeRemapExtrapolation::Inactive,
                )
                .map_err(|error| error.to_string())?;
                // One authored key is constant over exactly this lifetime.
                let remap = match remap {
                    animation::AuthoredRemap::Keys(remap) => remap,
                    animation::AuthoredRemap::Constant(value) => animation::constant_time_remap(
                        range,
                        value,
                        content_id,
                        &mut self.animation_budget,
                    )
                    .map_err(|error| format!("one-key constant remap: {error}"))?,
                };
                Ok::<_, String>((gate, remap))
            })();
            match gate {
                Ok((gate, remap)) => {
                    self.note_committed_remap(&gate);
                    self.note_committed_remap(&remap);
                    content.playback = remapped_playback(range, gate);
                    let source_clock_range =
                        TimeRangeProperty::new(Time::ZERO, Duration::from_secs(MAX_TIME_SECS));
                    let mut source_clock = group(
                        self.allocate_id()?,
                        "Authored source remap".into(),
                        Some(content.id),
                        source_clock_range,
                    );
                    source_clock.playback = remapped_playback(source_clock_range, remap);
                    clip = Some(std::mem::replace(&mut content, source_clock));
                }
                Err(error) => {
                    self.animation_budget.rollback(remap_checkpoint);
                    self.warn_animation_denial(
                        gate_denials,
                        comp_id,
                        layer_id,
                        "authored remap visibility gate",
                    );
                    self.warn(
                        Limitation::Timing,
                        Some(comp_id),
                        Some(layer_id),
                        format!(
                            "invalid or over-budget remap visibility gate: {error}; parent visibility retained with playback unset"
                        ),
                    );
                    let fallback_timing = self.timing(comp_id, comp, layer, id, false);
                    content.playback = fallback_timing.playback.map_or_else(
                        || identity_playback(fallback_timing.active_range),
                        |playback| remapped_playback(fallback_timing.active_range, playback),
                    );
                    content.is_hidden = fallback_timing.hide_content;
                }
            }
        }
        result.description = format!(
            "AEP comp={comp_id} layer={layer_id} kind={} source={source_id} transformParent={} matte={:?}; editable best-effort import; inspect conversion diagnostics",
            record.layer_type(),
            record.parent_id(),
            record.matte_layer_id_raw()
        );
        self.warn(Limitation::LayerMetadata, Some(comp_id), Some(layer_id), format!("source IDs/kind stored in description; label={}, editor state, quality, raw record fields and property payloads are not editable FX metadata", record.label()));
        self.warn(Limitation::LayerSwitches, Some(comp_id), Some(layer_id), format!("enabled/solo/guide flattened to isHidden={}; 3D Transform, layer motion-blur flags and supported Effects use existing FX controls. Collapse/continuous-rasterization, auto-orient, sampling quality, unsupported preserve-transparency cases, shy/lock and editor-only switches have no equivalent importer mapping; source flags={flags:?}", result.is_hidden));
        if record.parent_id() != 0 && !layer_indices.contains_key(&record.parent_id()) {
            self.warn(
                Limitation::MissingReference,
                Some(comp_id),
                Some(layer_id),
                format!("missing transform parent {} ignored", record.parent_id()),
            );
        }
        if let Some(mode) = compositing::blend_mode(record.blend_mode()) {
            result.blend_mode = mode;
        } else if !matches!(record.blend_mode(), 17 | 19) {
            self.warn(
                Limitation::BlendMode,
                Some(comp_id),
                Some(layer_id),
                format!(
                    "AE transfer mode {} has no existing FX blend equivalent; Normal used",
                    record.blend_mode()
                ),
            );
        }
        // A document's root switch is its composition setting; a linked
        // picture, like a nested one, takes its composition's switch here.
        result.motion_blur =
            flags.motion_blur && ((depth == 0 && !self.linked) || comp.record.flags()[1] & 8 != 0);
        let size = source_dimensions(source, layer);
        let anchor_dimensions = source_anchor_dimensions(source, layer);
        let correction = camera_normalization::LayerCorrection {
            position: camera_normalization
                .filter(|_| record.parent_id() == 0)
                .map(|normalization| normalization.offset),
            anchor: self
                .camera_normalizations
                .get(&source_id)
                .map(|normalization| normalization.offset),
        };
        // Shape/Text controls are natively composition-sized despite lacking a
        // footage source. Every imported effect is hosted by an FX Group whose
        // effect UV plane is the composition, which can differ from a solid or
        // media source's native control plane.
        let comp_sized_effects = matches!(record.layer_type(), 3 | 4);
        let native_effect_size = if comp_sized_effects {
            [comp.width, comp.height]
        } else {
            size
        };
        let mut unsupported_cutout = false;
        let mut basic_text = None;
        let mut unsupported_sweep_cutout = false;
        // The generic frame-fade outcome waits for the Shape importer, which can
        // commit the same preset onto a caption paint Group.
        let mut frame_fade: Result<Option<effects::FrameFade>, String> = Ok(None);
        let mut shape_lowered_fade = false;
        if purpose.includes_occurrence_pipeline() {
            let effect_denials = self.animation_budget.denials();
            let imported_effects = effects::import_with_context(
                effects::ImportContext {
                    evaluations: expression_samples,
                    composition_id: comp_id,
                    composition: Some(comp),
                    items: Some(&self.items),
                },
                layer,
                native_effect_size,
                [comp.width, comp.height],
                &mut self.next_id,
                &mut self.animation_budget,
            );
            if comp_sized_effects && !imported_effects.effects.is_empty() {
                self.warn(
                Limitation::Properties,
                Some(comp_id),
                Some(layer_id),
                "Shape/Text Effects retain their composition-sized coordinate plane on the destination FX Group; effect-specific algorithm, expansion and edge behavior can still differ".into(),
            );
            }
            basic_text = imported_effects.basic_text;
            adjustment_opacity = imported_effects.adjustment_opacity;
            native_effect_ordinals = imported_effects.native_ordinals;
            fractal_blends = imported_effects.fractal_blends;
            result.effects = imported_effects.effects;
            unsupported_cutout = imported_effects.unsupported_cutout;
            unsupported_sweep_cutout = imported_effects.unsupported_sweep_cutout;
            frame_fade = imported_effects.frame_fade;
            self.animations.extend(imported_effects.animations);
            self.warn_animation_denial(effect_denials, comp_id, layer_id, "Effects");
            for message in imported_effects.warnings {
                self.warn(
                    Limitation::Properties,
                    Some(comp_id),
                    Some(layer_id),
                    message,
                );
            }
            let style_denials = self.animation_budget.denials();
            let imported_styles = layer_styles::import(
                layer,
                [
                    f64::from(native_effect_size[0]),
                    f64::from(native_effect_size[1]),
                ],
                &mut self.next_id,
                &mut self.animation_budget,
            );
            // Layer Styles render after the effect stack; owner Opacity would also fade them.
            if !imported_styles.effects.is_empty() && matches!(frame_fade, Ok(Some(_))) {
                frame_fade = Err("Fade In+Out - frames: frame fade not lowered (Layer Styles render after the Solid Composite); Effect ADBE CM FadeInOutFrames and Effect ADBE Solid Composite omitted, owner retained".into());
            }
            result.effects.extend(imported_styles.effects);
            self.animations.extend(imported_styles.animations);
            self.warn_animation_denial(style_denials, comp_id, layer_id, "Layer Styles");
            for message in imported_styles.warnings {
                self.warn(
                    Limitation::Properties,
                    Some(comp_id),
                    Some(layer_id),
                    message,
                );
            }
        }
        let (mut transform, mut warnings) =
            transform::static_transform_with_sources(layer, size, comp_id, comp, &self.items);
        let anchor_scale = solid_anchor_scale(source, anchor_dimensions);
        if source.is_some_and(has_source_relative_anchor) {
            normalize_static_solid_anchor(layer, anchor_dimensions, &mut transform, &mut warnings);
        }
        let composition_offset = match camera_normalization::apply_static(
            &mut transform,
            correction,
        ) {
            Ok(()) => correction.position,
            Err(error) => {
                warnings.push(format!("generated-camera inverse normalization: {error}; affected static Transform component retained without translation"));
                None
            }
        };
        result.transform = transform;
        if self.stack.len() == 1
            && !self.linked
            && purpose.includes_occurrence_pipeline()
            && record.parent_id() == 0
            && record.auto_orient() == 0
            && record.layer_type() == 0
            && !flags.three_d_layer
            && !flags.null_layer
            && !flags.adjustment_layer
            && comp.pixel_aspect.0 == comp.pixel_aspect.1
            && source
                .and_then(|source| source.solid.as_ref())
                .and_then(|solid| solid.as_ref().ok())
                .is_some_and(|solid| solid.pixel_aspect.0 == solid.pixel_aspect.1)
        {
            match shadow_plane::compensate(layer, size, &result.transform, &mut result.effects) {
                Ok(true) => self.warn(
                    Limitation::Properties,
                    Some(comp_id),
                    Some(layer_id),
                    "Isolated static hard Drop Shadow offset transformed from the native Solid source plane into FX screen pixels; soft kernels, nested/animated transforms and native export fidelity remain unverified".into(),
                ),
                Ok(false) => {}
                Err(error) => self.warn(
                    Limitation::Properties,
                    Some(comp_id),
                    Some(layer_id),
                    format!("Hard Drop Shadow source-plane offset retained without compensation: {error}"),
                ),
            }
        }
        if self.stack.len() == 1
            && !self.linked
            && purpose.includes_occurrence_pipeline()
            && record.parent_id() == 0
            && record.layer_type() == 0
            && record.track_matte_type() == 0
            && !flags.three_d_layer
            && !flags.null_layer
            && !flags.adjustment_layer
            && !flags.preserve_transparency
            && comp.pixel_aspect.0 == comp.pixel_aspect.1
            && source
                .and_then(|source| source.solid.as_ref())
                .and_then(|solid| solid.as_ref().ok())
                .is_some_and(|solid| solid.pixel_aspect.0 == solid.pixel_aspect.1)
        {
            match directional_plane::compensate(layer, comp, size, &result.transform, &mut result.effects) {
                Ok(true) => self.warn(
                    Limitation::Properties,
                    Some(comp_id),
                    Some(layer_id),
                    "Isolated static Directional Blur direction and length transformed from native Solid source-plane controls into FX screen-space controls; sampling kernels, mixed/nested/animated transforms and native export fidelity remain unverified".into(),
                ),
                Ok(false) => {}
                Err(error) => self.warn(
                    Limitation::Properties,
                    Some(comp_id),
                    Some(layer_id),
                    format!("Directional Blur source-plane controls retained without compensation: {error}"),
                ),
            }
        }
        if purpose == LayerPurpose::MatteSample(set_matte::MatteSampleStage::Source) {
            // The occurrence's destination blend is not part of its source pixels.
            // Nested source layers were imported with their own ordinary purpose.
            result.blend_mode = fx_schema::BlendMode::Normal;
            result.transform.opacity =
                fx_schema::PercentageProperty::new(100.0).expect("100 is a valid opacity");
            self.warn(
                Limitation::TrackMatte,
                Some(comp_id),
                Some(layer_id),
                "Source-stage Set Matte sampling excludes provider owner opacity and occurrence blend; the independent helper uses static 100% owner opacity and Normal outer blend while source content, inner blends and paint opacity remain unchanged"
                    .into(),
            );
        }
        let animation_denials = self.animation_budget.denials();
        let convert_transform = |budget: &mut animation_budget::AnimationBudget| {
            let (mut entries, mut animation_warnings) = if correction.is_identity() {
                animation::transform_entries_with_sources(
                    layer,
                    comp_id,
                    comp,
                    &self.items,
                    id,
                    animation::AnimationTargetClock::ParentIdentity,
                    anchor_scale,
                    budget,
                )
            } else {
                camera_normalization::corrected_transform_entries(
                    layer,
                    comp,
                    id,
                    correction,
                    anchor_scale,
                    budget,
                )
            };
            if correction.is_identity() {
                let (evaluated, expression_warnings) = animation::evaluated_transform_entries(
                    expression_samples,
                    (comp_id, comp),
                    layer,
                    id,
                    anchor_scale,
                    true,
                    budget,
                );
                entries.retain(|entry| {
                    !evaluated
                        .iter()
                        .any(|replacement| replacement.target == entry.target)
                });
                entries.extend(evaluated);
                suppress_replaced_expression_warnings(
                    &mut animation_warnings,
                    &expression_warnings,
                );
                animation_warnings.extend(expression_warnings);
            } else if expression_samples.has_transform_layer(comp_id, layer_id) {
                animation_warnings.push("evaluated Transform expressions cannot be combined with generated-camera normalization; evaluated values omitted".into());
            }
            (entries, animation_warnings)
        };
        let filtered_transform = match purpose {
            LayerPurpose::Adjustment => Some((true, "Adjustment retained opacity")),
            LayerPurpose::MatteSample(set_matte::MatteSampleStage::Source) => {
                Some((false, "Source-stage Set Matte retained Transform"))
            }
            _ => None,
        };
        let (entries, animation_warnings) = if let Some((retain_opacity, label)) =
            filtered_transform
        {
            // Generate against an isolated bounded probe so tracks discarded by
            // the purpose-specific sampling policy cannot deny retained tracks.
            // Charge only actual retained entries, including final reminted IDs.
            let mut probe_budget = animation_budget::AnimationBudget::default();
            let (candidates, warnings) = convert_transform(&mut probe_budget);
            let mut entries = Vec::new();
            let mut accounting_warnings = Vec::new();
            for entry in candidates.into_iter().filter(|entry| {
                let owner_opacity = entry.target.as_property().is_some_and(|property| {
                    property.layer_id() == id
                        && property.property_type() == fx_schema::PropType::Opacity
                });
                owner_opacity == retain_opacity
            }) {
                match animation_budget::committed_entry_reservation_bytes(&entry)
                    .and_then(|bytes| self.animation_budget.reserve(bytes))
                {
                    Ok(()) => entries.push(entry),
                    Err(animation_budget::ReservationError::Exhausted { .. }) => {}
                    Err(error) => accounting_warnings.push(format!(
                        "{label} animation reservation could not be measured exactly: {error}; track omitted"
                    )),
                }
            }
            (
                entries,
                warnings.into_iter().chain(accounting_warnings).collect(),
            )
        } else {
            convert_transform(&mut self.animation_budget)
        };
        self.animations.extend(entries);
        self.warn_animation_denial(animation_denials, comp_id, layer_id, "Transform");
        for message in warnings.into_iter().chain(animation_warnings) {
            self.warn(
                Limitation::Properties,
                Some(comp_id),
                Some(layer_id),
                message,
            );
        }
        let mask_size = if size.contains(&0) {
            [u32::from(comp.width.max(1)), u32::from(comp.height.max(1))]
        } else {
            size.map(u32::from)
        };
        let animation_denials = self.animation_budget.denials();
        let mask_import = if purpose.includes_occurrence_pipeline() {
            masks::apply(
                layer,
                (comp_id, expression_samples),
                &mut result,
                mask_size,
                &mut self.next_id,
                &mut self.animation_budget,
            )
        } else {
            masks::MaskImport {
                animations: Vec::new(),
                guide_ids: Vec::new(),
                warnings: Vec::new(),
            }
        };
        let mask_supported = mask_import.warnings.is_empty();
        self.animations.extend(mask_import.animations);
        self.warn_animation_denial(animation_denials, comp_id, layer_id, "Mask");
        for message in mask_import.warnings {
            self.warn(
                Limitation::Properties,
                Some(comp_id),
                Some(layer_id),
                message,
            );
        }
        if purpose.includes_occurrence_pipeline() {
            let supported_plane = content_depth < MAX_GROUP_DEPTH
                && ancestors
                    .iter()
                    .all(|parent| !parent.record.flags().three_d_layer);
            match radial_wipe::apply(layer, comp, source, &mut result, &mut self.next_id, &mut self.animation_budget, supported_plane) {
                Ok(Some(entries)) => {
                    self.animations.extend(entries);
                    self.warn(Limitation::Properties, Some(comp_id), Some(layer_id), "Radial Wipe: bounded 50% zero-feather profile lowered to an independent editable half-plane guide and Rotation keys; live controller linkage and exact antialiasing are not retained".into());
                }
                Ok(None) => {}
                Err(message) => self.warn(Limitation::Properties, Some(comp_id), Some(layer_id), format!("Radial Wipe omitted: {message}; original content and other effects retained, expression fallback not used")),
            }
        }
        if record.layer_type() != 4 && size.contains(&0) && !result.masks.is_empty() {
            self.warn(Limitation::Properties, Some(comp_id), Some(layer_id),
                "mask normalization uses composition dimensions because source-local dimensions are unavailable; source-local mask alignment is approximate".into());
        }
        // A light/environment or future layer may reference a comp as an input
        // without being a visible precomp occurrence. Only AV layers expand it.
        if flags.null_layer || flags.adjustment_layer {
            // These carriers have no source pixels. The direct Adjustment lowering
            // retains sibling-stack effects and reports any actual limitations.
        } else if let Some(source) = source.filter(|source| {
            record.layer_type() == 0 && matches!(source.kind, ItemKind::Composition(_))
        }) {
            let ItemKind::Composition(source_comp) = &source.kind else {
                unreachable!()
            };
            let overrides = crate::essential::overrides(
                &layer.content,
                &source_comp.essential_properties.values,
            );
            for warning in overrides.warnings {
                self.warn(
                    Limitation::Properties,
                    Some(comp_id),
                    Some(layer_id),
                    format!("Essential Property {:?}: {}", warning.kind, warning.message),
                );
            }
            // Enclosing occurrence overrides take precedence over nested source defaults.
            let enclosing = std::mem::replace(&mut self.overrides, overrides.values);
            self.overrides.extend(enclosing.iter().cloned());
            content.layers = stored_layers(self.composition_layers(
                source,
                content.id,
                content_depth + usize::from(clip.is_some()),
            )?)?;
            self.overrides = enclosing;
        } else if let Some(solid) = source
            .and_then(|source| source.solid.as_ref())
            .filter(|_| record.layer_type() == 0 && !flags.null_layer && !flags.adjustment_layer)
        {
            match solid {
                Ok(solid) => {
                    if solid.pixel_aspect.0 != solid.pixel_aspect.1 {
                        self.warn(Limitation::Properties, Some(comp_id), Some(layer_id), format!("solid pixel aspect {:?} replaced by square pixels", solid.pixel_aspect));
                    }
                    result.description = format!("AEP comp={comp_id} layer={layer_id} kind=0 source={source_id} transformParent={}; editable solid with occurrence Transform and separate source clock; other properties best-effort", record.parent_id());
                    let rect = transform::solid_rect(solid, &content, self.allocate_id()?, content.transform);
                    content.layers.push(fx_schema::Layer::from_data(&FxLayer::Rect(rect))?);
                }
                Err(error) => self.warn(Limitation::Placeholder, Some(comp_id), Some(layer_id), format!("solid source cannot be decoded: {error}; non-rendering placeholder retained")),
            }
        } else if let Some(source) =
            source.filter(|source| record.layer_type() == 0 && source.media.is_some())
        {
            let animation_denials = self.animation_budget.denials();
            match media::convert_resolved(
                source,
                layer,
                &content,
                media::MediaImportOptions {
                    next_id: self.next_id,
                    frame_blending_enabled: comp.record.flags()[1] & 16 != 0,
                    asset_namespace: self.asset_namespace,
                },
                &mut self.media_resolver,
                &mut self.animation_budget,
                &mut self.shape_budget,
            ) {
                Ok(imported) => {
                    let limitation = if imported.layers.is_empty() { Limitation::Placeholder } else { Limitation::Properties };
                    self.next_id = imported.next_id;
                    self.animations.extend(imported.animations);
                    content.layers.extend(stored_layers(imported.layers)?);
                    self.assets.extend(imported.assets);
                    for message in imported.warnings {
                        self.warn(limitation, Some(comp_id), Some(layer_id), message);
                    }
                    self.warn_animation_denial(
                        animation_denials,
                        comp_id,
                        layer_id,
                        "Audio Levels",
                    );
                }
                Err(error) => self.warn(Limitation::Placeholder, Some(comp_id), Some(layer_id),
                    format!("media values cannot form a valid editable layer: {error}; empty carrier retained")),
            }
        } else if record.layer_type() == 4 {
            let animation_denials = self.animation_budget.denials();
            let imported = shapes::import_with_evaluations(
                layer,
                comp,
                (comp_id, expression_samples),
                &self.items,
                purpose.includes_occurrence_pipeline(),
                &content,
                MAX_GROUP_DEPTH.saturating_sub(content_depth + usize::from(clip.is_some())),
                &mut self.next_id,
                &mut self.shape_budget,
                &mut self.animation_budget,
            )?;
            shape_lowered_fade = imported.frame_fade_lowered;
            self.mapped_shape_expressions.extend(
                imported
                    .mapped_expressions
                    .into_iter()
                    .map(|identity| (comp_id, layer_id, identity)),
            );
            content.layers.extend(stored_layers(imported.layers)?);
            self.animations.extend(imported.animations);
            self.warn_animation_denial(animation_denials, comp_id, layer_id, "Shape contents");
            for message in imported.warnings {
                self.warn(
                    Limitation::Properties,
                    Some(comp_id),
                    Some(layer_id),
                    message,
                );
            }
        } else if record.layer_type() == 3 {
            let animation_denials = self.animation_budget.denials();
            let mut imported = text::import_in_composition(
                layer,
                context.comp,
                Some((comp_id, expression_samples)),
                &content,
                &mask_import.guide_ids,
                &mut self.next_id,
                &mut self.animation_budget,
            );
            if let Some(value) = self.text_overrides.get(&(comp_id, layer_id)) {
                let count = imported
                    .layers
                    .iter()
                    .filter(|layer| matches!(layer, FxLayer::Text(_)))
                    .count();
                if count == 1 {
                    for layer in &mut imported.layers {
                        if let FxLayer::Text(text) = layer {
                            text.source_text = value.document.clone();
                        }
                    }
                } else {
                    imported.warnings.push(format!("controller {} saved Text override could not bind one editable Text paint; template content/animation retained", value.controller_uuid));
                }
            }
            self.animations.extend(imported.animations);
            self.warn_animation_denial(animation_denials, comp_id, layer_id, "Text animation");
            content.layers.extend(stored_layers(imported.layers)?);
            for message in imported.warnings {
                self.warn(
                    Limitation::Properties,
                    Some(comp_id),
                    Some(layer_id),
                    message,
                );
            }
        } else {
            self.warn(Limitation::Placeholder, Some(comp_id), Some(layer_id), format!("layer kind {} / source {source_id} represented by a named non-rendering Group; unsupported solid/media/text/shape/light/camera/model/mesh content is not rendered or flattened", record.layer_type()));
            if source_id != 0 && source.is_none() {
                self.warn(
                    Limitation::MissingReference,
                    Some(comp_id),
                    Some(layer_id),
                    format!("source item {source_id} absent; structural placeholder retained"),
                );
            }
            if let Some(ProjectItem {
                kind: ItemKind::Unknown(kind),
                ..
            }) = source
            {
                self.warn(
                    Limitation::UnknownItem,
                    Some(comp_id),
                    Some(layer_id),
                    format!("source item kind {kind} has no FX content mapping"),
                );
            }
        }
        // Complete effect-generated source paint before any fallback hiding.
        if let Some(generator) = basic_text {
            let text = generator.into_layer(self.allocate_id()?, &content);
            // FX siblings are topmost-first. Composite On Original was admitted
            // explicitly, and the generator is the first native effect stage.
            content
                .layers
                .insert(0, fx_schema::Layer::from_data(&text)?);
        }
        // Unsegmented footage is not a neutral fallback: it is opaque where AE
        // keeps only the foreground. Hidden paint keeps its editable source.
        if unsupported_cutout && contributes {
            self.unavailable_cutouts += 1;
            if !purpose.samples_matte() {
                hide_visual_layers(&mut content.layers)?;
                self.warn(Limitation::Properties, Some(comp_id), Some(layer_id), "Roto Brush (ADBE Samurai) segmentation has no FX equivalent; this occurrence's unsegmented source is hidden so it does not cover lower layers. Its transform, masks, effects, audio, children and siblings are retained; the isolated foreground and its occlusion are lost. Unhide the source to see the raw frame".into());
            }
        }
        if unsupported_sweep_cutout && contributes && !purpose.samples_matte() {
            hide_visual_layers(&mut content.layers)?;
            self.warn(Limitation::Properties, Some(comp_id), Some(layer_id), "CC Light Sweep static Cutout reception has no current FX counterpart; this occurrence's raw source paint is hidden so the omitted cutout does not become an opaque cover over lower siblings. Editable source, owner controls and other effects are retained. Sweep pixels and suffix effects on those pixels are lost; Add, Composite, disabled, dynamic reception and nondefault compositing are not hidden. Alpha/render fidelity and export restoration are unverified".into());
        }
        // Unless a committed caption already lowered the preset onto its paint
        // Group, the owner lowers it or reports why not.
        if !shape_lowered_fade {
            match frame_fade {
                Ok(Some(fade)) => self.lower_frame_fade(fade, (comp_id, comp), layer, &result),
                Ok(None) => {}
                Err(message) => self.warn(
                    Limitation::Properties,
                    Some(comp_id),
                    Some(layer_id),
                    message,
                ),
            }
        }
        let source_depth = content_depth + usize::from(clip.is_some());
        let mut source_content = match clip {
            Some(mut clip) => {
                clip.layers
                    .push(fx_schema::Layer::from_data(&FxLayer::Group(content))?);
                clip
            }
            None => content,
        };
        if still_image && purpose.includes_occurrence_pipeline() {
            source_content = self.still_geometry2(
                (comp_id, layer),
                size,
                &result,
                source_content,
                source_depth,
            )?;
        }
        result.layers.insert(
            0,
            fx_schema::Layer::from_data(&FxLayer::Group(source_content))?,
        );
        if !flags.audio_enabled {
            let mut muted = HashSet::new();
            mute_audio(&mut result, &mut muted)?;
            if !muted.is_empty()
                && let Err(error) = self.discard_animations(|entry| muted.contains(&entry.target))
            {
                self.warn(
                    Limitation::ExpansionLimit,
                    Some(comp_id),
                    Some(layer_id),
                    format!(
                        "discarded muted-audio animation could not release its exact reservation: {error}; the allowance remains conservatively charged"
                    ),
                );
            }
        }
        if !purpose.samples_matte() && !flags.enabled {
            hide_visuals(&mut result)?;
            if !has_enabled_audio(&result) {
                result.is_hidden = true;
            }
        }
        if purpose.includes_occurrence_pipeline() {
            let before = result.effects.len();
            match mirror::apply(
                layer,
                &mut result,
                mirror::Context {
                    size: native_effect_size,
                    ordinals: &native_effect_ordinals,
                    depth: depth + ancestors.len(),
                    other_stages: !fractal_blends.is_empty(),
                },
                mirror::State {
                    next: &mut self.next_id,
                    entries: &mut self.animations,
                    animations: &mut self.animation_budget,
                    shapes: &mut self.shape_budget,
                },
            ) {
                Ok(true) => {
                    native_effect_ordinals.drain(..before - result.effects.len());
                    self.warn(Limitation::Properties,Some(comp_id),Some(layer_id),"Mirror approximated by two editable pre-effect vector copies, clipped to the retained half-plane, with one reflected about the native center/angle. Later effects and owner opacity remain outside once. Native projection, clipping/edge antialiasing and transformed-owner pixels remain uncalibrated; edits to the two copies are independent. Export writes the current graph, not native Mirror replay".into());
                }
                Ok(false) => {}
                Err(error) => self.warn(
                    Limitation::Properties,
                    Some(comp_id),
                    Some(layer_id),
                    format!("Mirror not lowered: {error}; owner and convertible siblings retained"),
                ),
            }
        }
        let wipe_ancestors = if record.layer_type() == 4 && source.is_none() {
            ancestors
                .iter()
                .map(|&parent| {
                    let parent_source = self.items.get(&parent.record.source_id()).copied();
                    let parent_size = source_dimensions(parent_source, parent);
                    let parent_anchor_dimensions = source_anchor_dimensions(parent_source, parent);
                    let (mut transform, mut warnings) = transform::static_transform_with_sources(
                        parent,
                        parent_size,
                        comp_id,
                        comp,
                        &self.items,
                    );
                    if parent_source.is_some_and(has_source_relative_anchor) {
                        normalize_static_solid_anchor(
                            parent,
                            parent_anchor_dimensions,
                            &mut transform,
                            &mut warnings,
                        );
                    }
                    transform.opacity =
                        fx_schema::PercentageProperty::new(100.0).expect("100 is valid opacity");
                    linear_wipe::AncestorTransform {
                        layer: parent,
                        transform,
                    }
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let mut linear_wipe_lowered = false;
        if purpose.includes_occurrence_pipeline() {
            match linear_wipe::apply(
                layer,
                &mut result,
                linear_wipe::Context {
                    source,
                    size: native_effect_size,
                    depth: depth + ancestors.len(),
                    planar: ancestors
                        .iter()
                        .all(|parent| !parent.record.flags().three_d_layer),
                    composition_id: comp_id,
                    composition_offset,
                    ancestors: &wipe_ancestors,
                    evaluations: expression_samples,
                },
                linear_wipe::State {
                    next: &mut self.next_id,
                    animations: &mut self.animation_budget,
                    shapes: &mut self.shape_budget,
                },
            ) {
                Ok(Some(lowered)) => {
                    linear_wipe_lowered = lowered.consumed_trailing_transform;
                    self.animations.extend(lowered.entries);
                    self.warn(Limitation::Properties,Some(comp_id),Some(layer_id),"hard Linear Wipes on a finite Solid/composition source plane or a statically inverse-mapped continuous-rasterized Shape composition plane approximated by editable projected half-plane masks and native or source-frame-evaluated Completion tracks. Optional source-normalized trailing Geometry2 Anchor tracks apply only to source-local planes. Same-layer aliases become independent values/keys; owner Transform/styles and source content clocks retain their authored stages. Native angled completion normalization and antialiasing are unverified; feather, dynamic/unrepresentable Shape planes and nonadjacent mixed effect stages remain unsupported".into());
                    for note in lowered.notes {
                        self.warn(
                            Limitation::Properties,
                            Some(comp_id),
                            Some(layer_id),
                            note,
                        );
                    }
                }
                Ok(None) => {}
                Err(error) => self.warn(
                    Limitation::Properties,
                    Some(comp_id),
                    Some(layer_id),
                    format!(
                        "Linear Wipe source profile not lowered: {error}; original owner and omission diagnostics retained"
                    ),
                ),
            }
        }
        if purpose.includes_occurrence_pipeline() {
            let before = result.effects.len();
            let fractal_hidden = timing.hide_content || result.is_hidden;
            match fractal_blend::apply(&mut result,&fractal_blends,fractal_blend::Context {ordinals:&native_effect_ordinals,size:native_effect_size,visibility:fractal_blend::Visibility {range,hidden:fractal_hidden},parent_depth:depth+ancestors.len()},fractal_blend::State {next:&mut self.next_id,budget:&mut self.shape_budget,animation_budget:&mut self.animation_budget,animations:&mut self.animations}) {
                Ok(true) => {
                    native_effect_ordinals.drain(..before-result.effects.len());
                    self.warn(Limitation::Properties,Some(comp_id),Some(layer_id),"Basic Fractal Multiply/Screen stages with bounded numeric keys approximated by independent opaque TurbulentNoise generators on the finite source plane and source-over blend Groups. Imported prefix/suffix order, generator identities, opacity and native visibility are retained; unsupported prior/later spatial stages remain omitted. Native noise kernel, scale calibration, HDR overflow, evolution and edge/raster behavior differ; Transform parity is not established. Independent generator effect-only disabling leaves opaque carrier paint and can change transparent prefix alpha; disable generator Group for full bypass".into());
                }
                Ok(false) => {},
                Err(error) => self.warn(Limitation::Properties,Some(comp_id),Some(layer_id),format!("Fractal blend stages not lowered: {error}; original owner retained and staged generators omitted")),
            }
        }
        if purpose.includes_occurrence_pipeline() {
            match alpha_projection::apply(layer,&mut result,native_effect_size,&native_effect_ordinals,depth,alpha_projection::PaintState {next: &mut self.next_id,animations: &mut self.animations,budget: &mut self.animation_budget}) {
                Ok(true) => self.warn(Limitation::Properties,Some(comp_id),Some(layer_id),"Solid Composite black → Shift Channels Lightness alpha → Remove Color Matting black: approximated by an editable opaque-black source provider and white Luma-matted output; prefix effects remain on source content before the black backing, suffix effects/styles and owner Transform remain outside. Admission requires grayscale paint or a full-strength grayscale Tint; arbitrary media and colored paint are declined. Ordinary Text paint does not prove color-font raster neutrality; color glyphs, native Lightness and unmatting edge alpha remain unverified".into()),
                Ok(false) => {},
                Err(error) => self.warn(Limitation::Properties,Some(comp_id),Some(layer_id),format!("Lightness alpha pipeline not lowered: {error}; original owner/effects retained")),
            }
        }
        if purpose.includes_occurrence_pipeline() {
            match vegas::apply(layer,&mut result,vegas::Context{size:native_effect_size,ordinals:&native_effect_ordinals,depth:depth+ancestors.len(),animations:&self.animations},vegas::State{next:&mut self.next_id,budget:&mut self.shape_budget}) {
                Ok(true) => self.warn(Limitation::Properties,Some(comp_id),Some(layer_id),"APC Vegas Transparent self-input Intensity Image Contours: approximated by a shared editable source provider, finite one-code-value LumaKey threshold, and centered SimpleChoker alpha ring. Native segments, animated sweep/rotation, contour tolerance, hardness and opacity gradient are omitted; native unpremultiplied Intensity, edge alpha and color-font behavior can differ. Prefix effects and source tracks remain editable; suffix effects/styles and owner Transform remain outside".into()),
                Ok(false) => {},
                Err(error) => self.warn(Limitation::Properties,Some(comp_id),Some(layer_id),format!("Vegas contour not lowered: {error}; original owner/effects retained")),
            }
        }
        let mut inverse_matte_lowered = false;
        if matches!(
            purpose,
            LayerPurpose::Ordinary
                | LayerPurpose::MatteSample(set_matte::MatteSampleStage::AllEffects)
        ) {
            match self_inverse_matte::apply(
                layer,
                self_inverse_matte::Context {
                    source,
                    items: &self.items,
                    depth,
                },
                &mut result,
                &mut self.animations,
                &mut self.next_id,
                mask_supported,
                &mut self.animation_budget,
            ) {
                Ok(true) => {
                    inverse_matte_lowered = true;
                    self.warn(Limitation::TrackMatte, Some(comp_id), Some(layer_id), "Self-inverse Set Matte: bounded opaque solid mask stage approximated with editable mask, shadow and inverse-mask Groups, plus source-declared centered post-effect Transform when present; native source stage -2 semantics and effect pixels remain approximate; omitted effects retain their diagnostics".into());
                }
                Ok(false) => {}
                Err(error) => self.warn(
                    Limitation::TrackMatte,
                    Some(comp_id),
                    Some(layer_id),
                    format!("Self-inverse Set Matte not lowered: {error}; original owner retained"),
                ),
            }
        }
        if matches!(
            purpose,
            LayerPurpose::Ordinary
                | LayerPurpose::MatteSample(set_matte::MatteSampleStage::AllEffects)
        ) {
            match foreign_inverse_matte::apply(
                foreign_inverse_matte::Context {
                    comp,
                    items: &self.items,
                    depth,
                },
                layer,
                &mut result,
                &mut self.animations,
                &mut self.next_id,
                &mut self.animation_budget,
            ) {
                Ok(Some(entries)) => {
                    inverse_matte_lowered = true;
                    self.animations.extend(entries);
                    self.warn(Limitation::TrackMatte, Some(comp_id), Some(layer_id), "Foreign Set Matte: exact one-hop mask path alias copied independently or equal native authored path/timing controls proved; editable mask/shadow/inverse/Glow/legacy centered Transform stages approximate native stage -2 and effect-buffer pixels; later edits are not linked".into());
                }
                Ok(None) => {}
                Err(error) => self.warn(
                    Limitation::TrackMatte,
                    Some(comp_id),
                    Some(layer_id),
                    format!(
                        "Foreign Set Matte not lowered: {error}; original cached owner retained"
                    ),
                ),
            }
        }
        // Undrawn paint adds no alpha to an enclosing matte sample, so cutouts
        // converted inside it cannot make that sample unavailable.
        if !contributes {
            self.unavailable_cutouts = cutouts_on_entry;
        }
        let geometry = if purpose.includes_occurrence_pipeline()
            && !still_image
            && !inverse_matte_lowered
            && !linear_wipe_lowered
        {
            geometry2::prepare(
                layer,
                comp,
                &self.items,
                &result,
                ancestors
                    .iter()
                    .all(|parent| !parent.record.flags().three_d_layer),
            )
            .and_then(|prepared| {
                if prepared.is_some() && content_depth >= MAX_GROUP_DEPTH {
                    Err("post-layer stage exceeds the converter group depth".into())
                } else {
                    Ok(prepared)
                }
            })
        } else {
            Ok(None)
        };
        if purpose.includes_occurrence_pipeline() {
            match twirl_plane::stage(layer, comp, &mut result, &mut self.next_id, self.stack.len() == 1 && ancestors.is_empty(), self.linked) {
                Ok(true) => self.warn(Limitation::Properties, Some(comp_id), Some(layer_id), "Twirl source-local image staged before static owner Transform using a late full-composition CornerPin; source controls, keys and clock remain editable. Native kernel, radius falloff and clipped frame edges remain approximate".into()),
                Ok(false) => {},
                Err(message) => self.warn(Limitation::Properties, Some(comp_id), Some(layer_id), message),
            }
        }
        let mut result = self.transform_parents(context, layer, ancestors, result)?;
        match geometry {
            Ok(Some(prepared)) => {
                let mut candidate_id = self.next_id;
                let applied = reserve_ids(&mut candidate_id, 1)
                    .ok_or_else(|| "generated layer budget exhausted".to_owned())
                    .and_then(|id| {
                        prepared.apply(&mut result, LayerId::new(id), &mut self.animation_budget)
                    });
                match applied {
                    Ok(entries) => {
                        self.next_id = candidate_id;
                        self.animations.extend(entries);
                        self.warn(Limitation::Properties, Some(comp_id), Some(layer_id),
                            "Geometry2: sole planar Shape Transform effect lowered after native layer/parent transforms as an editable Group; exact supported point expressions become independent sparse keys, validated on a 1ms composition grid within 0.01 units. Raster interpolation/clipping and shutter behavior remain approximate; live control linkage is not retained".into());
                    }
                    Err(error) => self.warn(
                        Limitation::Properties,
                        Some(comp_id),
                        Some(layer_id),
                        format!("Geometry2: {error}; effect omitted, owner retained"),
                    ),
                }
            }
            Ok(None) => {}
            Err(error) => self.warn(
                Limitation::Properties,
                Some(comp_id),
                Some(layer_id),
                format!("Geometry2: {error}; effect omitted, owner retained"),
            ),
        }
        Ok((result, adjustment_opacity))
    }

    /// Puts a still's Geometry2 between its owner and source content, or
    /// returns the content unchanged with a diagnosed omission.
    fn still_geometry2(
        &mut self,
        (comp_id, layer): (u32, &Layer),
        size: [u16; 2],
        owner: &GroupLayer,
        content: GroupLayer,
        content_depth: usize,
    ) -> Result<GroupLayer, DocumentError> {
        let layer_id = layer.record.id();
        let prepared = geometry2::prepare_still(layer, size, owner).and_then(|prepared| {
            if prepared.is_some() && content_depth + 1 >= MAX_GROUP_DEPTH {
                Err("the source-plane stage exceeds the converter group depth".into())
            } else {
                Ok(prepared)
            }
        });
        let stage = match prepared {
            Ok(Some(stage)) => stage,
            Ok(None) => return Ok(content),
            Err(error) => {
                self.warn(
                    Limitation::Properties,
                    Some(comp_id),
                    Some(layer_id),
                    format!("Geometry2: {error}; effect omitted, owner retained"),
                );
                return Ok(content);
            }
        };
        let mut candidate_id = self.next_id;
        let entries = reserve_ids(&mut candidate_id, 1)
            .ok_or_else(|| "generated layer budget exhausted".to_owned())
            .and_then(|id| {
                let id = LayerId::new(id);
                stage
                    .entries(id, layer, &mut self.animation_budget)
                    .map(|entries| (id, entries))
            });
        let (id, (entries, warnings)) = match entries {
            Ok(prepared) => prepared,
            Err(error) => {
                self.warn(
                    Limitation::Properties,
                    Some(comp_id),
                    Some(layer_id),
                    format!("Geometry2: {error}; effect omitted, owner retained"),
                );
                return Ok(content);
            }
        };
        self.next_id = candidate_id;
        self.animations.extend(entries);
        for message in warnings {
            self.warn(
                Limitation::Properties,
                Some(comp_id),
                Some(layer_id),
                format!("Geometry2: {message}"),
            );
        }
        self.warn(Limitation::Properties, Some(comp_id), Some(layer_id),
            "Geometry2: Transform on a still image lowered as an editable Group on the source image plane, before the owner Transform; native point keys and eases are retained. Raster sampling and motion-blur shutter remain approximate; live control linkage is not retained".into());
        Ok(stage.wrap(id, owner.id, content)?)
    }

    /// The fade multiplies the effect image before the native Composite, which
    /// FX owner Opacity reproduces after the owner's own effects.
    fn lower_frame_fade(
        &mut self,
        fade: effects::FrameFade,
        (comp_id, comp): (u32, &Composition),
        layer: &Layer,
        owner: &GroupLayer,
    ) {
        let layer_id = layer.record.id();
        let target = fx_schema::PropertyTarget::layer(owner.id, fx_schema::PropType::Opacity);
        let denials = self.animation_budget.denials();
        let lowered = if self.animations.iter().any(|entry| entry.target == target) {
            Err("owner Opacity is already animated".to_owned())
        } else {
            fade.opacity(layer, comp.frame_rate, owner.transform.opacity.value())
                .map_err(str::to_owned)
                .and_then(|keys| {
                    let (entries, warnings) = animation::numeric_entries(
                        "ADBE Solid Composite-0001",
                        &keys,
                        &[animation::NumericAnimationTarget::float(target, 0, 1.0)],
                        animation::NumericAnimationClock::parent_identity(layer)?,
                        &mut self.animation_budget,
                    );
                    if entries.is_empty() {
                        Err(warnings.join("; "))
                    } else {
                        Ok(entries)
                    }
                })
        };
        match lowered {
            Ok(entries) => {
                self.animations.extend(entries);
                self.warn(Limitation::Properties, Some(comp_id), Some(layer_id), "Fade In+Out - frames: the preset Solid Composite Source Opacity expression is lowered once to linear owner Opacity keys from the layer inPoint; later frame-control edits do not change them".into());
            }
            Err(reason) => {
                self.warn_animation_denial(denials, comp_id, layer_id, "Fade In+Out - frames");
                self.warn(Limitation::Properties, Some(comp_id), Some(layer_id), format!("Fade In+Out - frames: frame fade not lowered ({reason}); Effect ADBE CM FadeInOutFrames and Effect ADBE Solid Composite omitted, owner retained"));
            }
        }
    }

    fn transform_ancestors<'a>(
        &mut self,
        context: &LayerContext<'a>,
        layer: &Layer,
    ) -> Vec<&'a Layer> {
        let mut ancestors = Vec::new();
        let mut parent_id = layer.record.parent_id();
        let mut seen = HashSet::from([layer.record.id()]);
        while parent_id != 0 {
            if !seen.insert(parent_id) || ancestors.len() + context.depth + 2 >= MAX_GROUP_DEPTH {
                self.warn(
                    Limitation::Parenting,
                    Some(context.comp_id),
                    Some(layer.record.id()),
                    "cyclic or over-depth transform-parent chain omitted".into(),
                );
                return Vec::new();
            }
            let Some(&parent_index) = context.layer_indices.get(&parent_id) else {
                self.warn(
                    Limitation::Parenting,
                    Some(context.comp_id),
                    Some(layer.record.id()),
                    format!("missing transform parent {parent_id}; known part of chain retained"),
                );
                break;
            };
            let parent = &context.comp.layers[parent_index];
            ancestors.push(parent);
            parent_id = parent.record.parent_id();
        }
        ancestors
    }

    fn transform_parents(
        &mut self,
        context: &LayerContext<'_>,
        layer: &Layer,
        ancestors: Vec<&Layer>,
        mut result: GroupLayer,
    ) -> Result<GroupLayer, DocumentError> {
        if ancestors.is_empty() {
            return Ok(result);
        }
        let blend_mode = result.blend_mode;
        for parent in ancestors {
            let source = self.items.get(&parent.record.source_id()).copied();
            let size = source_dimensions(source, parent);
            let anchor_dimensions = source_anchor_dimensions(source, parent);
            let (mut transform, mut warnings) = transform::static_transform_with_sources(
                parent,
                size,
                context.comp_id,
                context.comp,
                &self.items,
            );
            let anchor_scale = solid_anchor_scale(source, anchor_dimensions);
            if source.is_some_and(has_source_relative_anchor) {
                normalize_static_solid_anchor(
                    parent,
                    anchor_dimensions,
                    &mut transform,
                    &mut warnings,
                );
            }
            transform.opacity =
                fx_schema::PercentageProperty::new(100.0).expect("100 is a valid opacity");
            let wrapper_id = self.allocate_id()?;
            let animation_denials = self.animation_budget.denials();
            let (entries, mut animation_warnings) =
                animation::transform_parent_entries_with_sources(
                    parent,
                    context.comp_id,
                    context.comp,
                    &self.items,
                    wrapper_id,
                    anchor_scale,
                    &mut self.animation_budget,
                );
            self.animations.extend(entries);
            let (entries, expression_warnings) = animation::evaluated_transform_entries(
                context.expression_samples,
                (context.comp_id, context.comp),
                parent,
                wrapper_id,
                anchor_scale,
                false,
                &mut self.animation_budget,
            );
            self.animations.retain(|entry| {
                !entries
                    .iter()
                    .any(|replacement| replacement.target == entry.target)
            });
            self.animations.extend(entries);
            suppress_replaced_expression_warnings(&mut animation_warnings, &expression_warnings);
            warnings.extend(expression_warnings);
            self.warn_animation_denial(
                animation_denials,
                context.comp_id,
                layer.record.id(),
                &format!("transform parent {}", parent.record.id()),
            );
            for message in warnings.into_iter().chain(animation_warnings) {
                self.warn(
                    Limitation::Parenting,
                    Some(context.comp_id),
                    Some(layer.record.id()),
                    format!("transform parent {}: {message}", parent.record.id()),
                );
            }
            if parent.record.flags().three_d_layer {
                self.warn(Limitation::Parenting, Some(context.comp_id), Some(layer.record.id()), format!("3D parent {} uses existing independently projected Group transforms, not a combined pre-projection 3D hierarchy", parent.record.id()));
            }
            let mut wrapper = group(
                wrapper_id,
                result.name.clone(),
                result.parent,
                TimeRangeProperty::new(Time::ZERO, Duration::from_secs(MAX_TIME_SECS)),
            );
            wrapper.transform = transform;
            wrapper.description = format!(
                "AEP comp={} layer={} transform-only parent copy {}; no inherited opacity, lifetime or visibility",
                context.comp_id,
                layer.record.id(),
                parent.record.id()
            );
            result.parent = Some(wrapper_id);
            result.blend_mode = Default::default();
            wrapper
                .layers
                .push(fx_schema::Layer::from_data(&FxLayer::Group(result))?);
            wrapper.blend_mode = blend_mode;
            result = wrapper;
        }
        self.warn(Limitation::Parenting, Some(context.comp_id), Some(layer.record.id()), "transform-only ancestor copies preserve stacking without inheriting opacity/visibility/timing; shared parent editing is not linked".into());
        Ok(result)
    }

    fn timing(
        &mut self,
        comp_id: u32,
        comp: &Composition,
        layer: &Layer,
        occurrence_id: LayerId,
        use_affine_source_clock: bool,
    ) -> ConvertedTiming {
        let record = &layer.record;
        let context = (Some(comp_id), Some(record.id()));
        let fallback_duration = self.duration(comp_id, comp.duration_secs);
        let inactive = || ConvertedTiming {
            active_range: TimeRangeProperty::new(Time::ZERO, fallback_duration),
            playback: None,
            hide_content: true,
        };
        let (Some(start), Some(input), Some(output), Some(stretch)) = (
            record.start_time(),
            record.in_point(),
            record.out_point(),
            record.stretch(),
        ) else {
            self.warn(
                Limitation::Timing,
                context.0,
                context.1,
                "invalid time denominator; editable source content retained hidden over the composition span".into(),
            );
            return inactive();
        };
        let in_comp = start + input * stretch;
        let out_comp = start + output * stretch;
        // Native bounds are source-local and ascend even when negative stretch
        // reverses their parent-time order. Adobe renders the pinned c30/c66
        // descending-bound form inactive, so do not sort it into visibility.
        if stretch == 0.0 || !in_comp.is_finite() || !out_comp.is_finite() || input >= output {
            if stretch < 0.0
                && input > output
                && in_comp.is_finite()
                && out_comp.is_finite()
                && in_comp.max(out_comp) <= MAX_TIME_SECS
            {
                let parent_begin = in_comp.min(out_comp).max(0.0);
                let parent_end = in_comp.max(out_comp);
                let parent_begin_time = Time::from_secs(parent_begin);
                let parent_end_time = Time::from_secs(parent_end);
                if parent_end > parent_begin && parent_end_time > parent_begin_time {
                    self.warn(
                        Limitation::Timing,
                        context.0,
                        context.1,
                        "negative stretch with non-ascending native source bounds is inactive; editable source content retained hidden over its positive parent span".into(),
                    );
                    return ConvertedTiming {
                        active_range: TimeRangeProperty::new(
                            parent_begin_time,
                            parent_end_time.saturating_sub(parent_begin_time),
                        ),
                        playback: None,
                        hide_content: true,
                    };
                }
            }
            self.warn(Limitation::Timing, context.0, context.1, format!("unrepresentable or inactive timing start={start}, in={input}, out={output}, stretch={stretch}; editable source content retained hidden over the composition span"));
            return inactive();
        }
        let (parent_begin, parent_end) = if stretch < 0.0 {
            (out_comp, in_comp)
        } else {
            (in_comp, out_comp)
        };
        if parent_end > MAX_TIME_SECS {
            self.warn(Limitation::Timing, context.0, context.1, format!("unrepresentable or inactive timing start={start}, in={input}, out={output}, stretch={stretch}; editable source content retained hidden over the composition span"));
            return inactive();
        }
        if parent_end <= 0.0 {
            self.warn(
                Limitation::Timing,
                context.0,
                context.1,
                "native parent span ends at or before composition zero; editable source content retained hidden over the composition span".into(),
            );
            return ConvertedTiming {
                active_range: TimeRangeProperty::new(Time::ZERO, fallback_duration),
                playback: None,
                hide_content: true,
            };
        }
        let parent_begin = parent_begin.max(0.0);
        let parent_begin_time = Time::from_secs(parent_begin);
        let parent_end_time = Time::from_secs(parent_end);
        if parent_end_time <= parent_begin_time {
            self.warn(
                Limitation::Timing,
                context.0,
                context.1,
                "positive native parent span rounds to an empty FX interval; editable source content retained hidden over the composition span".into(),
            );
            return ConvertedTiming {
                active_range: TimeRangeProperty::new(Time::ZERO, fallback_duration),
                playback: None,
                hide_content: true,
            };
        }
        let parent_range = TimeRangeProperty::new(
            parent_begin_time,
            parent_end_time.saturating_sub(parent_begin_time),
        );
        if !use_affine_source_clock {
            if in_comp < 0.0 || [parent_begin, out_comp].into_iter().any(fractional_millis) {
                self.warn(Limitation::Timing, context.0, context.1, "negative parent-time prefix clipped at zero and/or fractional-millisecond endpoints rounded; native parent span retained without an affine source clock".into());
            }
            return ConvertedTiming {
                active_range: parent_range,
                playback: None,
                hide_content: false,
            };
        }
        let mut begin = parent_begin;
        let mut end = parent_end;
        if stretch > 0.0 {
            begin = begin.max(start);
        } else {
            end = end.min(start);
        }
        let begin_time = Time::from_secs(begin);
        let end_time = Time::from_secs(end);
        if end_time <= begin_time {
            self.warn(
                Limitation::Timing,
                context.0,
                context.1,
                "nonnegative source clock has no positive-duration representable overlap; editable source content retained hidden over its positive parent span".into(),
            );
            return ConvertedTiming {
                active_range: parent_range,
                playback: None,
                hide_content: true,
            };
        }
        let source_begin = (begin - start) / stretch;
        let source_end = (end - start) / stretch;
        let range = TimeRangeProperty::new(begin_time, end_time.saturating_sub(begin_time));
        if in_comp.min(out_comp) < 0.0
            || end < parent_end
            || [begin, end, source_begin, source_end]
                .into_iter()
                .any(fractional_millis)
        {
            self.warn(Limitation::Timing, context.0, context.1, "parent/source nonnegative boundary clipped and/or fractional-millisecond endpoints rounded; source offset preserved where representable".into());
        }
        let source_begin_time = Time::from_secs(source_begin);
        // Rounding each source end on its own can turn an authored 1x clock
        // into a near-1x rate, which playback treats as a speed change. An
        // exact unit stretch keeps the rounded source origin and spans the
        // rounded parent window, so only that origin is approximated.
        let (numerator, denominator) = record.stretch_fraction();
        let exact_unit_stretch = denominator != 0 && u32::try_from(numerator) == Ok(denominator);
        let source_end_time = if exact_unit_stretch {
            source_begin_time.checked_add_duration(range.duration)
        } else {
            Some(Time::from_secs(source_end))
        };
        let representable =
            |value: f64| value.is_finite() && (0.0..=MAX_TIME_SECS).contains(&value);
        let Some(source_end_time) = source_end_time.filter(|&time| {
            representable(source_begin)
                && representable(source_end)
                && time <= Time::from_secs(MAX_TIME_SECS)
        }) else {
            self.warn(Limitation::Timing, context.0, context.1, "unrepresentable source clock retained as hidden editable content instead of substituting 1x playback".into());
            return ConvertedTiming {
                active_range: range,
                playback: None,
                hide_content: true,
            };
        };
        let key_data = [(begin_time, source_begin_time), (end_time, source_end_time)];
        let mut estimate = animation_budget::TimeRemapEstimate::default();
        for (index, (time, value)) in key_data.into_iter().enumerate() {
            let id = match animation_budget::GeneratedKeyframeIdSize::new(format_args!(
                "aep-time-{occurrence_id}-{index}"
            )) {
                Ok(id) => id,
                Err(error) => {
                    self.warn(
                        Limitation::Timing,
                        context.0,
                        context.1,
                        format!("source time mapping omitted before allocation: {error}"),
                    );
                    return ConvertedTiming {
                        active_range: range,
                        playback: None,
                        hide_content: false,
                    };
                }
            };
            if let Err(error) = estimate.push_key(&id, time, value, PropertyKeyframeEasing::Linear)
            {
                self.warn(
                    Limitation::Timing,
                    context.0,
                    context.1,
                    format!("source time mapping omitted before allocation: {error}"),
                );
                return ConvertedTiming {
                    active_range: range,
                    playback: None,
                    hide_content: false,
                };
            }
        }
        let reservation = match estimate.reservation_bytes(
            TimeRemapExtrapolation::Inactive,
            TimeRemapExtrapolation::Inactive,
        ) {
            Ok(reservation) => reservation,
            Err(error) => {
                self.warn(
                    Limitation::Timing,
                    context.0,
                    context.1,
                    format!("source time mapping omitted before allocation: {error}"),
                );
                return ConvertedTiming {
                    active_range: range,
                    playback: None,
                    hide_content: false,
                };
            }
        };
        let checkpoint = self.animation_budget.checkpoint();
        let denials = self.animation_budget.denials();
        if let Err(error) = self.animation_budget.reserve(reservation) {
            debug_assert!(self.animation_budget.denials() > denials);
            self.warn(
                Limitation::ExpansionLimit,
                context.0,
                context.1,
                format!(
                    "affine source-clock animation omitted at the generated-animation allowance ({error}); visible static content retained with playback unset"
                ),
            );
            return ConvertedTiming {
                active_range: range,
                playback: None,
                hide_content: false,
            };
        }
        let keys = key_data
            .into_iter()
            .enumerate()
            .map(|(index, (time, value))| TimeRemapKeyframe {
                id: KeyframeId::new(format!("aep-time-{occurrence_id}-{index}")),
                time,
                value,
                easing: PropertyKeyframeEasing::Linear,
            })
            .collect();
        match TimeRemapProperty::new(
            keys,
            TimeRemapExtrapolation::Inactive,
            TimeRemapExtrapolation::Inactive,
        ) {
            Ok(remap) => {
                self.note_committed_remap(&remap);
                ConvertedTiming {
                    active_range: range,
                    playback: Some(remap),
                    hide_content: false,
                }
            }
            Err(error) => {
                self.animation_budget.rollback(checkpoint);
                self.warn(
                    Limitation::Timing,
                    context.0,
                    context.1,
                    format!("source time mapping replaced by 1x playback: {error}"),
                );
                ConvertedTiming {
                    active_range: range,
                    playback: None,
                    hide_content: false,
                }
            }
        }
    }
}

struct ConvertedTiming {
    active_range: TimeRangeProperty,
    playback: Option<TimeRemapProperty>,
    hide_content: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LayerPurpose {
    Ordinary,
    MatteSample(set_matte::MatteSampleStage),
    Adjustment,
}

impl LayerPurpose {
    fn samples_matte(self) -> bool {
        matches!(self, Self::MatteSample(_))
    }

    fn includes_occurrence_pipeline(self) -> bool {
        match self {
            Self::MatteSample(stage) => stage.includes_occurrence_pipeline(),
            Self::Ordinary | Self::Adjustment => true,
        }
    }
}

struct LayerContext<'a> {
    expression_samples: &'a ExpressionSamples,
    comp_id: u32,
    comp: &'a Composition,
    parent: LayerId,
    depth: usize,
    solo: bool,
    layer_indices: &'a HashMap<u32, usize>,
    camera_normalization: Option<camera_normalization::CompositionNormalization>,
}

// Built from the effective occurrence after Essential Property overrides. Keeping
// positions avoids copying layers and preserves the old linear lookup's first match.
/// Reuses the source-proven native tdsn envelope decoder for runtime snapshots.
pub(super) fn native_property_name(chunks: &[crate::rifx::Chunk]) -> Option<&str> {
    control_links::display_name(chunks)
}

fn index_layers(layers: &[Layer]) -> HashMap<u32, usize> {
    let mut indices = HashMap::with_capacity(layers.len());
    for (index, layer) in layers.iter().enumerate() {
        indices.entry(layer.record.id()).or_insert(index);
    }
    indices
}

fn is_solid_source(source: &ProjectItem) -> bool {
    matches!(source.solid.as_ref(), Some(Ok(_)))
}

/// Whether AE draws an ordinary occurrence's visuals in its composition.
fn paints_visuals(flags: crate::schema::layer_records::LayerFlags, solo: bool) -> bool {
    flags.enabled && !flags.guide_layer && (!solo || flags.solo)
}

fn is_still_image(source: &ProjectItem) -> bool {
    matches!(&source.media, Some(Ok(media)) if media.kind == crate::media::MediaKind::StillImage)
}

fn has_source_relative_anchor(source: &ProjectItem) -> bool {
    is_solid_source(source)
        || matches!(source.kind, ItemKind::Composition(_))
        || source
            .footage
            .as_ref()
            .is_some_and(|footage| footage.main_source == crate::structure::FootageSourceKind::File)
}

fn solid_anchor_scale(source: Option<&ProjectItem>, size: [u16; 2]) -> [f64; 2] {
    if source.is_some_and(has_source_relative_anchor) {
        size.map(f64::from)
    } else {
        [1.0; 2]
    }
}

fn normalize_static_solid_anchor(
    layer: &Layer,
    size: [u16; 2],
    transform: &mut Transform,
    warnings: &mut Vec<String>,
) {
    match crate::properties::read_static_source_relative_anchor(&layer.content) {
        Ok(Some(anchor)) => {
            let pixel_anchor = [anchor[0] * f64::from(size[0]), anchor[1] * f64::from(size[1])];
            if pixel_anchor.iter().all(|value| value.is_finite()) {
                transform.anchor_point = pixel_anchor;
            } else {
                warnings.push(
                    "ADBE Anchor Point: source-relative value overflows source dimensions; decoded pixel/default value retained".into(),
                );
            }
        }
        Ok(None) => {}
        Err(error) => warnings.push(format!(
            "ADBE Anchor Point: source-relative storage could not be decoded ({error}); decoded pixel/default value retained"
        )),
    }
}

pub(super) fn source_anchor_dimensions(source: Option<&ProjectItem>, layer: &Layer) -> [u16; 2] {
    if let Some(Ok(solid)) = source.and_then(|source| source.solid.as_ref()) {
        return [solid.width, solid.height];
    }
    source_dimensions(source, layer)
}

fn source_dimensions(source: Option<&ProjectItem>, layer: &Layer) -> [u16; 2] {
    if layer.record.flags().null_layer || matches!(layer.record.layer_type(), 3 | 4) {
        return [0, 0];
    }
    if let Some(source) = source {
        if let Some(Ok(solid)) = &source.solid {
            return [solid.width, solid.height];
        }
        if let ItemKind::Composition(comp) = &source.kind {
            return [comp.width, comp.height];
        }
        if let Some(Ok(media)) = &source.media {
            return [media.width, media.height];
        }
    }
    [0, 0]
}

/// Audio switches and matte helpers silence whole source-precomp subtrees.
fn mute_audio(
    group: &mut GroupLayer,
    muted: &mut HashSet<fx_schema::PropertyTarget>,
) -> Result<(), serde_json::Error> {
    for layer in &mut group.layers {
        // These are freshly authored records, not imported FX records with opaque fields.
        let mut data = layer.data().clone();
        let id = match &mut data {
            FxLayer::Group(child) => {
                mute_audio(child, muted)?;
                None
            }
            FxLayer::Video(video) => {
                video.volume = None;
                Some(video.id)
            }
            FxLayer::Audio(audio) => {
                audio.is_hidden = true;
                Some(audio.id)
            }
            _ => continue,
        };
        *layer = fx_schema::Layer::from_data(&data)?;
        if let Some(id) = id {
            muted.insert(fx_schema::PropertyTarget::layer(
                id,
                fx_schema::PropType::AudioVolume,
            ));
        }
    }
    Ok(())
}

fn has_enabled_audio(group: &GroupLayer) -> bool {
    !group.is_hidden
        && group.layers.iter().any(|layer| match layer.data() {
            FxLayer::Group(child) => has_enabled_audio(child),
            FxLayer::Audio(audio) => !audio.is_hidden,
            FxLayer::Video(video) => !video.is_hidden && video.volume.is_some(),
            _ => false,
        })
}

/// AE's eye switch is not an audio mute. AV audio is a separate AudioLayer.
fn hide_visuals(group: &mut GroupLayer) -> Result<(), serde_json::Error> {
    hide_visual_layers(&mut group.layers)
}

fn hide_visual_layers(layers: &mut [fx_schema::Layer]) -> Result<(), serde_json::Error> {
    for layer in layers {
        let mut data = layer.data().clone();
        match &mut data {
            FxLayer::Group(child) => hide_visuals(child)?,
            FxLayer::Audio(_) => {}
            FxLayer::Text(layer) => layer.is_hidden = true,
            FxLayer::Video(layer) => layer.is_hidden = true,
            FxLayer::Media(layer) => layer.is_hidden = true,
            FxLayer::Image(layer) => layer.is_hidden = true,
            FxLayer::Pag(layer) => layer.is_hidden = true,
            FxLayer::Rect(layer) => layer.is_hidden = true,
            FxLayer::Shape(layer) => layer.is_hidden = true,
            FxLayer::AiEdit(layer) => hide_visual_layers(&mut layer.layers)?,
            FxLayer::BooleanOperation(layer) => layer.is_hidden = true,
            FxLayer::Adjustment(layer) => layer.is_hidden = true,
        }
        *layer = fx_schema::Layer::from_data(&data)?;
    }
    Ok(())
}

/// Whether a composition of `seconds` has an FX duration of its own.
fn representable_duration(seconds: f64) -> bool {
    seconds.is_finite() && (0.001..=MAX_TIME_SECS).contains(&seconds)
}

/// The FX duration of a composition of `seconds`, which its converted root
/// Group spans: an invalid or unrepresentable duration is one second.
pub(crate) fn composition_duration(seconds: f64) -> Duration {
    if representable_duration(seconds) {
        Duration::from_secs(seconds)
    } else {
        Duration::from_secs(1.0)
    }
}

fn fractional_millis(seconds: f64) -> bool {
    let millis = seconds * 1000.0;
    (millis - millis.round()).abs() > 0.000_001
}

fn identity_playback(range: TimeRangeProperty) -> fx_schema::LayerPlayback {
    fx_schema::LayerPlayback::linear(range, range, range, 0)
        .expect("converter-generated identity playback has a positive finite range")
}

fn remapped_playback(
    range: TimeRangeProperty,
    property: TimeRemapProperty,
) -> fx_schema::LayerPlayback {
    fx_schema::LayerPlayback::remapped(range, property, 0)
        .expect("converter-validated time remap has a positive finite window")
}

fn group(
    id: LayerId,
    name: String,
    parent: Option<LayerId>,
    active_range: TimeRangeProperty,
) -> GroupLayer {
    GroupLayer {
        id,
        name,
        description: String::new(),
        is_hidden: false,
        parent,
        blend_mode: Default::default(),
        track_matte: None,
        masks: Vec::new(),
        playback: identity_playback(active_range),
        effects: Vec::new(),
        motion_blur: false,
        padding_top: Default::default(),
        padding_right: Default::default(),
        padding_bottom: Default::default(),
        padding_left: Default::default(),
        fills: Vec::new(),
        corner_radius_top_left: Default::default(),
        corner_radius_top_right: Default::default(),
        corner_radius_bottom_right: Default::default(),
        corner_radius_bottom_left: Default::default(),
        transform: Transform {
            anchor_point: [0.0, 0.0],
            position: fx_schema::Position::TwoD([0.0, 0.0]),
            scale: [100.0, 100.0],
            rotation: 0.0,
            skew: 0.0,
            skew_axis: 0.0,
            rotation_x: 0.0,
            rotation_y: 0.0,
            orientation: [0.0, 0.0, 0.0],
            opacity: fx_schema::PercentageProperty::new(100.0)
                .expect("100 is a finite percentage in 0..=100"),
        },
        layers: Vec::new(),
    }
}

#[cfg(test)]
mod tests;
