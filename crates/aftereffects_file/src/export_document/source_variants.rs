//! Finite source-switch planning for current editable FX media leaves.
//!
//! Whole-document reference facts must be established before this module
//! allocates occurrence identities. Shared export wiring is described in
//! `/tmp/aep-source-reference-planning.md`.

#[path = "references.rs"]
mod references;

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

use fx_schema::{
    AssetId, Duration, EditableFxCompositionDocument, ImageSource, Layer, LayerData, LayerId,
    MediaSourceKind, PropType, PropertyKeyframeEasing, PropertyTarget, PropertyValue, TimeOffset,
    TimeRangeProperty,
    animator::{
        AnimationGraphEntry, AnimatorData, PropertyAnimator, PropertyKeyframe,
        PropertyKeyframeTrack,
    },
};

use super::media::{self, MediaRequest};
pub(super) use references::SourceVariantEligibility;
use references::{DocumentReferenceFacts, ReferenceAnalysisError};

impl SourceVariantEligibility {
    pub(super) const fn has_external_references(self) -> bool {
        self.nested_or_parented
            || self.referenced_as_parent
            || self.referenced_as_matte
            || self.referenced_as_mask_guide
            || self.referenced_as_text_guide
            || self.referenced_as_ai_edit_source
            || self.referenced_as_segment
            || self.referenced_by_animation_layer_ref
    }

    pub(super) const fn has_unsupported_graph_edges(self) -> bool {
        self.referenced_by_animation_dependency || self.has_unresolved_animation_dependency
    }

    fn semantic_reference_reason(self) -> Option<&'static str> {
        if self.referenced_as_ai_edit_source {
            Some(
                "a temporal source selector used as an AI Edit source cannot become a native precomposition",
            )
        } else if self.referenced_as_segment {
            Some(
                "a temporal source selector used as segment identity cannot become a native precomposition",
            )
        } else if self.referenced_by_animation_dependency {
            Some(
                "a temporal source selector used by an animation dependency needs shared graph-clock lowering",
            )
        } else if self.referenced_by_animation_layer_ref {
            Some(
                "a temporal source selector exposed as animator asset metadata cannot become a native precomposition",
            )
        } else if self.has_unresolved_animation_dependency {
            Some(
                "an animation dependency owner could not be resolved; temporal source identity is conservatively retained",
            )
        } else {
            None
        }
    }

    fn needs_visual_identity_wrapper(self) -> bool {
        self.nested_or_parented
            || self.owns_masks_or_matte
            || self.referenced_as_parent
            || self.referenced_as_matte
            || self.referenced_as_mask_guide
            || self.referenced_as_text_guide
    }
}

/// A fatal planning failure. Representable-but-unsupported authoring is returned
/// as [`SourceVariantDecision::Unsupported`] instead.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum SourceVariantPlanError {
    ReferenceAnalysis(ReferenceAnalysisError),
    DuplicateSourceSelector { layer_id: LayerId },
    UnsafeAssetIdentity { layer_id: LayerId, asset_id: String },
    LayerIdentityExhausted { layer_id: LayerId },
    VariantConstruction { layer_id: LayerId, message: String },
}

impl fmt::Display for SourceVariantPlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReferenceAnalysis(error) => {
                write!(formatter, "source-variant preflight failed: {error}")
            }
            Self::DuplicateSourceSelector { layer_id } => {
                write!(
                    formatter,
                    "FX layer {layer_id} has duplicate source selector entries"
                )
            }
            Self::UnsafeAssetIdentity { layer_id, asset_id } => write!(
                formatter,
                "FX layer {layer_id} source selector contains unsafe asset identity {asset_id:?}"
            ),
            Self::LayerIdentityExhausted { layer_id } => write!(
                formatter,
                "FX layer {layer_id} source variants exhausted the layer identity space"
            ),
            Self::VariantConstruction { layer_id, message } => write!(
                formatter,
                "FX layer {layer_id} source variant could not be constructed: {message}"
            ),
        }
    }
}

/// Result of inspecting one current source-backed leaf.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum SourceVariantDecision {
    /// No source selector targets this leaf; the ordinary typed-media route owns it.
    NotAnimated,
    /// The finite authored selector has an exact occurrence plan.
    Ready(SourceVariantPlan),
    /// Valid FX authoring that needs hierarchy/clock/native work not provided here.
    Unsupported {
        layer_id: LayerId,
        reason: &'static str,
    },
}

/// Finite bounds established by the shared all-time geometry analysis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct SourceVariantBounds {
    pub min: [f64; 2],
    pub max: [f64; 2],
}

/// Resolved source geometry needed to prove that the existing native
/// precomposition representation can preserve one referenced layer identity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct SourceVariantPrecompositionInput {
    pub source_dimensions: [u16; 2],
    pub all_time_bounds: SourceVariantBounds,
}

/// Explicit handoff to the shared hierarchy finalizer. This module does not
/// call an assumed writer helper or invent a native record shape.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct SourceVariantPrecompositionHandoff {
    pub wrapper_layer_id: LayerId,
    pub source_dimensions: [u16; 2],
    pub width: u16,
    pub height: u16,
    pub origin: [f64; 2],
}

/// How the planned occurrences must be published.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum SourceVariantPublication {
    DirectOccurrences,
    /// Every child occurrence is reminted; only the final wrapper may retain
    /// `wrapper_layer_id`, so no inbound reference is redirected to a segment.
    Precomposition(SourceVariantPrecompositionHandoff),
}

/// Ordered finite occurrences for one current FX media leaf.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct SourceVariantPlan {
    pub source_layer_id: LayerId,
    pub source_property: PropType,
    /// Exact indices in the original graph. The parent must not report these as
    /// unconsumed native property targets after materializing the plan.
    pub consumed_source_entries: Vec<usize>,
    pub publication: SourceVariantPublication,
    pub variants: Vec<SourceVariant>,
}

/// Fallible whole-document plan produced only after canonical references and
/// all original identities have been collected.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct SourceVariantDocumentPlan {
    pub decisions: BTreeMap<LayerId, SourceVariantDecision>,
    pub eligibility: BTreeMap<LayerId, SourceVariantEligibility>,
    pub media_requests: Vec<MediaRequest>,
    pub occupied_ids: BTreeSet<LayerId>,
}

/// One half-open active span with current source data installed on a clone.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct SourceVariant {
    /// Unique within the caller's complete export occurrence set.
    pub occurrence_id: LayerId,
    pub asset_id: AssetId,
    pub layer: Layer,
    /// Same-owner entries other than the consumed source selector, with layer
    /// targets, local key times, and key identities rebased to this occurrence.
    pub owner_entries: Vec<AnimationGraphEntry>,
}

/// Plans a Constant or finite incoming-Hold source selector on one typed leaf.
///
/// `occupied_ids` must initially contain every layer identity in the complete
/// document. Reserved occurrence IDs remain in it across calls, preventing
/// collisions between plans.
#[cfg(test)]
pub(super) fn plan_layer(
    layer: &Layer,
    dynamics: &[AnimationGraphEntry],
    eligibility: SourceVariantEligibility,
    occupied_ids: &mut BTreeSet<LayerId>,
) -> Result<SourceVariantDecision, SourceVariantPlanError> {
    plan_layer_with_precomposition(layer, dynamics, eligibility, occupied_ids, None)
}

/// Variant planner used by whole-document preflight once resolved source
/// dimensions and finite all-time bounds are available.
pub(super) fn plan_layer_with_precomposition(
    layer: &Layer,
    dynamics: &[AnimationGraphEntry],
    eligibility: SourceVariantEligibility,
    occupied_ids: &mut BTreeSet<LayerId>,
    precomposition: Option<SourceVariantPrecompositionInput>,
) -> Result<SourceVariantDecision, SourceVariantPlanError> {
    let Some(source_property) = source_property(layer) else {
        return Ok(SourceVariantDecision::NotAnimated);
    };
    let source_entries = dynamics
        .iter()
        .enumerate()
        .filter(|(_, entry)| {
            entry.target.as_property().is_some_and(|property| {
                property.layer_id() == layer.id() && property.property_type() == source_property
            })
        })
        .collect::<Vec<_>>();
    if source_entries.is_empty() {
        return Ok(SourceVariantDecision::NotAnimated);
    }
    if source_entries.len() != 1 {
        return Err(SourceVariantPlanError::DuplicateSourceSelector {
            layer_id: layer.id(),
        });
    }
    let (source_index, source_entry) = source_entries[0];
    if !source_entry.dependencies.is_empty()
        || source_entry.random_seed_target.is_some()
        || !source_entry.layer_refs.is_empty()
    {
        return Ok(unsupported(
            layer.id(),
            "dependent source selection requires a shared graph/clock plan; it is not sampled or baked",
        ));
    }
    let selections = match selector_segments(layer, &source_entry.animator)? {
        Ok(segments) => segments,
        Err(reason) => return Ok(unsupported(layer.id(), reason)),
    };
    if let Err(reason) = exact_clock_supported(layer) {
        return Ok(unsupported(layer.id(), reason));
    }

    let is_full_span = selections.len() == 1
        && selections[0].start_offset_ms == 0
        && selections[0].duration_ms == layer.active_range().duration.as_millis();
    let publication = if is_full_span {
        // A Constant (or coalesced one-value Hold track) changes only the
        // backing asset. Keeping the original typed layer identity preserves
        // every legitimate inbound reference without a wrapper.
        SourceVariantPublication::DirectOccurrences
    } else if let Some(reason) = eligibility.semantic_reference_reason() {
        return Ok(unsupported(layer.id(), reason));
    } else if eligibility.needs_visual_identity_wrapper() {
        let Some(input) = precomposition else {
            return Ok(unsupported(
                layer.id(),
                "referenced temporal source variants require resolved source dimensions and finite all-time bounds",
            ));
        };
        let handoff = match precomposition_handoff(layer.id(), input) {
            Ok(handoff) => handoff,
            Err(reason) => return Ok(unsupported(layer.id(), reason)),
        };
        SourceVariantPublication::Precomposition(handoff)
    } else {
        SourceVariantPublication::DirectOccurrences
    };

    occupied_ids.insert(layer.id());
    let preserve_source_id = matches!(publication, SourceVariantPublication::DirectOccurrences);
    let mut variants = Vec::with_capacity(selections.len());
    for (index, selection) in selections.into_iter().enumerate() {
        let occurrence_id = if preserve_source_id && index == 0 {
            layer.id()
        } else {
            reserve_layer_id(layer.id(), occupied_ids)?
        };
        let variant_layer = clone_variant_layer(layer, occurrence_id, &selection)?;
        let owner_entries = match clone_owner_entries(
            dynamics,
            source_index,
            layer.id(),
            occurrence_id,
            selection.start_offset_ms,
        ) {
            Ok(entries) => entries,
            Err(reason) => return Ok(unsupported(layer.id(), reason)),
        };
        variants.push(SourceVariant {
            occurrence_id,
            asset_id: selection.asset_id,
            layer: variant_layer,
            owner_entries,
        });
    }

    Ok(SourceVariantDecision::Ready(SourceVariantPlan {
        source_layer_id: layer.id(),
        source_property,
        consumed_source_entries: vec![source_index],
        publication,
        variants,
    }))
}

/// Discovers every archive asset selected by a ready plan.
///
/// The adapter owner must union these with ordinary media requests before
/// staging. This deliberately derives requests from the cloned typed leaves, so
/// a Constant override cannot accidentally stage/use the stale persisted source.
pub(super) fn media_requests(
    plan: &SourceVariantPlan,
) -> Result<Vec<MediaRequest>, SourceVariantPlanError> {
    plan.variants
        .iter()
        .map(|variant| {
            let request = media::request(&variant.layer).ok_or_else(|| {
                SourceVariantPlanError::VariantConstruction {
                    layer_id: plan.source_layer_id,
                    message: "planned occurrence is no longer typed source media".to_owned(),
                }
            })?;
            if request.asset_id != variant.asset_id {
                return Err(SourceVariantPlanError::VariantConstruction {
                    layer_id: plan.source_layer_id,
                    message: "typed request does not use the selected variant asset".to_owned(),
                });
            }
            Ok(request)
        })
        .collect()
}

/// Performs the fatal whole-document identity/reference preflight, then plans
/// every source selector using caller-supplied resolved geometry where a
/// referenced temporal selector needs a native precomposition wrapper.
#[cfg(test)]
pub(super) fn preflight_document(
    document: &EditableFxCompositionDocument,
    precompositions: &BTreeMap<LayerId, SourceVariantPrecompositionInput>,
) -> Result<SourceVariantDocumentPlan, SourceVariantPlanError> {
    preflight_layers(document, document.composition().layers(), precompositions)
}

pub(super) fn preflight_layers(
    document: &EditableFxCompositionDocument,
    roots: &[Layer],
    precompositions: &BTreeMap<LayerId, SourceVariantPrecompositionInput>,
) -> Result<SourceVariantDocumentPlan, SourceVariantPlanError> {
    fn collect(
        layers: &[Layer],
        dynamics: &[AnimationGraphEntry],
        facts: &DocumentReferenceFacts,
        precompositions: &BTreeMap<LayerId, SourceVariantPrecompositionInput>,
        occupied_ids: &mut BTreeSet<LayerId>,
        decisions: &mut BTreeMap<LayerId, SourceVariantDecision>,
        requests: &mut Vec<MediaRequest>,
    ) -> Result<(), SourceVariantPlanError> {
        for layer in layers {
            let decision = plan_layer_with_precomposition(
                layer,
                dynamics,
                facts.eligibility(layer.id()),
                occupied_ids,
                precompositions.get(&layer.id()).copied(),
            )?;
            match &decision {
                SourceVariantDecision::Ready(plan) => requests.extend(media_requests(plan)?),
                SourceVariantDecision::NotAnimated => {
                    if let Some(request) = media::request(layer) {
                        requests.push(request);
                    }
                }
                SourceVariantDecision::Unsupported { .. } => {}
            }
            decisions.insert(layer.id(), decision);
            if let Some(children) = layer.child_layers() {
                collect(
                    children,
                    dynamics,
                    facts,
                    precompositions,
                    occupied_ids,
                    decisions,
                    requests,
                )?;
            }
        }
        Ok(())
    }

    let composition = document.composition();
    let facts = DocumentReferenceFacts::analyze(
        composition.layers(),
        composition.segment_layer_ids(),
        composition.dynamics().entries(),
    )
    .map_err(SourceVariantPlanError::ReferenceAnalysis)?;
    let mut occupied_ids = facts.occupied_layer_ids();
    let mut decisions = BTreeMap::new();
    let mut requests = Vec::new();
    collect(
        roots,
        composition.dynamics().entries(),
        &facts,
        precompositions,
        &mut occupied_ids,
        &mut decisions,
        &mut requests,
    )?;
    let eligibility = decisions
        .keys()
        .copied()
        .map(|layer_id| (layer_id, facts.eligibility(layer_id)))
        .collect();
    Ok(SourceVariantDocumentPlan {
        decisions,
        eligibility,
        media_requests: requests,
        occupied_ids,
    })
}

enum MediaDiscovery {
    Ordinary,
    Selected(Vec<Selection>),
    Unsupported,
}

fn discover_layer_media(
    layer: &Layer,
    dynamics: &[AnimationGraphEntry],
    eligibility: SourceVariantEligibility,
) -> Result<MediaDiscovery, SourceVariantPlanError> {
    let Some(property) = source_property(layer) else {
        return Ok(MediaDiscovery::Ordinary);
    };
    let selectors = dynamics
        .iter()
        .enumerate()
        .filter(|(_, entry)| {
            entry.target.as_property().is_some_and(|target| {
                target.layer_id() == layer.id() && target.property_type() == property
            })
        })
        .collect::<Vec<_>>();
    if selectors.len() > 1 {
        return Err(SourceVariantPlanError::DuplicateSourceSelector {
            layer_id: layer.id(),
        });
    }
    let Some(&(source_index, entry)) = selectors.first() else {
        return Ok(MediaDiscovery::Ordinary);
    };
    if !entry.dependencies.is_empty()
        || entry.random_seed_target.is_some()
        || !entry.layer_refs.is_empty()
    {
        return Ok(MediaDiscovery::Unsupported);
    }
    let selections = match selector_segments(layer, &entry.animator)? {
        Ok(selections) => selections,
        Err(_) => return Ok(MediaDiscovery::Unsupported),
    };
    if exact_clock_supported(layer).is_err() {
        return Ok(MediaDiscovery::Unsupported);
    }
    let is_full_span = selections.len() == 1
        && selections[0].start_offset_ms == 0
        && selections[0].duration_ms == layer.active_range().duration.as_millis();
    if !is_full_span && eligibility.semantic_reference_reason().is_some() {
        return Ok(MediaDiscovery::Unsupported);
    }
    for selection in &selections {
        if clone_owner_entries(
            dynamics,
            source_index,
            layer.id(),
            layer.id(),
            selection.start_offset_ms,
        )
        .is_err()
        {
            return Ok(MediaDiscovery::Unsupported);
        }
    }
    Ok(MediaDiscovery::Selected(selections))
}

/// Discovers selected assets without allocating occurrence identities. The
/// canonical reference walk still runs first, so malformed duplicate identity
/// data reaches the adapter as a fatal error instead of being swallowed into a
/// per-layer omission. Unsupported selectors intentionally produce no request:
/// their persisted source is stale once a selector owns the property.
#[cfg(test)]
pub(super) fn discover_document_media_requests(
    document: &EditableFxCompositionDocument,
) -> Result<Vec<MediaRequest>, SourceVariantPlanError> {
    discover_layer_media_requests(document, document.composition().layers())
}

pub(super) fn discover_layer_media_requests(
    document: &EditableFxCompositionDocument,
    roots: &[Layer],
) -> Result<Vec<MediaRequest>, SourceVariantPlanError> {
    fn collect(
        layers: &[Layer],
        dynamics: &[AnimationGraphEntry],
        facts: &DocumentReferenceFacts,
        requests: &mut Vec<MediaRequest>,
    ) -> Result<(), SourceVariantPlanError> {
        for layer in layers {
            match discover_layer_media(layer, dynamics, facts.eligibility(layer.id()))? {
                MediaDiscovery::Ordinary => {
                    if let Some(request) = media::request(layer) {
                        requests.push(request);
                    }
                }
                MediaDiscovery::Selected(selections) => {
                    for selection in selections {
                        let occurrence = clone_variant_layer(layer, layer.id(), &selection)?;
                        let request = media::request(&occurrence).ok_or_else(|| {
                            SourceVariantPlanError::VariantConstruction {
                                layer_id: layer.id(),
                                message: "selected source is no longer typed media".to_owned(),
                            }
                        })?;
                        requests.push(request);
                    }
                }
                MediaDiscovery::Unsupported => {}
            }
            if let Some(children) = layer.child_layers() {
                collect(children, dynamics, facts, requests)?;
            }
        }
        Ok(())
    }

    let composition = document.composition();
    let facts = DocumentReferenceFacts::analyze(
        composition.layers(),
        composition.segment_layer_ids(),
        composition.dynamics().entries(),
    )
    .map_err(SourceVariantPlanError::ReferenceAnalysis)?;
    let mut requests = Vec::new();
    collect(
        roots,
        composition.dynamics().entries(),
        &facts,
        &mut requests,
    )?;
    Ok(requests)
}

pub(super) fn precomposition_candidates(
    document: &EditableFxCompositionDocument,
    roots: &[Layer],
) -> Result<BTreeMap<LayerId, Vec<Layer>>, SourceVariantPlanError> {
    fn collect(
        layers: &[Layer],
        dynamics: &[AnimationGraphEntry],
        candidates: &mut BTreeMap<LayerId, Vec<Layer>>,
    ) -> Result<(), SourceVariantPlanError> {
        for layer in layers {
            if let Some(property) = source_property(layer) {
                let selectors = dynamics
                    .iter()
                    .filter(|entry| {
                        entry.target.as_property().is_some_and(|target| {
                            target.layer_id() == layer.id() && target.property_type() == property
                        })
                    })
                    .collect::<Vec<_>>();
                if selectors.len() > 1 {
                    return Err(SourceVariantPlanError::DuplicateSourceSelector {
                        layer_id: layer.id(),
                    });
                }
                if let Some(entry) = selectors.first()
                    && entry.dependencies.is_empty()
                    && entry.random_seed_target.is_none()
                    && entry.layer_refs.is_empty()
                    && exact_clock_supported(layer).is_ok()
                    && let Ok(selections) = selector_segments(layer, &entry.animator)?
                {
                    let layers = selections
                        .into_iter()
                        .map(|selection| clone_variant_layer(layer, layer.id(), &selection))
                        .collect::<Result<Vec<_>, _>>()?;
                    candidates.insert(layer.id(), layers);
                }
            }
            if let Some(children) = layer.child_layers() {
                collect(children, dynamics, candidates)?;
            }
        }
        Ok(())
    }

    let composition = document.composition();
    DocumentReferenceFacts::analyze(
        composition.layers(),
        composition.segment_layer_ids(),
        composition.dynamics().entries(),
    )
    .map_err(SourceVariantPlanError::ReferenceAnalysis)?;
    let mut candidates = BTreeMap::new();
    collect(roots, composition.dynamics().entries(), &mut candidates)?;
    Ok(candidates)
}

fn precomposition_handoff(
    wrapper_layer_id: LayerId,
    input: SourceVariantPrecompositionInput,
) -> Result<SourceVariantPrecompositionHandoff, &'static str> {
    if input.source_dimensions.contains(&0) {
        return Err("referenced temporal source has zero resolved source dimensions");
    }
    let [left, top] = input.all_time_bounds.min.map(f64::floor);
    let [right, bottom] = input.all_time_bounds.max.map(f64::ceil);
    if ![left, top, right, bottom].into_iter().all(f64::is_finite)
        || right <= left
        || bottom <= top
        || right - left > f64::from(u16::MAX)
        || bottom - top > f64::from(u16::MAX)
    {
        return Err(
            "referenced temporal source all-time bounds are non-finite, empty, or exceed the native precomposition canvas",
        );
    }
    Ok(SourceVariantPrecompositionHandoff {
        wrapper_layer_id,
        source_dimensions: input.source_dimensions,
        width: (right - left) as u16,
        height: (bottom - top) as u16,
        origin: [left, top],
    })
}

fn unsupported(layer_id: LayerId, reason: &'static str) -> SourceVariantDecision {
    SourceVariantDecision::Unsupported { layer_id, reason }
}

fn source_property(layer: &Layer) -> Option<PropType> {
    match layer.data() {
        LayerData::Image(_) | LayerData::Video(_) | LayerData::Media(_) => {
            Some(PropType::MediaSourceAssetId)
        }
        LayerData::Audio(_) => Some(PropType::AudioSourceAssetId),
        _ => None,
    }
}

#[derive(Clone, Debug)]
struct Selection {
    start_offset_ms: u64,
    duration_ms: u64,
    asset_id: AssetId,
}

fn selector_segments(
    layer: &Layer,
    animator: &PropertyAnimator,
) -> Result<Result<Vec<Selection>, &'static str>, SourceVariantPlanError> {
    let duration_ms = layer.active_range().duration.as_millis();
    let raw = match animator.data() {
        AnimatorData::Constant { value }
        | AnimatorData::Keyframes {
            enabled: false,
            disabled_value: Some(value),
            ..
        } => vec![(0, string_asset(layer.id(), value)?)],
        AnimatorData::Keyframes {
            track,
            enabled: true,
            ..
        } => {
            if track.has_spatial_tangents() {
                return Ok(Err("source selector keys cannot carry spatial tangents"));
            }
            if track
                .keyframes()
                .iter()
                .skip(1)
                .any(|key| key.easing() != PropertyKeyframeEasing::Hold)
            {
                return Ok(Err("source selector changes must use incoming Hold easing"));
            }
            let keys = track.keyframes();
            let initial = keys
                .iter()
                .rfind(|key| key.layer_time().as_millis() <= 0)
                .unwrap_or(&keys[0]);
            let mut values = vec![(0, string_asset(layer.id(), initial.value())?)];
            for key in keys {
                let time = key.layer_time().as_millis();
                if time > 0 && u64::try_from(time).is_ok_and(|time| time < duration_ms) {
                    values.push((time, string_asset(layer.id(), key.value())?));
                }
            }
            values
        }
        AnimatorData::Keyframes {
            enabled: false,
            disabled_value: None,
            ..
        } => {
            return Ok(Err(
                "disabled source selector has no runtime-visible disabledValue",
            ));
        }
        AnimatorData::JsScript { .. } => {
            return Ok(Err(
                "scripted source selection is not finite authored source-switch data",
            ));
        }
    };

    let mut changes: Vec<(u64, AssetId)> = Vec::with_capacity(raw.len());
    for (time, asset_id) in raw {
        if changes
            .last()
            .is_some_and(|(_, previous)| previous == &asset_id)
        {
            continue;
        }
        changes.push((u64::try_from(time).unwrap_or(0), asset_id));
    }
    let selections = changes
        .iter()
        .enumerate()
        .map(|(index, (start, asset_id))| {
            let end = changes
                .get(index + 1)
                .map_or(duration_ms, |(next, _)| *next);
            Selection {
                start_offset_ms: *start,
                duration_ms: end - start,
                asset_id: asset_id.clone(),
            }
        })
        .collect();
    Ok(Ok(selections))
}

fn string_asset(
    layer_id: LayerId,
    value: &PropertyValue,
) -> Result<AssetId, SourceVariantPlanError> {
    let PropertyValue::String(asset_id) = value else {
        return Err(SourceVariantPlanError::UnsafeAssetIdentity {
            layer_id,
            asset_id: format!("non-string {}", value.kind().label()),
        });
    };
    AssetId::validate_namespaced(asset_id).map_err(|_| {
        SourceVariantPlanError::UnsafeAssetIdentity {
            layer_id,
            asset_id: asset_id.clone(),
        }
    })?;
    Ok(AssetId::from_trusted(asset_id.clone()))
}

fn exact_clock_supported(layer: &Layer) -> Result<(), &'static str> {
    let (active, source, playback, start_time) = match layer.data() {
        LayerData::Image(_) => return Ok(()),
        LayerData::Video(video) => (
            video.playback.input_range(),
            exact_linear_source_range(&video.playback)?,
            video.playback.time_remap(),
            video.start_time,
        ),
        LayerData::Audio(audio) => (
            audio.playback.input_range(),
            exact_linear_source_range(&audio.playback)?,
            audio.playback.time_remap(),
            audio.start_time,
        ),
        LayerData::Media(media) => {
            if media.source.kind == MediaSourceKind::Image && media.source_range.is_none() {
                return Ok(());
            }
            (
                media.active_range,
                media
                    .source_range
                    .ok_or("legacy moving media source variants require an explicit sourceRange")?,
                media.playback.as_ref(),
                media.start_time,
            )
        }
        _ => return Err("source variants require typed current source media"),
    };
    if playback.is_some() {
        return Err(
            "authored endpoint Time Remap source switches require a shared exact clock plan",
        );
    }
    // Audio's implicit audible clock is offset-only; its stored source duration
    // does not impose a stretch. Video retains its explicit 1x restriction.
    if !matches!(layer.data(), LayerData::Audio(_)) && active.duration != source.duration {
        return Err("source switches currently require an exact 1x affine source clock");
    }
    if let LayerData::Audio(audio) = layer.data() {
        // Audio startTime describes the independent authored source selection,
        // not the visible window's mapped in-point.
        return media::check_start_time(start_time, audio.source_range);
    }
    if let Some(start_secs) = start_time {
        let expected = i128::from(active.start.as_millis()) - i128::from(source.start.as_millis());
        let actual = start_secs * 1_000.0;
        if !actual.is_finite() || actual.round() != expected as f64 {
            return Err(
                "explicit media startTime does not exactly match the 1x source range clock",
            );
        }
    }
    Ok(())
}

fn exact_linear_source_range(
    playback: &fx_schema::LayerPlayback,
) -> Result<TimeRangeProperty, &'static str> {
    let fx_schema::LayerPlaybackMapping::Linear { input, output } = playback.mapping() else {
        return Err(
            "authored endpoint Time Remap source switches require a shared exact clock plan",
        );
    };
    if input.duration != output.duration {
        return Err("source switches currently require an exact 1x affine source clock");
    }
    super::media_clock::linear_output_range(playback, *input, *output)
        .map_err(|_| "source switches require exact source clock endpoints")
}

fn trimmed_playback(
    playback: &fx_schema::LayerPlayback,
    active: TimeRangeProperty,
    layer_id: LayerId,
) -> Result<fx_schema::LayerPlayback, SourceVariantPlanError> {
    let candidate = match playback.mapping() {
        fx_schema::LayerPlaybackMapping::Linear { input, output } => {
            fx_schema::LayerPlayback::linear(active, *input, *output, playback.input_offset_ms())
        }
        fx_schema::LayerPlaybackMapping::TimeRemap { property } => {
            fx_schema::LayerPlayback::remapped(active, property.clone(), playback.input_offset_ms())
        }
    };
    candidate.map_err(|message| SourceVariantPlanError::VariantConstruction {
        layer_id,
        message: message.to_owned(),
    })
}

fn reserve_layer_id(
    source_layer_id: LayerId,
    occupied_ids: &mut BTreeSet<LayerId>,
) -> Result<LayerId, SourceVariantPlanError> {
    let mut candidate = occupied_ids
        .last()
        .map_or(0, |id| id.value())
        .checked_add(1)
        .ok_or(SourceVariantPlanError::LayerIdentityExhausted {
            layer_id: source_layer_id,
        })?;
    loop {
        let id = LayerId::new(candidate);
        if occupied_ids.insert(id) {
            return Ok(id);
        }
        candidate =
            candidate
                .checked_add(1)
                .ok_or(SourceVariantPlanError::LayerIdentityExhausted {
                    layer_id: source_layer_id,
                })?;
    }
}

fn clone_variant_layer(
    source: &Layer,
    occurrence_id: LayerId,
    selection: &Selection,
) -> Result<Layer, SourceVariantPlanError> {
    let source_id = source.id();
    let mut data = source.data().clone();
    let start = source
        .active_range()
        .start
        .checked_add_duration(Duration::from_millis(selection.start_offset_ms))
        .ok_or(SourceVariantPlanError::VariantConstruction {
            layer_id: source_id,
            message: "active-range start overflow".to_owned(),
        })?;
    let active = TimeRangeProperty::new(start, Duration::from_millis(selection.duration_ms));
    match &mut data {
        LayerData::Image(image) => {
            image.id = occurrence_id;
            image.active_range = active;
            let ImageSource::Asset(asset) = &mut image.source;
            asset.asset_id = selection.asset_id.clone();
        }
        LayerData::Video(video) => {
            video.id = occurrence_id;
            video.playback = trimmed_playback(&video.playback, active, source_id)?;
            video.source.asset_id = selection.asset_id.clone();
        }
        LayerData::Audio(audio) => {
            audio.id = occurrence_id;
            audio.playback = trimmed_playback(&audio.playback, active, source_id)?;
            audio.source.asset_id = selection.asset_id.clone();
        }
        LayerData::Media(media) => {
            media.id = occurrence_id;
            media.active_range = active;
            if let Some(source_range) = media.source_range.as_mut() {
                *source_range = TimeRangeProperty::new(
                    source_range
                        .start
                        .saturating_add(Duration::from_millis(selection.start_offset_ms)),
                    Duration::from_millis(selection.duration_ms),
                );
            }
            media.source.asset_id = selection.asset_id.clone();
        }
        _ => {
            return Err(SourceVariantPlanError::VariantConstruction {
                layer_id: source_id,
                message: "source variant is not typed current media".to_owned(),
            });
        }
    }
    Layer::from_data(&data).map_err(|error| SourceVariantPlanError::VariantConstruction {
        layer_id: source_id,
        message: error.to_string(),
    })
}

fn clone_owner_entries(
    dynamics: &[AnimationGraphEntry],
    consumed_index: usize,
    source_layer_id: LayerId,
    occurrence_id: LayerId,
    start_offset_ms: u64,
) -> Result<Vec<AnimationGraphEntry>, &'static str> {
    dynamics
        .iter()
        .enumerate()
        .filter(|(index, entry)| {
            *index != consumed_index && entry.target.layer_id() == Some(source_layer_id)
        })
        .map(|(entry_index, entry)| {
            if !entry.dependencies.is_empty()
                || entry.random_seed_target.is_some()
                || !entry.layer_refs.is_empty()
            {
                return Err("dependent same-owner animation needs shared graph/clock lowering");
            }
            let Some(property) = entry.target.as_property() else {
                return Err(
                    "non-layer same-owner animation needs shared effect/fx-item ownership lowering",
                );
            };
            let animator =
                rebase_animator(&entry.animator, start_offset_ms, occurrence_id, entry_index)?;
            let mut clone = entry.clone();
            clone.target = PropertyTarget::layer(occurrence_id, property.property_type());
            clone.animator = animator;
            Ok(clone)
        })
        .collect()
}

fn rebase_animator(
    animator: &PropertyAnimator,
    start_offset_ms: u64,
    occurrence_id: LayerId,
    entry_index: usize,
) -> Result<PropertyAnimator, &'static str> {
    match animator.data() {
        AnimatorData::Constant { value }
        | AnimatorData::Keyframes {
            enabled: false,
            disabled_value: Some(value),
            ..
        } => PropertyAnimator::constant(value.clone())
            .map_err(|_| "effective constant animator clone failed"),
        AnimatorData::Keyframes {
            track,
            enabled: true,
            ..
        } => {
            let shift = i64::try_from(start_offset_ms)
                .map_err(|_| "source variant key-time shift exceeds i64")?;
            let keys = track
                .keyframes()
                .iter()
                .enumerate()
                .map(|(key_index, key)| {
                    let time = key
                        .layer_time()
                        .as_millis()
                        .checked_sub(shift)
                        .ok_or("source variant key-time rebasing overflowed")?;
                    Ok(PropertyKeyframe::new(
                        fx_schema::KeyframeId::new(format!(
                            "ae-sv-{}-{entry_index}-{key_index}",
                            occurrence_id.value()
                        )),
                        TimeOffset::from_millis(time),
                        key.value().clone(),
                        key.easing(),
                    )
                    .with_spatial_tangents(key.spatial_in_tangent(), key.spatial_out_tangent()))
                })
                .collect::<Result<Vec<_>, &'static str>>()?;
            let track = PropertyKeyframeTrack::new(keys)
                .map_err(|_| "rebased owner key track is not structurally valid")?;
            Ok(PropertyAnimator::keyframes(track))
        }
        AnimatorData::Keyframes {
            enabled: false,
            disabled_value: None,
            ..
        } => Err("disabled same-owner animation has no runtime-visible disabledValue"),
        AnimatorData::JsScript { .. } => {
            Err("same-owner scripts cannot have their layer-time clock rewritten")
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use serde_json::json;

    use super::*;

    fn image_layer() -> Layer {
        image_layer_with_asset("persisted")
    }

    fn image_layer_with_asset(asset_id: &str) -> Layer {
        serde_json::from_value(json!({
            "type": "Image",
            "id": 7,
            "name": "still",
            "parent": null,
            "activeRange": {"start": 1000, "duration": 1000},
            "transform": {
                "anchorPoint": [0, 0], "position": [0, 0], "scale": [100, 100],
                "rotation": 0, "opacity": 100
            },
            "source": {"assetId": asset_id, "fit": "contain"}
        }))
        .unwrap()
    }

    fn entry(animator: PropertyAnimator) -> AnimationGraphEntry {
        AnimationGraphEntry {
            target: PropertyTarget::layer(LayerId::new(7), PropType::MediaSourceAssetId),
            animator,
            dependencies: Vec::new(),
            random_seed_target: None,
            layer_refs: Default::default(),
        }
    }

    fn key(id: &str, time: i64, asset: &str, easing: PropertyKeyframeEasing) -> PropertyKeyframe {
        PropertyKeyframe::new(
            fx_schema::KeyframeId::new(id),
            TimeOffset::from_millis(time),
            PropertyValue::String(asset.to_owned()),
            easing,
        )
    }

    #[test]
    fn constant_override_is_the_only_requested_and_installed_asset() {
        let layer = image_layer();
        let dynamics = vec![entry(
            PropertyAnimator::constant(PropertyValue::String("override".to_owned())).unwrap(),
        )];
        let mut occupied = BTreeSet::from([layer.id()]);
        let SourceVariantDecision::Ready(plan) = plan_layer(
            &layer,
            &dynamics,
            SourceVariantEligibility::default(),
            &mut occupied,
        )
        .unwrap() else {
            panic!("expected finite plan")
        };
        assert_eq!(plan.consumed_source_entries, vec![0]);
        assert_eq!(plan.variants.len(), 1);
        assert_eq!(plan.variants[0].asset_id.as_str(), "override");
        assert_eq!(
            media_requests(&plan).unwrap()[0].asset_id.as_str(),
            "override"
        );
    }

    #[test]
    fn incoming_hold_keys_form_exact_half_open_spans_and_unique_occurrences() {
        let layer = image_layer();
        let track = PropertyKeyframeTrack::new(vec![
            key("before", -100, "a", PropertyKeyframeEasing::Linear),
            key("swap", 250, "b", PropertyKeyframeEasing::Hold),
            key("same", 500, "b", PropertyKeyframeEasing::Hold),
            key("last", 750, "c", PropertyKeyframeEasing::Hold),
        ])
        .unwrap();
        let dynamics = vec![entry(PropertyAnimator::keyframes(track))];
        let mut occupied = BTreeSet::from([layer.id(), LayerId::new(9)]);
        let SourceVariantDecision::Ready(plan) = plan_layer(
            &layer,
            &dynamics,
            SourceVariantEligibility::default(),
            &mut occupied,
        )
        .unwrap() else {
            panic!("expected finite plan")
        };
        assert_eq!(
            plan.variants
                .iter()
                .map(|variant| variant.asset_id.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "b", "c"]
        );
        assert_eq!(
            plan.variants
                .iter()
                .map(|variant| variant.layer.active_range().duration.as_millis())
                .collect::<Vec<_>>(),
            vec![250, 500, 250]
        );
        assert_eq!(
            plan.variants
                .iter()
                .map(|variant| variant.occurrence_id.value())
                .collect::<Vec<_>>(),
            vec![7, 10, 11]
        );
    }

    #[test]
    fn constant_override_preserves_original_identity_even_when_referenced() {
        let layer = image_layer();
        let dynamics = vec![entry(
            PropertyAnimator::constant(PropertyValue::String("override".to_owned())).unwrap(),
        )];
        let eligibility = SourceVariantEligibility {
            referenced_as_parent: true,
            referenced_as_matte: true,
            referenced_as_text_guide: true,
            ..Default::default()
        };
        let mut occupied = BTreeSet::from([layer.id()]);
        let SourceVariantDecision::Ready(plan) =
            plan_layer(&layer, &dynamics, eligibility, &mut occupied).unwrap()
        else {
            panic!("constant override must retain the referenced layer")
        };
        assert_eq!(plan.variants.len(), 1);
        assert_eq!(plan.variants[0].occurrence_id, layer.id());
        assert_eq!(
            plan.publication,
            SourceVariantPublication::DirectOccurrences
        );
    }

    #[test]
    fn referenced_temporal_variants_remint_every_child_and_handoff_the_original_wrapper_id() {
        let layer = image_layer();
        let track = PropertyKeyframeTrack::new(vec![
            key("a", 0, "a", PropertyKeyframeEasing::Linear),
            key("b", 500, "b", PropertyKeyframeEasing::Hold),
        ])
        .unwrap();
        let dynamics = vec![entry(PropertyAnimator::keyframes(track))];
        let mut occupied = BTreeSet::from([layer.id()]);
        let decision = plan_layer_with_precomposition(
            &layer,
            &dynamics,
            SourceVariantEligibility {
                referenced_as_matte: true,
                ..Default::default()
            },
            &mut occupied,
            Some(SourceVariantPrecompositionInput {
                source_dimensions: [1920, 1080],
                all_time_bounds: SourceVariantBounds {
                    min: [-10.25, 20.5],
                    max: [100.25, 220.5],
                },
            }),
        )
        .unwrap();
        let SourceVariantDecision::Ready(plan) = decision else {
            panic!("bounded visual reference should produce a wrapper handoff")
        };
        let SourceVariantPublication::Precomposition(handoff) = plan.publication else {
            panic!("expected precomposition handoff")
        };
        assert_eq!(handoff.wrapper_layer_id, layer.id());
        assert_eq!((handoff.width, handoff.height), (112, 201));
        assert!(
            plan.variants
                .iter()
                .all(|variant| variant.occurrence_id != layer.id())
        );
    }

    #[test]
    fn review_graph_unsupported_source_selector_does_not_discover_stale_base_asset() {
        let layer = image_layer_with_asset("stale-exr");
        let layer_id = layer.id();
        let dependency = PropertyTarget::layer(layer_id, PropType::PositionX);
        let producer = AnimationGraphEntry {
            target: dependency.clone(),
            animator: PropertyAnimator::constant(PropertyValue::Float(123.0)).unwrap(),
            dependencies: Vec::new(),
            random_seed_target: None,
            layer_refs: Default::default(),
        };
        let mut selector = entry(
            PropertyAnimator::constant(PropertyValue::String("selected-exr".to_owned())).unwrap(),
        );
        selector.dependencies.push(dependency);
        let document = EditableFxCompositionDocument::from_json_value(json!({
            "$schema": fx_schema::EDITABLE_FX_DOCUMENT_SCHEMA_URL,
            "formatVersion": 1,
            "dimensions": {"width": 1920, "height": 1080},
            "duration": 2.0,
            "backgroundColor": null,
            "composition": {
                "id": "review-source-discovery",
                "name": "review source discovery",
                "layers": [layer],
                "dynamics": {"entries": [producer, selector]}
            }
        }))
        .unwrap();

        let requests = discover_document_media_requests(&document).unwrap();
        assert!(
            requests.is_empty(),
            "an unsupported selector must not fall back to its persisted stale asset"
        );

        let plan = preflight_document(&document, &BTreeMap::new()).unwrap();
        assert!(matches!(
            plan.decisions.get(&layer_id),
            Some(SourceVariantDecision::Unsupported { .. })
        ));
        assert!(
            plan.media_requests.is_empty(),
            "integrated preflight must retain the unsupported decision without restaging stale media"
        );
    }

    #[test]
    fn discovery_rejects_selected_media_when_another_owner_animator_cannot_be_rebased() {
        let layer = image_layer_with_asset("stale-exr");
        let layer_id = layer.id();
        let selector = entry(
            PropertyAnimator::constant(PropertyValue::String("selected-exr".to_owned())).unwrap(),
        );
        let producer_target = PropertyTarget::layer(layer_id, PropType::PositionY);
        let producer = AnimationGraphEntry {
            target: producer_target.clone(),
            animator: PropertyAnimator::constant(PropertyValue::Float(10.0)).unwrap(),
            dependencies: Vec::new(),
            random_seed_target: None,
            layer_refs: Default::default(),
        };
        let dependent = AnimationGraphEntry {
            target: PropertyTarget::layer(layer_id, PropType::PositionX),
            animator: PropertyAnimator::constant(PropertyValue::Float(20.0)).unwrap(),
            dependencies: vec![producer_target],
            random_seed_target: None,
            layer_refs: Default::default(),
        };
        let document = EditableFxCompositionDocument::from_json_value(json!({
            "$schema": fx_schema::EDITABLE_FX_DOCUMENT_SCHEMA_URL,
            "formatVersion": 1,
            "dimensions": {"width": 1920, "height": 1080},
            "duration": 2.0,
            "backgroundColor": null,
            "composition": {
                "id": "review-owner-discovery",
                "name": "review owner discovery",
                "layers": [layer],
                "dynamics": {"entries": [selector, producer, dependent]}
            }
        }))
        .unwrap();

        assert!(
            discover_document_media_requests(&document)
                .unwrap()
                .is_empty()
        );
        let plan = preflight_document(&document, &BTreeMap::new()).unwrap();
        assert!(matches!(
            plan.decisions.get(&layer_id),
            Some(SourceVariantDecision::Unsupported { reason, .. })
                if *reason == "dependent same-owner animation needs shared graph/clock lowering"
        ));
    }

    #[test]
    fn discovery_accepts_disabled_owner_constant_and_rejects_script_owner() {
        let layer = image_layer_with_asset("stale-exr");
        let selector = entry(
            PropertyAnimator::constant(PropertyValue::String("selected-exr".to_owned())).unwrap(),
        );
        let keyed = PropertyKeyframeTrack::new(vec![key(
            "position",
            0,
            "unused",
            PropertyKeyframeEasing::Hold,
        )])
        .unwrap();
        let disabled = PropertyAnimator::from_data(&AnimatorData::Keyframes {
            track: keyed,
            enabled: false,
            disabled_value: Some(PropertyValue::Float(333.0)),
        })
        .unwrap();
        let owner = |animator| AnimationGraphEntry {
            target: PropertyTarget::layer(layer.id(), PropType::PositionX),
            animator,
            dependencies: Vec::new(),
            random_seed_target: None,
            layer_refs: Default::default(),
        };
        let dynamics = vec![selector.clone(), owner(disabled)];
        assert!(matches!(
            discover_layer_media(&layer, &dynamics, SourceVariantEligibility::default()).unwrap(),
            MediaDiscovery::Selected(_)
        ));
        let mut occupied = BTreeSet::from([layer.id()]);
        let SourceVariantDecision::Ready(plan) = plan_layer(
            &layer,
            &dynamics,
            SourceVariantEligibility::default(),
            &mut occupied,
        )
        .unwrap() else {
            panic!("disabled owner with a runtime-visible value is an effective constant")
        };
        assert_eq!(plan.variants.len(), 1);
        assert!(matches!(
            plan.variants[0].owner_entries[0].animator.data(),
            AnimatorData::Constant { value: PropertyValue::Float(value) } if *value == 333.0
        ));

        let script = PropertyAnimator::from_data(&AnimatorData::JsScript {
            code: None,
            layer_time_js_code: Some("0".to_owned()),
        })
        .unwrap();
        let dynamics = vec![selector, owner(script)];
        assert!(matches!(
            discover_layer_media(&layer, &dynamics, SourceVariantEligibility::default()).unwrap(),
            MediaDiscovery::Unsupported
        ));
        let mut occupied = BTreeSet::from([layer.id()]);
        assert!(matches!(
            plan_layer(
                &layer,
                &dynamics,
                SourceVariantEligibility::default(),
                &mut occupied,
            )
            .unwrap(),
            SourceVariantDecision::Unsupported { reason, .. }
                if reason == "same-owner scripts cannot have their layer-time clock rewritten"
        ));
    }

    #[test]
    fn non_hold_switches_are_not_interpolated_or_sampled() {
        let track = PropertyKeyframeTrack::new(vec![
            key("a", 0, "a", PropertyKeyframeEasing::Hold),
            key("b", 500, "b", PropertyKeyframeEasing::Linear),
        ]);
        assert!(
            track.is_err(),
            "the actual FX schema rejects interpolated strings"
        );
    }
}
