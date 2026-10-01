//! Experimental lowering from current editable FX values, never source AEP bytes.

mod audio;
mod composition_options;
mod effects;
mod hierarchy;
mod hierarchy_clock;
mod layer_styles;
mod layout;
mod masks;
pub(crate) mod media;
mod media_clock;
mod paint_color;
mod paint_controls;
mod paint_opacity;
mod path_animation;
mod radial_wipe;
mod rect_dashes;
mod rect_geometry;
mod source_variants;
mod takeover;
mod text;
mod transform3d;
mod vector_animation;

#[cfg(test)]
#[path = "export_document/constant_shape_path_tests.rs"]
mod constant_shape_path_tests;

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

use fx_conv::Progress;
use fx_schema::{
    BooleanOperationLayer, EditableFxCompositionDocument, EffectData, EffectPayload, GroupLayer,
    Layer, LayerData, LayerId, LayerPlayback, LayerPlaybackMapping, Position, PropType,
    PropertyKeyframeEasing, PropertyValue, RectLayer, ShapePath, Time, TimeRangeProperty,
    TimeRemapProperty, Transform,
    animator::{AnimationGraphEntry, AnimatorData, PropertyAnimator, PropertyKeyframeTrack},
    layer::{ShapeGradientType, ShapeLayer, ShapePaint, ShapeStrokeStyle},
};

use crate::writer::{
    AepWriteError, CompositionOptions, GeometryAnimations, KeyframeEasing, LayerReferenceFacts,
    LayerSpec, LayerTiming, NativeLayerOptions, NativeMaskMode, NativeMatteRef, NativeTransform3d,
    NumericKeyframe, NumericTrack, PrecompositionSpec, RectAnimations, SolidLayerSpec,
    SolidTransform, StrokeAnimations, StrokeDashes, Transform3dAnimations, TransformAnimations,
    VectorContent, VectorGeometry, VectorGroupSpec, VectorGroupTransform, VectorLayerSpec,
    VectorModifierSpec, VectorPaintAnimations, VectorPaintSpec, VectorRectSpec,
    emitted_media_paths, write_composition_at_rate, write_picture_only_composition_at_rate,
};
use vector_animation::{group_animations, program_transform_animations};

/// Export limitations are distinct from native-source import diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportDiagnostic {
    /// Current FX layer identity, not a historical AEP layer ID.
    pub layer_id: Option<LayerId>,
    /// Omitted semantics or an explicit normalization and its impact.
    pub message: String,
}

impl fmt::Display for ExportDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[AE-EXPORT]")?;
        if let Some(id) = self.layer_id {
            write!(f, " FX layer {id}")?;
        }
        write!(f, ": {}", self.message)
    }
}

pub(crate) struct ExportedDocument {
    pub bytes: Vec<u8>,
    pub root: crate::GeneratedRootComposition,
    pub diagnostics: Vec<ExportDiagnostic>,
    pub omitted_layer_ids: BTreeSet<LayerId>,
    pub emitted_media_paths: BTreeSet<crate::writer::footage::RelativeMediaPath>,
}

#[derive(Clone, Copy)]
pub(crate) struct ExportDocumentViews<'a> {
    original: &'a EditableFxCompositionDocument,
    prepared: &'a EditableFxCompositionDocument,
    roots: &'a [Layer],
    original_roots: &'a [Layer],
    selected: bool,
}

impl<'a> ExportDocumentViews<'a> {
    pub(crate) fn from_preparation(
        original: &'a EditableFxCompositionDocument,
        prepared: &'a EditableFxCompositionDocument,
    ) -> Self {
        Self {
            original,
            prepared,
            roots: prepared.composition().layers(),
            original_roots: original.composition().layers(),
            selected: false,
        }
    }

    #[cfg(test)]
    fn unchanged(document: &'a EditableFxCompositionDocument) -> Self {
        Self::from_preparation(document, document)
    }

    #[cfg(test)]
    pub(crate) fn original(&self) -> &'a EditableFxCompositionDocument {
        self.original
    }

    pub(crate) fn prepared(&self) -> &'a EditableFxCompositionDocument {
        self.prepared
    }

    fn original_roots(&self) -> &'a [Layer] {
        self.original_roots
    }

    pub(crate) fn select_roots(
        mut self,
        range: std::ops::Range<usize>,
    ) -> Result<Self, AepWriteError> {
        let original = self
            .original
            .composition()
            .layers()
            .get(range.clone())
            .ok_or(AepWriteError::Invalid("invalid selected root range"))?;
        let prepared = self
            .prepared
            .composition()
            .layers()
            .get(range)
            .ok_or(AepWriteError::Invalid("prepared root range changed"))?;
        if original.is_empty()
            || !original
                .iter()
                .map(Layer::id)
                .eq(prepared.iter().map(Layer::id))
        {
            return Err(AepWriteError::Invalid(
                "selected roots changed during preparation",
            ));
        }
        self.roots = prepared;
        self.original_roots = original;
        self.selected = true;
        Ok(self)
    }

    pub(crate) fn media_requests(&self) -> Result<Vec<media::MediaRequest>, AepWriteError> {
        source_variants::discover_layer_media_requests(self.prepared, self.roots)
            .map_err(|error| AepWriteError::InvalidDocument(error.to_string()))
    }
}

#[cfg(test)]
pub(crate) fn to_aep(
    document: &EditableFxCompositionDocument,
) -> Result<ExportedDocument, AepWriteError> {
    to_aep_with_media(document, &BTreeMap::new())
}

#[cfg(test)]
pub(crate) fn media_requests(
    document: &EditableFxCompositionDocument,
) -> Result<Vec<media::MediaRequest>, AepWriteError> {
    source_variants::discover_document_media_requests(document)
        .map_err(|error| AepWriteError::InvalidDocument(error.to_string()))
}

fn composition_layer_ids(layers: &[Layer]) -> BTreeSet<LayerId> {
    layers.iter().map(Layer::id).collect()
}

fn collect_source_layer_ids(layers: &[Layer], ids: &mut BTreeSet<LayerId>) {
    for layer in layers {
        ids.insert(layer.id());
        if let Some(children) = layer.child_layers() {
            collect_source_layer_ids(children, ids);
        }
    }
}

fn omit_dangling_reference_owners(
    layers: &mut Vec<LayerSpec>,
    source_layer_ids: &BTreeSet<LayerId>,
    diagnostics: &mut Vec<ExportDiagnostic>,
    omitted_layer_ids: &mut BTreeSet<LayerId>,
) -> Result<(), AepWriteError> {
    loop {
        let emitted_ids = layers
            .iter()
            .map(LayerSpec::local_reference_facts)
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter_map(|facts| facts.layer_id)
            .collect::<BTreeSet<_>>();
        let mut removals = Vec::new();
        for (index, layer) in layers.iter().enumerate() {
            let facts: LayerReferenceFacts = layer.local_reference_facts()?;
            let missing = [("parent", facts.parent), ("matte", facts.matte)]
                .into_iter()
                .find_map(|(relation, target)| {
                    target
                        .filter(|target| {
                            source_layer_ids.contains(target) && !emitted_ids.contains(target)
                        })
                        .map(|target| (relation, target))
                });
            if let Some((relation, target)) = missing {
                removals.push((index, facts.layer_id, relation, target));
            }
        }
        if removals.is_empty() {
            return Ok(());
        }
        for (index, owner, relation, target) in removals.into_iter().rev() {
            layers.remove(index);
            if let Some(owner) = owner {
                omitted_layer_ids.insert(owner);
            }
            diagnostics.push(ExportDiagnostic {
                layer_id: owner,
                message: format!(
                    "Native {relation} provider FX layer {target} was omitted; dependent owner omitted transitively, convertible unrelated siblings retained."
                ),
            });
        }
    }
}

fn source_variant_precompositions(
    document: &EditableFxCompositionDocument,
    roots: &[Layer],
    resolved_media: &BTreeMap<String, media::ResolvedMediaSource>,
) -> Result<BTreeMap<LayerId, source_variants::SourceVariantPrecompositionInput>, AepWriteError> {
    let candidates = source_variants::precomposition_candidates(document, roots)
        .map_err(|error| AepWriteError::InvalidDocument(error.to_string()))?;
    let dynamics = document.composition().dynamics().entries();
    let canvas = document.dimensions();
    let mut inputs = BTreeMap::new();
    for (owner, variants) in candidates {
        let geometry_dynamics = dynamics
            .iter()
            .filter(|entry| {
                !entry.target.as_property().is_some_and(|property| {
                    property.layer_id() == owner
                        && matches!(
                            property.property_type(),
                            PropType::MediaSourceAssetId | PropType::AudioSourceAssetId
                        )
                })
            })
            .cloned()
            .collect::<Vec<_>>();
        let mut dimensions = None;
        let mut union: Option<hierarchy::Bounds> = None;
        let mut proved = true;
        for variant in variants {
            let Some(request) = media::request(&variant) else {
                proved = false;
                break;
            };
            let Some(source) = resolved_media.get(request.asset_id.as_str()) else {
                proved = false;
                break;
            };
            if dimensions.is_some_and(|value| value != source.dimensions) {
                proved = false;
                break;
            }
            dimensions = Some(source.dimensions);
            let Ok(geometry_variant) = media_geometry_view(&variant) else {
                proved = false;
                break;
            };
            let Ok(Some(bounds)) = hierarchy::all_time_layer_bounds(
                &geometry_variant,
                &geometry_dynamics,
                resolved_media,
                canvas,
            ) else {
                proved = false;
                break;
            };
            match &mut union {
                Some(union) => union.include(bounds),
                None => union = Some(bounds),
            }
        }
        if let (true, Some(source_dimensions), Some(bounds)) = (proved, dimensions, union) {
            inputs.insert(
                owner,
                source_variants::SourceVariantPrecompositionInput {
                    source_dimensions,
                    all_time_bounds: source_variants::SourceVariantBounds {
                        min: bounds.min,
                        max: bounds.max,
                    },
                },
            );
        }
    }
    Ok(inputs)
}

/// Lowers current FX values with archive assets already interpreted and staged.
///
/// `resolved_media` is keyed by the current asset ID string. Missing entries
/// remain contextual per-layer diagnostics; no placeholder source is invented.
#[cfg(test)]
pub(crate) fn to_aep_with_media(
    document: &EditableFxCompositionDocument,
    resolved_media: &BTreeMap<String, media::ResolvedMediaSource>,
) -> Result<ExportedDocument, AepWriteError> {
    to_aep_with_media_and_fps(document, resolved_media, 24.0)
}

#[cfg(test)]
pub(crate) fn to_aep_with_fps(
    document: &EditableFxCompositionDocument,
    fps: f64,
) -> Result<ExportedDocument, AepWriteError> {
    to_aep_with_media_and_fps(document, &BTreeMap::new(), fps)
}

/// Combines staged media and native composition settings with the selected output rate.
#[cfg(test)]
pub(crate) fn to_aep_with_media_and_fps(
    document: &EditableFxCompositionDocument,
    resolved_media: &BTreeMap<String, media::ResolvedMediaSource>,
    fps: f64,
) -> Result<ExportedDocument, AepWriteError> {
    to_aep_with_document_views_and_media_and_fps(
        ExportDocumentViews::unchanged(document),
        resolved_media,
        fps,
    )
}

#[cfg(test)]
pub(crate) fn to_aep_with_document_views_and_media_and_fps(
    documents: ExportDocumentViews<'_>,
    resolved_media: &BTreeMap<String, media::ResolvedMediaSource>,
    fps: f64,
) -> Result<ExportedDocument, AepWriteError> {
    to_aep_with_document_views_and_media_and_fps_with_progress(
        documents,
        resolved_media,
        fps,
        Progress::default(),
    )
}

pub(crate) fn to_aep_with_document_views_and_media_and_fps_with_progress(
    documents: ExportDocumentViews<'_>,
    resolved_media: &BTreeMap<String, media::ResolvedMediaSource>,
    fps: f64,
    progress: Progress<'_>,
) -> Result<ExportedDocument, AepWriteError> {
    to_aep_with_media_fps_and_audio_mode(
        documents,
        resolved_media,
        fps,
        OutputAudioMode::Preserve,
        progress,
    )
}

pub(crate) fn to_picture_only_aep_with_document_views_and_media_and_fps_with_progress(
    documents: ExportDocumentViews<'_>,
    resolved_media: &BTreeMap<String, media::ResolvedMediaSource>,
    fps: f64,
    progress: Progress<'_>,
) -> Result<ExportedDocument, AepWriteError> {
    to_aep_with_media_fps_and_audio_mode(
        documents,
        resolved_media,
        fps,
        OutputAudioMode::DisableAllNativeSwitches,
        progress,
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OutputAudioMode {
    Preserve,
    DisableAllNativeSwitches,
}

fn to_aep_with_media_fps_and_audio_mode(
    documents: ExportDocumentViews<'_>,
    resolved_media: &BTreeMap<String, media::ResolvedMediaSource>,
    fps: f64,
    audio_mode: OutputAudioMode,
    progress: Progress<'_>,
) -> Result<ExportedDocument, AepWriteError> {
    progress.stage("prepare AE layers");
    let document = documents.prepared();
    let rate = crate::timing::FrameRate::new(fps)?;
    let dimensions = document.dimensions();
    let root_layers = documents.roots;
    let source_layer_ids = composition_layer_ids(root_layers);
    let mut selected_source_ids = BTreeSet::new();
    collect_source_layer_ids(documents.original_roots(), &mut selected_source_ids);
    let millis = document.duration().as_millis();
    let (frames, duration) = rate.duration(millis)?;
    let width = u16::try_from(dimensions.width)
        .map_err(|_| AepWriteError::Invalid("canvas width exceeds u16"))?;
    let height = u16::try_from(dimensions.height)
        .map_err(|_| AepWriteError::Invalid("canvas height exceeds u16"))?;
    let composition_options =
        composition_options::from_motion_blur(document.composition().motion_blur())?;
    let precompositions = source_variant_precompositions(document, root_layers, resolved_media)?;
    let source_variants =
        source_variants::preflight_layers(document, root_layers, &precompositions)
            .map_err(|error| AepWriteError::InvalidDocument(error.to_string()))?;
    let mut lowerer = Lowerer {
        duration,
        end: Time::ZERO.saturating_add(document.duration()),
        dynamics: document.composition().dynamics().entries(),
        dimensions,
        resolved_media,
        composition_options,
        occupied_ids: source_variants.occupied_ids,
        source_variant_eligibility: source_variants.eligibility,
        source_variants: source_variants.decisions,
        consumed_guides: BTreeSet::new(),
        inside_precomposition: false,
        layers: Vec::new(),
        diagnostics: Vec::new(),
        omitted_layer_ids: BTreeSet::new(),
    };
    lowerer.warn(None, format!("Experimental fresh native Solid/vector/Text/footage export: Adobe open/render acceptance is unverified. Root output uses {}fps and square pixels; generated precompositions inherit the selected root frame rate. Current integral motion-blur/shutter settings are authored on root and generated precompositions. Other FX document metadata is not restored. Solid RGB narrows to native f32 precision.", rate.fps()));
    if (rate.fps() - fps).abs() > f64::EPSILON {
        lowerer.warn(
            None,
            format!(
                "Requested {fps}fps approximated as {}fps by native 16.16 FPS encoding.",
                rate.fps()
            ),
        );
    }
    lowerer.warn(None, "Representable vector-only Groups are authored as nested native vector groups; identity mixed-content wrappers may be normalized away. Exact precomposition clocks/collapse/clipping and unsupported native records remain diagnosed rather than guessed. Native layer/source IDs are newly allocated; no original AEP is replayed.");
    if (f64::from(duration.ticks()) / 24_576.0 - millis as f64 / 1000.0).abs() > 1.0 / 24_576.0 {
        lowerer.warn(None, format!("Duration {millis}ms rounded up to {frames} frames at {}fps; full-span layers extend to that boundary.", rate.fps()));
    }
    if document
        .background_color()
        .is_some_and(|color| color[3] != 0.0)
    {
        lowerer.warn(
            None,
            "Document background omitted; output has no background solid.",
        );
    }
    if document.unknown_field_names().next().is_some()
        || document.composition().unknown_fields().next().is_some()
    {
        lowerer.warn(None, "Unknown retained FX fields are not interpreted or exported; their semantics are unverified.");
    }
    for entry in document.composition().dynamics().entries() {
        if let AnimatorData::Keyframes {
            enabled: false,
            disabled_value,
            ..
        } = entry.animator.data()
        {
            lowerer.warn(
                entry.target.layer_id(),
                if disabled_value.is_some() {
                    "Disabled animator key authoring is not retained; its runtime-visible disabledValue is normalized to a native constant."
                } else {
                    "Disabled animator has no runtime-visible disabledValue; mapped native export fails closed instead of using a stale typed base."
                },
            );
        }
    }
    lowerer.consumed_guides = lowerer.collect_consumed_guides(root_layers);
    let root_demand = hierarchy::root_demand(dimensions, millis);
    let root_progress = progress.phase("lower AE root layers", "root layers", root_layers.len());
    for (index, layer) in root_layers.iter().enumerate() {
        lowerer.layer(layer, None, None, root_layers, 0, true, &root_demand);
        root_progress.update(index + 1);
    }
    progress.stage("validate and serialize AEP");
    omit_dangling_reference_owners(
        &mut lowerer.layers,
        &source_layer_ids,
        &mut lowerer.diagnostics,
        &mut lowerer.omitted_layer_ids,
    )?;
    if lowerer.layers.is_empty() {
        if documents.selected {
            return Err(AepWriteError::NoConvertiblePicture(lowerer.diagnostics));
        }
        lowerer.warn(
            None,
            "No supported visible layers remain; output composition is empty.",
        );
    }
    crate::writer::append_root_camera(
        &mut lowerer.layers,
        crate::writer::NativeCameraSpec::root(u32::from(width), u32::from(height)),
    )?;
    crate::writer::validate_layers(&lowerer.layers, duration)?;
    let retained_media_paths = emitted_media_paths(&lowerer.layers);
    let write = match audio_mode {
        OutputAudioMode::Preserve => write_composition_at_rate,
        OutputAudioMode::DisableAllNativeSwitches => write_picture_only_composition_at_rate,
    };
    let written = write(
        document.composition().name(),
        width,
        height,
        &lowerer.layers,
        rate,
        duration,
        Some(lowerer.composition_options),
    )?;
    lowerer
        .omitted_layer_ids
        .retain(|id| selected_source_ids.contains(id));
    Ok(ExportedDocument {
        bytes: written,
        root: crate::GeneratedRootComposition {
            name: document.composition().name().to_owned(),
            width,
            height,
            frame_rate: rate.fps(),
            duration_secs: f64::from(duration.ticks()) / 24_576.0,
        },
        diagnostics: lowerer.diagnostics,
        omitted_layer_ids: lowerer.omitted_layer_ids,
        emitted_media_paths: retained_media_paths,
    })
}

struct Lowerer<'a> {
    duration: crate::timing::Duration24,
    end: Time,
    dynamics: &'a [AnimationGraphEntry],
    dimensions: fx_schema::Dimensions,
    resolved_media: &'a BTreeMap<String, media::ResolvedMediaSource>,
    composition_options: CompositionOptions,
    occupied_ids: BTreeSet<LayerId>,
    source_variant_eligibility: BTreeMap<LayerId, source_variants::SourceVariantEligibility>,
    source_variants: BTreeMap<LayerId, source_variants::SourceVariantDecision>,
    consumed_guides: BTreeSet<LayerId>,
    inside_precomposition: bool,
    layers: Vec<LayerSpec>,
    diagnostics: Vec<ExportDiagnostic>,
    omitted_layer_ids: BTreeSet<LayerId>,
}

struct LowerLayerContext<'a> {
    parent: Option<LayerId>,
    inherited: Option<(&'a Transform, LayerId)>,
    siblings: &'a [Layer],
    depth: usize,
    inline_group: bool,
    effectful_vector_group: bool,
    output_visible: bool,
    demand: &'a hierarchy::Demand,
}

impl Lowerer<'_> {
    fn warn(&mut self, layer_id: Option<LayerId>, message: impl Into<String>) {
        self.diagnostics.push(ExportDiagnostic {
            layer_id,
            message: message.into(),
        });
    }

    fn collect_consumed_guides(&self, root_layers: &[Layer]) -> BTreeSet<LayerId> {
        let mut probe = Self {
            duration: self.duration,
            end: self.end,
            dynamics: self.dynamics,
            dimensions: self.dimensions,
            resolved_media: self.resolved_media,
            composition_options: self.composition_options,
            occupied_ids: self.occupied_ids.clone(),
            source_variant_eligibility: self.source_variant_eligibility.clone(),
            source_variants: self.source_variants.clone(),
            consumed_guides: BTreeSet::new(),
            inside_precomposition: false,
            layers: Vec::new(),
            diagnostics: Vec::new(),
            omitted_layer_ids: BTreeSet::new(),
        };
        let root_demand = hierarchy::root_demand(self.dimensions, self.end.as_millis());
        for layer in root_layers {
            probe.layer(layer, None, None, root_layers, 0, true, &root_demand);
        }
        probe.consumed_guides
    }

    fn inline_vector_group_eligible(&self, group: &GroupLayer, depth: usize) -> bool {
        let mut ignored_approximations = Vec::new();
        inline_vector_group(group)
            && !group_transform_is_animated(group, self.dynamics)
            && group.effects.is_empty()
            && self.check_group(group).is_ok()
            && vector_group_program(
                group,
                &group.transform,
                self.dynamics,
                depth,
                &mut ignored_approximations,
            )
            .is_ok()
    }

    fn effectful_vector_group_eligible(&self, group: &GroupLayer, depth: usize) -> bool {
        let children_keep_identity = group.layers.iter().all(|child| {
            let facts = self
                .source_variant_eligibility
                .get(&child.id())
                .copied()
                .unwrap_or_default();
            !facts.referenced_as_parent
                && !facts.referenced_as_matte
                && !facts.referenced_as_mask_guide
                && !facts.referenced_as_text_guide
                && !facts.referenced_as_ai_edit_source
                && !facts.referenced_as_segment
                && !facts.referenced_by_animation_dependency
                && !facts.referenced_by_animation_layer_ref
        });
        let mut ignored_approximations = Vec::new();
        effectful_inline_vector_group(group)
            && !group_transform_is_animated(group, self.dynamics)
            && self.check_group(group).is_ok()
            && children_keep_identity
            && !group.effects.iter().any(effect_is_layer_style)
            && effectful_vector_group_program(
                group,
                self.dynamics,
                depth,
                &mut ignored_approximations,
            )
            .is_ok()
    }

    fn prepare_layer_options(
        &mut self,
        layer: &Layer,
        parent: Option<LayerId>,
        inherited: Option<(&Transform, LayerId)>,
        siblings: &[Layer],
        dynamics: &[AnimationGraphEntry],
        output_visible: bool,
    ) -> Result<(NativeLayerOptions, Option<u16>), &'static str> {
        let mut options = native_layer_options(layer)?;
        // FX consumes a matte dependency instead of painting it independently.
        // AE still samples a disabled provider through the owner's matte link.
        if self
            .source_variant_eligibility
            .get(&layer.id())
            .is_some_and(|facts| facts.referenced_as_matte)
        {
            options.enabled = false;
        }
        let source_size = self.layer_source_size(layer)?;
        let effect_size = match layer.data() {
            LayerData::Rect(rect) => rect.rect.size,
            _ => source_size.map(f64::from),
        };
        // Group effect coordinates belong to the generated precomposition,
        // whose finite dimensions are not available until hierarchy planning.
        if !matches!(layer.data(), LayerData::Group(_)) {
            let (effects, styles) =
                self.lower_effect_stack(layer.id(), layer.data().effects(), dynamics, effect_size);
            options.effects = effects;
            options.styles = styles;
        }
        let Some((own_transform, path_masks, text_path)) = mask_and_transform(layer) else {
            return Ok((options, None));
        };
        // Group masks belong to the bounded precomposition source, but their
        // presence must not bypass the occurrence's native 3D Transform.
        let path_masks = if matches!(layer.data(), LayerData::Group(_)) {
            &[][..]
        } else {
            path_masks
        };
        // Adjustment geometry is a gate in stack coordinates; its own spatial
        // Transform is deliberately inert in FX. Guides carry editable geometry.
        let gate_transform = identity_fx_transform();
        let own_transform = if matches!(layer.data(), LayerData::Adjustment(_)) {
            &gate_transform
        } else {
            own_transform
        };
        let selected = choose_transform(
            inherited,
            own_transform,
            layer.id(),
            has_transform_entries(dynamics, layer.id()),
        )?;
        let (selected_transform, transform_owner_id) =
            selected.unwrap_or((own_transform, layer.id()));
        let lowered_masks = masks::lower(
            path_masks,
            text_path,
            masks::MaskOwner {
                id: layer.id(),
                parent,
                transform: selected_transform,
                source_size,
                clock: match layer.data() {
                    LayerData::Image(_) | LayerData::Video(_) | LayerData::Media(_) => None,
                    LayerData::Group(group) if playback_time_remap(&group.playback).is_some() => {
                        None
                    }
                    _ => Some(layer.active_range()),
                },
            },
            siblings,
            dynamics,
        );
        for diagnostic in lowered_masks.diagnostics {
            self.warn(Some(layer.id()), diagnostic);
        }
        if output_visible {
            self.consumed_guides.extend(lowered_masks.consumed_guides);
        }
        options.masks = lowered_masks.masks;
        if let Some(request) = media::request(layer) {
            let source = self
                .resolved_media
                .get(request.asset_id.as_str())
                .ok_or("Media archive source was not resolved/staged")?;
            if let Some(mut crop) = media::crop_mask(layer, source)? {
                if !options.masks.is_empty() {
                    crop.mode = NativeMaskMode::Intersect;
                }
                options.masks.push(crop);
            }
        }

        if matches!(layer.data(), LayerData::Adjustment(_)) {
            return Ok((options, lowered_masks.text_path_index));
        }
        let provisional = transform3d::lower(
            dynamics,
            selected_transform,
            transform_owner_id,
            transform3d::Native2dGeometry::IDENTITY,
        )?;
        let lowered_3d = if provisional.is_some()
            && matches!(
                layer.data(),
                LayerData::Image(_) | LayerData::Video(_) | LayerData::Media(_)
            ) {
            let request = media::request(layer).ok_or("Media layer has no archive request")?;
            let source = self
                .resolved_media
                .get(request.asset_id.as_str())
                .ok_or("Media archive source was not resolved/staged")?;
            let footage =
                media::lower_with_transform(layer, source, self.dimensions, selected_transform)?;
            transform3d::lower(
                dynamics,
                selected_transform,
                transform_owner_id,
                transform3d::Native2dGeometry::source(
                    footage.source_geometry.origin,
                    footage.source_geometry.scale,
                ),
            )?
        } else if provisional.is_some() && matches!(layer.data(), LayerData::Rect(_)) {
            let LayerData::Rect(rect) = layer.data() else {
                return Err("Rectangle layer kind changed during 3D lowering");
            };
            transform3d::lower(
                dynamics,
                selected_transform,
                transform_owner_id,
                transform3d::Native2dGeometry::centered(rect.rect.position),
            )?
        } else {
            provisional
        };
        if let Some(lowered) = lowered_3d {
            if lowered.transform.is_three_d {
                self.warn(Some(layer.id()), lowered.projection_diagnostic);
            }
            options.transform_3d = Some((lowered.transform, lowered.animations));
        }
        Ok((options, lowered_masks.text_path_index))
    }

    fn lower_effect_stack(
        &mut self,
        owner: LayerId,
        records: &[fx_schema::EffectRecord],
        dynamics: &[AnimationGraphEntry],
        size: [f64; 2],
    ) -> (
        Vec<crate::writer::effects::NativeEffect>,
        Vec<crate::layer_styles::NativeLayerStyle>,
    ) {
        let lowered = effects::lower(records, dynamics, size);
        if self.inside_precomposition && (!lowered.effects.is_empty() || !lowered.styles.is_empty())
        {
            // Bounds uncertainty is a fidelity limitation, not a reason to
            // discard otherwise representable editable controls and keys.
            self.warn(Some(owner), "Effect stack retained on a nested precomposition source layer: finite content-only bounds may clip effect/style expansion; native controls and compatible keys remain editable, but render fidelity is unverified.");
        }
        for warning in lowered.warnings {
            self.warn(Some(owner), warning);
        }
        (lowered.effects, lowered.styles)
    }

    fn layer_source_size(&self, layer: &Layer) -> Result<[u32; 2], &'static str> {
        if let Some(request) = media::request(layer) {
            let source = self
                .resolved_media
                .get(request.asset_id.as_str())
                .ok_or("Media archive source was not resolved/staged")?;
            return Ok(source.dimensions.map(u32::from));
        }
        Ok([self.dimensions.width, self.dimensions.height])
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "the recursive walk passes parent, inherited transform, siblings, depth, visibility and demand separately"
    )]
    fn layer(
        &mut self,
        layer: &Layer,
        parent: Option<LayerId>,
        inherited: Option<(&Transform, LayerId)>,
        siblings: &[Layer],
        depth: usize,
        ancestor_visible: bool,
        demand: &hierarchy::Demand,
    ) {
        if self.consumed_guides.contains(&layer.id()) {
            return;
        }
        let start = self.layers.len();
        let diagnostics_before = self.diagnostics.len();
        let consumed_guides_before = self.consumed_guides.clone();
        let occupied_ids_before = self.occupied_ids.clone();
        let output_visible = ancestor_visible && !layer_is_hidden(layer);
        let inline_group = match layer.data() {
            LayerData::Group(group) => self.inline_vector_group_eligible(group, depth),
            _ => false,
        };
        let effectful_vector_group = match layer.data() {
            LayerData::Group(group) => self.effectful_vector_group_eligible(group, depth),
            _ => false,
        };
        let mut reflected_owners = Vec::new();
        if !matches!(layer.data(), LayerData::Group(_)) && has_reflected_gradient(layer) {
            reflected_owners.push(layer.id());
        }
        if matches!(
            layer.data(),
            LayerData::Image(_) | LayerData::Video(_) | LayerData::Audio(_) | LayerData::Media(_)
        ) {
            let result = self
                .lower_media_layer(layer, parent, inherited, siblings, output_visible)
                .map_err(str::to_owned)
                .and_then(|()| {
                    paint_color::retain_supported_tracks(
                        &mut self.layers[start..],
                        &mut self.diagnostics,
                    );
                    for output in &self.layers[start..] {
                        crate::writer::validate_layer_payload(output, self.duration)
                            .map_err(|error| error.to_string())?;
                    }
                    Ok(())
                });
            if let Err(reason) = result {
                self.layers.truncate(start);
                self.diagnostics.truncate(diagnostics_before);
                self.omitted_layer_ids.insert(layer.id());
                self.consumed_guides = consumed_guides_before;
                self.occupied_ids = occupied_ids_before;
                self.warn(
                    Some(layer.id()),
                    format!(
                        "{reason}; layer and its subtree omitted, convertible siblings retained."
                    ),
                );
            }
            return;
        }
        let result = self
            .prepare_layer_options(
                layer,
                parent,
                inherited,
                siblings,
                self.dynamics,
                output_visible,
            )
            .and_then(|(mut options, text_path_index)| {
                self.lower_layer(
                    layer,
                    LowerLayerContext {
                        parent,
                        inherited,
                        siblings,
                        depth,
                        inline_group,
                        effectful_vector_group,
                        output_visible,
                        demand,
                    },
                    (&mut options, text_path_index),
                )?;
                let wraps_output = !matches!(
                    layer.data(),
                    LayerData::Group(_)
                        | LayerData::Image(_)
                        | LayerData::Video(_)
                        | LayerData::Audio(_)
                        | LayerData::Media(_)
                );
                if wraps_output {
                    let emitted: Vec<_> = self
                        .layers
                        .drain(start..)
                        .map(|output| LayerSpec::Options(Box::new(output), options.clone()))
                        .collect();
                    self.layers.extend(emitted);
                }
                Ok(())
            })
            .map_err(str::to_owned)
            .and_then(|()| {
                paint_color::retain_supported_tracks(
                    &mut self.layers[start..],
                    &mut self.diagnostics,
                );
                for output in &self.layers[start..] {
                    crate::writer::validate_layer_payload(output, self.duration)
                        .map_err(|error| error.to_string())?;
                }
                Ok(())
            });
        match result {
            Ok(()) => {
                for owner in reflected_owners {
                    self.warn(Some(owner), "Reflected gradient editable controls were normalized to mirrored Linear native controls; the authored symmetric interpolation is retained, but AE exposes the normalized Linear representation.");
                }
            }
            Err(reason) => {
                self.layers.truncate(start);
                self.diagnostics.truncate(diagnostics_before);
                self.omitted_layer_ids.insert(layer.id());
                self.consumed_guides = consumed_guides_before;
                self.occupied_ids = occupied_ids_before;
                self.warn(
                    Some(layer.id()),
                    format!(
                        "{reason}; layer and its subtree omitted, convertible siblings retained."
                    ),
                );
            }
        }
    }

    fn lower_layer(
        &mut self,
        layer: &Layer,
        context: LowerLayerContext<'_>,
        prepared: (&mut NativeLayerOptions, Option<u16>),
    ) -> Result<(), &'static str> {
        let LowerLayerContext {
            parent,
            inherited,
            siblings,
            depth,
            inline_group,
            effectful_vector_group,
            output_visible,
            demand,
        } = context;
        let (options, text_path_index) = prepared;
        if depth >= 48 {
            return Err("Static export depth (48) limit reached");
        }
        if effective_parent(layer, parent) != parent {
            return Err("Non-containment transform parent is not yet exportable");
        }
        let range = layer.active_range();
        let partial = range.start != Time::ZERO || range.end() < self.end;
        let timing = if partial {
            let start_millis = i64::try_from(range.start.as_millis())
                .map_err(|_| "Layer start exceeds native clock range")?;
            let end_millis = i64::try_from(range.end().min(self.end).as_millis())
                .map_err(|_| "Layer end exceeds native clock range")?;
            if start_millis >= end_millis {
                return Err("Layer has no positive active span inside the composition");
            }
            Some(LayerTiming {
                start_millis,
                end_millis,
            })
        } else {
            None
        };
        match layer.data() {
            LayerData::Group(group) => {
                if effectful_vector_group {
                    let mut paint_approximations = Vec::new();
                    if let Ok(program) = effectful_vector_group_program(
                        group,
                        self.dynamics,
                        depth,
                        &mut paint_approximations,
                    ) {
                        let lowered = effects::lower(
                            &group.effects,
                            self.dynamics,
                            [
                                f64::from(self.dimensions.width),
                                f64::from(self.dimensions.height),
                            ],
                        );
                        let mut candidate_options = options.clone();
                        candidate_options.effects = lowered.effects;
                        candidate_options.styles = lowered.styles;
                        let output = LayerSpec::VectorProgram(program);
                        let output = match timing {
                            Some(timing) => LayerSpec::Timed(Box::new(output), timing),
                            None => output,
                        };
                        let output = LayerSpec::Options(Box::new(output), candidate_options);
                        if crate::writer::validate_layer_payload(&output, self.duration).is_ok() {
                            self.layers.push(output);
                            for warning in lowered.warnings {
                                self.warn(Some(group.id), warning);
                            }
                            for (owner, message) in paint_approximations {
                                self.warn(Some(owner), message);
                            }
                            let mut reflected_owners = Vec::new();
                            collect_reflected_gradient_owners(layer, &mut reflected_owners);
                            for owner in reflected_owners {
                                self.warn(Some(owner), "Reflected gradient editable controls were normalized to mirrored Linear native controls; the authored symmetric interpolation is retained, but AE exposes the normalized Linear representation.");
                            }
                            return Ok(());
                        }
                    }
                }
                if inline_group {
                    let base_transform = identity_fx_transform();
                    let transform = if options.transform_3d.is_some() {
                        &base_transform
                    } else {
                        &group.transform
                    };
                    let mut paint_approximations = Vec::new();
                    if let Ok(program) = vector_group_program(
                        group,
                        transform,
                        self.dynamics,
                        depth,
                        &mut paint_approximations,
                    ) {
                        let output = LayerSpec::VectorProgram(program);
                        let output = match timing {
                            Some(timing) => LayerSpec::Timed(Box::new(output), timing),
                            None => output,
                        };
                        let output = LayerSpec::Options(Box::new(output), options.clone());
                        // Some layer blends have no native vector-paint ordinal.
                        // Test the complete candidate before committing to this
                        // representation, not only when writing the whole file.
                        if crate::writer::validate_layer_payload(&output, self.duration).is_ok() {
                            self.layers.push(output);
                            for (owner, message) in paint_approximations {
                                self.warn(Some(owner), message);
                            }
                            let mut reflected_owners = Vec::new();
                            collect_reflected_gradient_owners(layer, &mut reflected_owners);
                            for owner in reflected_owners {
                                self.warn(Some(owner), "Reflected gradient editable controls were normalized to mirrored Linear native controls; the authored symmetric interpolation is retained, but AE exposes the normalized Linear representation.");
                            }
                            return Ok(());
                        }
                    }
                }
                // Vector-only content is not necessarily a single native Shape:
                // child keys, clocks or compositing may require real layers. Let
                // the existing hierarchy path preserve eligible children and
                // diagnose unsupported leaves instead of omitting the whole tree.
                self.lower_group(
                    group,
                    options.clone(),
                    siblings,
                    depth,
                    output_visible,
                    demand,
                )?;
                return Ok(());
            }
            LayerData::Adjustment(adjustment) => {
                if inherited.is_some() {
                    return Err(
                        "Adjustment cannot inherit a flattened Group Transform; retain its containing composition",
                    );
                }
                let width = dimension(f64::from(self.dimensions.width))?;
                let height = dimension(f64::from(self.dimensions.height))?;
                let center = [f64::from(width) / 2.0, f64::from(height) / 2.0];
                let source = SolidLayerSpec {
                    name: adjustment.name.clone(),
                    width,
                    height,
                    color: [1.0; 3],
                    transform: SolidTransform {
                        anchor: center,
                        position: center,
                        scale: [100.0; 2],
                        rotation: 0.0,
                        opacity: adjustment.transform.opacity.value(),
                    },
                };
                let animations = TransformAnimations {
                    opacity: scalar_track(
                        track(self.dynamics, adjustment.id, PropType::Opacity)?,
                        100.0,
                    )?,
                    ..TransformAnimations::default()
                };
                if !adjustment.masks.is_empty()
                    || adjustment.track_matte.is_some()
                    || adjustment.transform.opacity.value() < 100.0
                    || animations.opacity.is_some()
                {
                    self.warn(Some(adjustment.id), "Gated Adjustment exports native AE mask/matte/opacity controls; existing FX dry-plus-wet alpha behavior can differ from AE interpolation, so FX-to-Adobe render equivalence is unverified.");
                }
                let output = LayerSpec::AnimatedSolid(source, animations);
                self.layers.push(match timing {
                    Some(timing) => LayerSpec::Timed(Box::new(output), timing),
                    None => output,
                });
                return Ok(());
            }
            LayerData::AiEdit(ai_edit) => {
                let normalized = layout::normalize_ai_edit(ai_edit)
                    .map_err(|_| "AiEdit descendant normalization failed")?;
                for diagnostic in normalized.diagnostics {
                    self.diagnostics.push(diagnostic);
                }
                self.lower_group(
                    &normalized.group,
                    options.clone(),
                    siblings,
                    depth,
                    output_visible,
                    demand,
                )?;
                return Ok(());
            }
            LayerData::Rect(rect) => {
                let selected = choose_transform(
                    inherited,
                    &rect.transform,
                    rect.id,
                    has_transform_entries(self.dynamics, rect.id),
                )?;
                let (selected_transform, transform_id) =
                    selected.unwrap_or((&rect.transform, rect.id));
                let base_transform = identity_fx_transform();
                let transform = if options.transform_3d.is_some() {
                    &base_transform
                } else {
                    selected_transform
                };
                if timing.is_some_and(|range| range.start_millis != 0)
                    && transform_id != rect.id
                    && has_entries_for(self.dynamics, transform_id)
                {
                    return Err(
                        "Animated Group Transform cannot be flattened onto a later-starting child without rebasing its source clock",
                    );
                }
                let vector_skew = options.transform_3d.is_none()
                    && (transform.skew != 0.0
                        || transform.skew_axis != 0.0
                        || vector_animation::has_skew_tracks(self.dynamics, transform_id));
                let mut transform_animations = transform_animations_partitioned(
                    self.dynamics,
                    transform_id,
                    transform,
                    rect.id,
                    options.transform_3d.is_some(),
                    vector_skew,
                )?;
                let has_content_keys = self.dynamics.iter().any(|entry| {
                    entry.target.as_property().is_some_and(|property| {
                        property.layer_id() == rect.id
                            && matches!(
                                property.property_type(),
                                PropType::RectSize
                                    | PropType::RectRoundness
                                    | PropType::FillColor
                                    | PropType::StrokeColor
                                    | PropType::StrokeWidth
                                    | PropType::StrokeDashOffset
                                    | PropType::StrokeMiterLimit
                                    | PropType::StrokeJoin
                                    | PropType::FillEnabled
                                    | PropType::StrokeEnabled
                            )
                    })
                });
                if rect.description.starts_with("Editable AE solid;")
                    && !vector_skew
                    && !has_content_keys
                    && let Ok(source) = solid(rect, transform)
                {
                    if has_entries_for(self.dynamics, rect.id) && rect.id != transform_id {
                        return Err(
                            "Solid content has animator targets that cannot be represented on the native source",
                        );
                    }
                    // Solid sources start at (0, 0), whereas the edited Rect
                    // can have a local origin. Rebase authored anchors just as
                    // solid() rebases the static anchor; tangents are offsets.
                    if let Some(anchor) = &mut transform_animations.anchor {
                        for key in &mut anchor.keys {
                            key.values[0] -= rect.rect.position[0];
                            key.values[1] -= rect.rect.position[1];
                        }
                    }
                    let output = if transform_animations == TransformAnimations::default() {
                        LayerSpec::Solid(source)
                    } else {
                        LayerSpec::AnimatedSolid(source, transform_animations)
                    };
                    self.layers.push(match timing {
                        Some(timing) => LayerSpec::Timed(Box::new(output), timing),
                        None => output,
                    });
                    return Ok(());
                }
                let needs_paint_program = vector_skew
                    || rect.rect.fill_paint.is_some()
                    || (rect.rect.fill_enabled && rect.rect.stroke_enabled)
                    || !rect.rect.stroke_dashes.is_empty()
                    || self.dynamics.iter().any(|entry| {
                        entry.target.as_property().is_some_and(|property| {
                            property.layer_id() == rect.id
                                && matches!(
                                    property.property_type(),
                                    PropType::FillEnabled | PropType::StrokeEnabled
                                )
                        })
                    })
                    || rect
                        .rect
                        .fill_blend_mode
                        .is_some_and(|mode| mode != Default::default());
                if needs_paint_program {
                    let data = LayerData::Rect(rect.clone());
                    let paints = paint_controls::materialize(&data, self.dynamics)?;
                    for diagnostic in paints.diagnostics() {
                        self.warn(Some(rect.id), *diagnostic);
                    }
                    let program = vector_rect_paints_program(
                        rect,
                        transform,
                        transform_animations,
                        transform_id,
                        &paints,
                        self.dynamics,
                        true,
                    )?;
                    let rewritten =
                        rect_dashes::isolate_static_dashed_stroke(rect, self.dynamics, program)?;
                    self.diagnostics.extend(rewritten.diagnostics);
                    self.layers
                        .push(LayerSpec::VectorProgram(rewritten.program));
                    if let Some(timing) = timing {
                        let output = self.layers.pop().expect("native Rectangle program emitted");
                        self.layers.push(LayerSpec::Timed(Box::new(output), timing));
                    }
                    return Ok(());
                }
                let vector = vector_rect(rect, transform)?;
                let animations = rect_animations(
                    self.dynamics,
                    rect.id,
                    transform_id,
                    rect.rect.position,
                    transform_animations,
                    options.transform_3d.is_some(),
                )?;
                if rect.description.starts_with("Editable AE solid;") {
                    self.warn(Some(rect.id), "Edited Solid content is exported as a native vector Rectangle instead of flattening changed paint/geometry into a Solid source.");
                }
                if animations == RectAnimations::default() {
                    self.layers.push(LayerSpec::Rect(vector));
                } else {
                    self.layers
                        .push(LayerSpec::AnimatedRect(vector, animations));
                }
            }
            LayerData::Shape(shape) => {
                let selected = choose_transform(
                    inherited,
                    &shape.transform,
                    shape.id,
                    has_transform_entries(self.dynamics, shape.id),
                )?;
                let (selected_transform, transform_id) =
                    selected.unwrap_or((&shape.transform, shape.id));
                let base_transform = identity_fx_transform();
                let transform = if options.transform_3d.is_some() {
                    &base_transform
                } else {
                    selected_transform
                };
                if timing.is_some_and(|range| range.start_millis != 0)
                    && transform_id != shape.id
                    && has_entries_for(self.dynamics, transform_id)
                {
                    return Err(
                        "Animated Group Transform cannot be flattened onto a later-starting child without rebasing its source clock",
                    );
                }
                let transform_keys = transform_animations_partitioned(
                    self.dynamics,
                    transform_id,
                    transform,
                    shape.id,
                    options.transform_3d.is_some(),
                    true,
                )?;
                let geometry_keys = shape_animations(self.dynamics, shape.id, transform_id)?;
                if shape.shape.strokes.is_empty() && has_stroke_animator(self.dynamics, shape.id) {
                    return Err(
                        "Shape Stroke animator has no owned stroke; static fallback is forbidden",
                    );
                }
                let data = LayerData::Shape(shape.clone());
                let paints = paint_controls::materialize(&data, self.dynamics)?;
                for diagnostic in paints.diagnostics() {
                    self.warn(Some(shape.id), *diagnostic);
                }
                let mut paint_approximations = Vec::new();
                let program = vector_shape_program(
                    shape,
                    transform,
                    (transform_keys, geometry_keys),
                    transform_id,
                    (
                        vector_paint_animations(self.dynamics, shape.id)?,
                        modifier_animations(self.dynamics, shape.id)?,
                    ),
                    self.dynamics,
                    &paints,
                    options.transform_3d.as_mut(),
                    true,
                    false,
                    &mut paint_approximations,
                )?;
                self.layers.push(LayerSpec::VectorProgram(program));
                for (owner, message) in paint_approximations {
                    self.warn(Some(owner), message);
                }
            }
            LayerData::BooleanOperation(boolean) => {
                let selected = choose_transform(
                    inherited,
                    &boolean.transform,
                    boolean.id,
                    has_transform_entries(self.dynamics, boolean.id),
                )?;
                let (selected_transform, transform_id) =
                    selected.unwrap_or((&boolean.transform, boolean.id));
                let base_transform = identity_fx_transform();
                let transform = if options.transform_3d.is_some() {
                    &base_transform
                } else {
                    selected_transform
                };
                if timing.is_some_and(|range| range.start_millis != 0)
                    && transform_id != boolean.id
                    && has_entries_for(self.dynamics, transform_id)
                {
                    return Err(
                        "Animated Group Transform cannot be flattened onto a later-starting Boolean child without rebasing its source clock",
                    );
                }
                if self.dynamics.iter().any(|entry| {
                    entry.target.layer_id() == Some(boolean.id)
                        && entry.target.as_property().is_none_or(|property| {
                            !(matches!(
                                property.property_type(),
                                PropType::AnchorPointX
                                    | PropType::AnchorPointY
                                    | PropType::PositionX
                                    | PropType::PositionY
                                    | PropType::ScaleX
                                    | PropType::ScaleY
                                    | PropType::Rotation
                                    | PropType::Opacity
                                    | PropType::FillColor
                                    | PropType::StrokeColor
                                    | PropType::StrokeWidth
                                    | PropType::StrokeDashOffset
                                    | PropType::StrokeMiterLimit
                                    | PropType::StrokeJoin
                                    | PropType::TrimStart
                                    | PropType::TrimEnd
                                    | PropType::TrimOffset
                            ) || options.transform_3d.is_none()
                                && matches!(
                                    property.property_type(),
                                    PropType::Skew | PropType::SkewAxis
                                )
                                || options.transform_3d.is_some()
                                    && is_native_3d_partition_property(property.property_type()))
                        })
                }) {
                    return Err(
                        "Boolean animator target has no mapped native editable property; static fallback is forbidden",
                    );
                }
                let animations = transform_animations_partitioned(
                    self.dynamics,
                    transform_id,
                    transform,
                    boolean.id,
                    options.transform_3d.is_some(),
                    true,
                )?;
                if boolean.strokes.is_empty() && has_stroke_animator(self.dynamics, boolean.id) {
                    return Err(
                        "Boolean Stroke animator has no owned stroke; static fallback is forbidden",
                    );
                }
                let data = LayerData::BooleanOperation(boolean.clone());
                let paints = paint_controls::materialize(&data, self.dynamics)?;
                for diagnostic in paints.diagnostics() {
                    self.warn(Some(boolean.id), *diagnostic);
                }
                let spec = vector_boolean_program(
                    boolean,
                    transform,
                    animations,
                    transform_id,
                    self.dynamics,
                    &paints,
                    true,
                )?;
                for child in &boolean.layers {
                    let has_ignored_paint = match child.data() {
                        LayerData::Shape(shape) => {
                            !shape.shape.fills.is_empty() || !shape.shape.strokes.is_empty()
                        }
                        LayerData::Rect(rect) => rect.rect.fill_enabled || rect.rect.stroke_enabled,
                        LayerData::BooleanOperation(nested) => {
                            !nested.fills.is_empty() || !nested.strokes.is_empty()
                        }
                        _ => false,
                    };
                    if has_ignored_paint {
                        self.warn(Some(child.id()), "Boolean operand paint is ignored by FX geometry and not reconstructed as an independent native paint; the Boolean's current owned paint is exported once");
                    }
                }
                self.layers.push(LayerSpec::VectorProgram(spec));
            }
            LayerData::Text(text_layer) => {
                let selected = choose_transform(
                    inherited,
                    &text_layer.transform,
                    text_layer.id,
                    has_transform_entries(self.dynamics, text_layer.id),
                )?;
                let (selected_transform, transform_id) =
                    selected.unwrap_or((&text_layer.transform, text_layer.id));
                let base_transform = identity_fx_transform();
                let transform = if options.transform_3d.is_some() {
                    &base_transform
                } else {
                    selected_transform
                };
                if timing.is_some_and(|range| range.start_millis != 0)
                    && transform_id != text_layer.id
                    && has_entries_for(self.dynamics, transform_id)
                {
                    return Err(
                        "Animated Group Transform cannot be flattened onto a later-starting Text child without rebasing its source clock",
                    );
                }
                let lowered = text::lower_with_native_3d(
                    text_layer,
                    transform,
                    transform_id,
                    self.dynamics,
                    text_path_index,
                    options.transform_3d.is_some(),
                )?;
                for diagnostic in lowered.diagnostics {
                    self.warn(Some(text_layer.id), diagnostic);
                }
                self.layers.push(LayerSpec::Text(lowered.spec));
            }
            LayerData::Image(_)
            | LayerData::Video(_)
            | LayerData::Audio(_)
            | LayerData::Media(_) => {
                return Err("Media dispatch bypassed retained whole-document preflight");
            }
            _ => return Err("Layer kind has no native exporter yet"),
        }
        if let Some(timing) = timing {
            let output = self.layers.pop().expect("native visual layer emitted");
            self.layers.push(LayerSpec::Timed(Box::new(output), timing));
        }
        Ok(())
    }

    fn lower_media_layer(
        &mut self,
        layer: &Layer,
        parent: Option<LayerId>,
        inherited: Option<(&Transform, LayerId)>,
        siblings: &[Layer],
        output_visible: bool,
    ) -> Result<(), &'static str> {
        if inherited.is_some() {
            return Err(
                "Media under a nonidentity flattened Group Transform requires a source-backed precomposition",
            );
        }
        if let LayerData::Audio(audio) = layer.data()
            && let fx_schema::LayerPlaybackMapping::Linear { input, .. } = audio.playback.mapping()
            && i128::from(audio.playback.input_range().start.as_millis())
                + i128::from(audio.playback.input_offset_ms())
                != i128::from(input.start.as_millis())
            && let Some(NativeTrack::Keyframes(keys)) =
                track(self.dynamics, layer.id(), PropType::AudioVolume)?
            && keys.keyframes().len() > 1
        {
            // Check the original owner before source variants trim its window
            // and rebase entries into each occurrence's local clock.
            return Err("shifted Audio input clock cannot preserve native Audio Levels key timing");
        }
        let decision = self
            .source_variants
            .remove(&layer.id())
            .unwrap_or(source_variants::SourceVariantDecision::NotAnimated);
        if media_has_takeover_placement(layer) {
            let has_source_variants = !matches!(
                &decision,
                source_variants::SourceVariantDecision::NotAnimated
            );
            return self.lower_media_takeover(layer, has_source_variants);
        }
        match decision {
            source_variants::SourceVariantDecision::NotAnimated => {
                let (options, _) = self.prepare_layer_options(
                    layer,
                    parent,
                    inherited,
                    siblings,
                    self.dynamics,
                    output_visible,
                )?;
                let (selected, owner) = selected_media_transform(layer, inherited, self.dynamics)?;
                self.emit_media_occurrence(
                    layer,
                    self.dynamics,
                    options,
                    selected,
                    owner,
                    output_visible,
                )
            }
            source_variants::SourceVariantDecision::Ready(plan) => match plan.publication {
                source_variants::SourceVariantPublication::DirectOccurrences => {
                    for variant in plan.variants {
                        let (mut options, _) = self.prepare_layer_options(
                            &variant.layer,
                            parent,
                            inherited,
                            siblings,
                            &variant.owner_entries,
                            output_visible,
                        )?;
                        options.fx_id = variant.occurrence_id;
                        if layer.data().effects().iter().any(|record| {
                            matches!(record.data(), fx_schema::EffectData::Identified { id, .. }
                                if self.dynamics.iter().any(|entry| entry.target.effect_id() == Some(*id)))
                        }) {
                            options.effects.clear();
                            self.warn(Some(layer.id()), "Effect stack omitted from switched source occurrence: effect-ID keyframe clocks cannot yet be reminted and rebased per occurrence; footage and its non-effect animation remain editable.");
                        }
                        let (selected, owner) = selected_media_transform(
                            &variant.layer,
                            inherited,
                            &variant.owner_entries,
                        )?;
                        self.emit_media_occurrence(
                            &variant.layer,
                            &variant.owner_entries,
                            options,
                            selected,
                            owner,
                            output_visible,
                        )?;
                    }
                    Ok(())
                }
                source_variants::SourceVariantPublication::Precomposition(handoff) => {
                    let consumed_source_entries = plan.consumed_source_entries;
                    self.emit_variant_precomposition(
                        layer,
                        siblings,
                        plan.variants,
                        &consumed_source_entries,
                        handoff,
                        output_visible,
                    )
                }
            },
            source_variants::SourceVariantDecision::Unsupported { reason, .. } => Err(reason),
        }
    }

    fn lower_media_takeover(
        &mut self,
        layer: &Layer,
        has_source_variants: bool,
    ) -> Result<(), &'static str> {
        if has_source_variants {
            return Err("Half-canvas takeover cannot remap source variants atomically");
        }
        let request = media::request(layer).ok_or("Takeover media has no archive request")?;
        let source = self
            .resolved_media
            .get(request.asset_id.as_str())
            .ok_or("Takeover media archive source was not resolved/staged")?;
        let eligibility = self
            .source_variant_eligibility
            .get(&layer.id())
            .copied()
            .unwrap_or_default();
        let ids = self.next_takeover_ids(layer.id())?;
        let composition_end_millis = i64::try_from(self.end.as_millis())
            .map_err(|_| "Takeover composition endpoint exceeds i64 milliseconds")?;
        let plan = takeover::lower_image_takeover(
            layer,
            source,
            self.dynamics,
            self.dimensions,
            self.duration,
            composition_end_millis,
            self.composition_options,
            native_layer_options(layer)?,
            ids,
            &self.occupied_ids,
            takeover::TakeoverGraphFacts {
                has_source_variants,
                has_external_references: eligibility.has_external_references(),
                has_unsupported_graph_edges: eligibility.has_unsupported_graph_edges(),
            },
        )
        .map_err(takeover_error)?;
        crate::writer::validate_layers(&plan.layers, self.duration).map_err(takeover_error)?;
        for id in plan.required_synthetic_ids {
            if !self.occupied_ids.insert(id) {
                return Err("Takeover synthetic identity was reserved concurrently");
            }
        }
        self.diagnostics.extend(plan.diagnostics);
        self.layers.extend(plan.layers);
        Ok(())
    }

    fn next_takeover_ids(&self, seed: LayerId) -> Result<takeover::TakeoverIds, &'static str> {
        let mut occupied = self.occupied_ids.clone();
        let mut next = seed.value();
        let mut reserve = || loop {
            next = next
                .checked_add(1)
                .ok_or("Takeover synthetic identity space is exhausted")?;
            let candidate = LayerId::new(next);
            if occupied.insert(candidate) {
                return Ok::<_, &'static str>(candidate);
            }
        };
        Ok(takeover::TakeoverIds {
            inner_footage: reserve()?,
            slide_parent: reserve()?,
        })
    }

    fn emit_variant_precomposition(
        &mut self,
        layer: &Layer,
        siblings: &[Layer],
        variants: Vec<source_variants::SourceVariant>,
        consumed_source_entries: &[usize],
        handoff: source_variants::SourceVariantPrecompositionHandoff,
        output_visible: bool,
    ) -> Result<(), &'static str> {
        let transform = media_transform(layer)
            .ok_or("Referenced temporal source precomposition requires visual media")?;
        let Position::TwoD(position) = transform.position else {
            return Err("Referenced temporal source precomposition has no established 3D wrapper");
        };
        if transform.skew != 0.0 || transform.skew_axis != 0.0 {
            return Err("Referenced temporal source precomposition cannot author skew");
        }
        let mut options = native_layer_options(layer)?;
        let lowered_effects = effects::lower(
            layer.data().effects(),
            self.dynamics,
            [f64::from(handoff.width), f64::from(handoff.height)],
        );
        options.effects = lowered_effects.effects;
        options.styles = lowered_effects.styles;
        for warning in lowered_effects.warnings {
            self.warn(Some(layer.id()), warning);
        }
        let path_masks = match layer.data() {
            LayerData::Image(value) => &value.masks,
            LayerData::Video(value) => &value.masks,
            LayerData::Media(value) => &value.masks,
            _ => return Err("Referenced temporal source wrapper is not visual media"),
        };
        let lowered_masks = masks::lower(
            path_masks,
            None,
            masks::MaskOwner {
                id: layer.id(),
                parent: layer.parent_id(),
                transform,
                source_size: [u32::from(handoff.width), u32::from(handoff.height)],
                clock: None,
            },
            siblings,
            self.dynamics,
        );
        for diagnostic in lowered_masks.diagnostics {
            self.warn(Some(layer.id()), diagnostic);
        }
        if output_visible {
            self.consumed_guides.extend(lowered_masks.consumed_guides);
        }
        options.masks = lowered_masks.masks;
        masks::translate(&mut options.masks, [-handoff.origin[0], -handoff.origin[1]]);

        let child_start = self.layers.len();
        let identity = identity_fx_transform();
        for variant in variants {
            let content_layer = media_wrapper_content_view(&variant.layer)
                .map_err(|_| "Referenced temporal source child could not be normalized")?;
            let mut child_options = native_layer_options(&content_layer)?;
            child_options.fx_id = variant.occurrence_id;
            child_options.parent = None;
            child_options.matte = None;
            child_options.masks.clear();
            // Effects belong to the wrapper occurrence, not the reminted source.
            child_options.effects.clear();
            child_options.transform_3d = None;
            let child_dynamics = variant
                .owner_entries
                .into_iter()
                .filter(|entry| !is_transform_entry(entry, variant.occurrence_id))
                .collect::<Vec<_>>();
            self.emit_media_occurrence(
                &content_layer,
                &child_dynamics,
                child_options,
                Some(&identity),
                variant.occurrence_id,
                output_visible,
            )?;
        }
        let mut children = self.layers.drain(child_start..).collect::<Vec<_>>();
        for child in &mut children {
            child
                .translate_composition_root([-handoff.origin[0], -handoff.origin[1]])
                .map_err(|_| "Referenced temporal source child origin translation failed")?;
        }

        let mut wrapper_transform = SolidTransform {
            anchor: transform.anchor_point,
            position,
            scale: transform.scale,
            rotation: transform.rotation,
            opacity: transform.opacity.value(),
        };
        let wrapper_dynamics = self
            .dynamics
            .iter()
            .enumerate()
            .filter(|(index, entry)| {
                entry.target.layer_id() == Some(layer.id())
                    && !consumed_source_entries.contains(index)
            })
            .map(|(_, entry)| entry.clone())
            .collect::<Vec<_>>();
        let mut wrapper_animations =
            solid_transform_animations(&wrapper_dynamics, layer.id(), &wrapper_transform, false)?;
        wrapper_transform.anchor[0] -= handoff.origin[0];
        wrapper_transform.anchor[1] -= handoff.origin[1];
        translate_track(
            wrapper_animations.anchor.as_mut(),
            [-handoff.origin[0], -handoff.origin[1]],
        )?;
        let mut composition_record = crate::schema::CompositionRecord::empty_ae26(
            handoff.width,
            handoff.height,
            self.duration,
        )
        .map_err(|_| "Referenced temporal source composition record failed")?;
        crate::writer::apply_composition_options(&mut composition_record, self.composition_options)
            .map_err(|_| "Referenced temporal source composition options failed")?;
        self.layers.push(LayerSpec::Options(
            Box::new(LayerSpec::Precomposition(PrecompositionSpec {
                collapse_transformations: false,
                name: layer.data().name().to_owned(),
                width: handoff.width,
                height: handoff.height,
                duration: self.duration,
                transform: wrapper_transform,
                transform_animations: wrapper_animations,
                layers: children,
                composition_record: Some(composition_record),
            })),
            options,
        ));
        Ok(())
    }

    fn emit_media_occurrence(
        &mut self,
        layer: &Layer,
        dynamics: &[AnimationGraphEntry],
        mut options: NativeLayerOptions,
        selected_transform: Option<&Transform>,
        transform_owner: LayerId,
        output_visible: bool,
    ) -> Result<(), &'static str> {
        let request = media::request(layer).ok_or("Media layer has no archive request")?;
        let resolved = self
            .resolved_media
            .get(request.asset_id.as_str())
            .ok_or("Media archive source was not resolved/staged")?;
        let mut footage = match selected_transform {
            Some(transform) => {
                media::lower_with_transform(layer, resolved, self.dimensions, transform)?
            }
            None => media::lower(layer, resolved, self.dimensions)?,
        };
        if let LayerData::Audio(audio) = layer.data()
            && let crate::writer::footage::FootageClock::Source(clock) = &footage.clock
            && clock.active_range.duration < audio.playback.input_range().duration
        {
            self.warn(Some(layer.id()), "Audio active span extends past source EOF; native occurrence retains the audible prefix and omits only its silent tail. The original editable active-span length is not retained.");
        }
        if let LayerData::Audio(audio) = layer.data() {
            // Native WAVE occurrences retain the general layer switch. Audio
            // extracted from a movie must keep its video channel disabled.
            options.enabled = output_visible
                && !audio.is_hidden
                && resolved.format == crate::writer::footage::NativeSourceFormat::Wave;
            // FX hidden containers suppress descendant audio as well as pixels.
            // AE's eye switch on the precomposition alone would not mute it.
            footage.audio_enabled &= output_visible;
        }
        if !options.masks.iter().any(|mask| mask.name == "Media Crop")
            && let Some(mut crop) = media::crop_mask(layer, resolved)?
        {
            if !options.masks.is_empty() {
                crop.mode = NativeMaskMode::Intersect;
            }
            options.masks.push(crop);
        }
        // Muted Audio still owns editable levels; the audio switch controls
        // audibility independently of whether those keys can be represented.
        let allow_audio = footage.audio_enabled || matches!(layer.data(), LayerData::Audio(_));
        let animations = solid_transform_animations_partitioned(
            dynamics,
            transform_owner,
            &footage.transform.transform,
            allow_audio,
            options.transform_3d.is_some(),
        )?;
        footage.audio_levels_animation =
            audio::levels_animation(dynamics, layer.id(), allow_audio)?;
        if let LayerData::Audio(audio) = layer.data()
            && audio::exactly_silent(dynamics, layer.id(), audio.volume.as_f64())?
        {
            // Exact static mute is a native switch, not merely very quiet dB.
            footage.audio_enabled = false;
        }
        if footage
            .audio_levels_animation
            .as_ref()
            .is_some_and(|track| {
                track.keys.iter().any(|key| {
                    key.easing
                        .iter()
                        .any(|easing| *easing != KeyframeEasing::Hold)
                })
            })
        {
            self.warn(Some(layer.id()), "AudioVolume continuous gain keys are approximated by native dB Bezier controls; authored key times/values and temporal handles are retained, but between-key audio fidelity is unverified.");
        }
        if footage.audio_levels_db.contains(&-192.0)
            || footage
                .audio_levels_animation
                .as_ref()
                .is_some_and(|track| track.keys.iter().any(|key| key.values.contains(&-192.0)))
        {
            self.warn(Some(layer.id()), "Audio gain at/below -192 dB is stored at a finite floor; constant zero Audio uses the audio-off switch, but animated mathematical zero is only near-silence.");
        }
        media::validate_native(&footage, self.duration)
            .map_err(|_| "Native footage validation rejected current media semantics")?;
        self.layers.push(LayerSpec::Options(
            Box::new(LayerSpec::Footage(footage, animations)),
            options,
        ));
        Ok(())
    }

    fn lower_group(
        &mut self,
        group: &GroupLayer,
        mut options: NativeLayerOptions,
        parent_siblings: &[Layer],
        depth: usize,
        output_visible: bool,
        demand: &hierarchy::Demand,
    ) -> Result<(), &'static str> {
        let mut omitted_effect_warnings = Vec::new();
        let filtered_group = if hierarchy::has_skew(group) {
            let retained = group
                .effects
                .iter()
                .filter_map(|effect| {
                    if let Some(warning) = effects::unmapped_warning(effect) {
                        omitted_effect_warnings.push(warning);
                        None
                    } else {
                        Some(effect.clone())
                    }
                })
                .collect();
            (!omitted_effect_warnings.is_empty()).then(|| {
                let mut retained_group = group.clone();
                retained_group.effects = retained;
                retained_group
            })
        } else {
            None
        };
        let group = filtered_group.as_ref().unwrap_or(group);
        if hierarchy::has_skew(group) {
            hierarchy::validate_skew_source(group)?;
        }
        let background_id = self.available_generated_id(group.id)?;
        let normalized = layout::normalize_group(
            group,
            background_id,
            self.dynamics,
            self.resolved_media,
            self.dimensions,
        )
        .map_err(|_| "Group layout normalization failed")?;
        if normalized
            .group
            .layers
            .iter()
            .any(|layer| layer.id() == background_id)
            && !self.occupied_ids.insert(background_id)
        {
            return Err("Generated Group background identity was reserved concurrently");
        }
        for diagnostic in normalized.diagnostics {
            self.diagnostics.push(diagnostic);
        }
        let mut geometry = normalized.group;
        // Classify a recognized mask's *painted* source, not its very large,
        // unpainted guide. A failed canvas/effect proof restores the ordinary
        // mask path and recomputes the native source plan below.
        let radial = if playback_is_identity(&group.playback)
            && group_has_root_identity_clock(group, self.end)
            && !hierarchy::has_skew(group)
            && options.transform_3d.is_none()
        {
            match radial_wipe::recognize(&geometry, self.dynamics, self.end) {
                Ok(value) => value.filter(|candidate| {
                    !self
                        .source_variant_eligibility
                        .get(&candidate.guide)
                        .is_some_and(|facts| {
                            facts.referenced_as_parent
                                || facts.referenced_as_matte
                                || facts.referenced_as_text_guide
                                || facts.referenced_as_ai_edit_source
                                || facts.referenced_as_segment
                                || facts.referenced_by_animation_dependency
                                || facts.referenced_by_animation_layer_ref
                        })
                }),
                Err(reason) => {
                    self.warn(Some(group.id), format!("Radial Wipe export not admitted: {reason}; ordinary Mask fallback retained."));
                    None
                }
            }
        } else {
            None
        };
        let original_geometry = radial.as_ref().map(|_| {
            let mut original = geometry.clone();
            // Masks are occurrence records, never hierarchy-source inputs.
            original.masks.clear();
            original
        });
        if let Some(radial) = &radial {
            geometry.layers.retain(|child| child.id() != radial.guide);
        }
        if options.transform_3d.is_some() {
            // The sidecar retains the actual occurrence Transform. Classify
            // only the source canvas here; never substitute a 3D Null parent.
            geometry.transform = identity_fx_transform();
        }
        let planar_dynamics;
        let hierarchy_dynamics = if options.transform_3d.is_some() {
            planar_dynamics = self
                .dynamics
                .iter()
                .filter(|entry| {
                    !entry.target.as_property().is_some_and(|target| {
                        target.layer_id() == group.id
                            && is_transform_property(target.property_type())
                    })
                })
                .cloned()
                .collect::<Vec<_>>();
            planar_dynamics.as_slice()
        } else {
            self.dynamics
        };
        let masks = std::mem::take(&mut geometry.masks);
        let mut child_demand = hierarchy::child_demand(
            group,
            &geometry,
            &masks,
            self.dynamics,
            self.dimensions,
            demand,
        );
        let root_output_viewport = depth == 0
            && options.transform_3d.is_none()
            && child_demand.use_root_output_viewport(
                group,
                self.dynamics,
                parent_siblings,
                self.dimensions,
            );
        let spatial_occurrence = options
            .transform_3d
            .as_ref()
            .is_some_and(|(transform, _)| transform.is_three_d);
        if !root_output_viewport && !spatial_occurrence {
            // Separated planar X/Y curves also use the Transform sidecar; its
            // presence alone does not make this occurrence projective.
            // Rejection leaves full bounds in force; only oversized sources use it.
            let _ = child_demand.use_3d_consumer_viewport(group, parent_siblings, self.dynamics);
        }
        if hierarchy::has_skew(&geometry) && !masks.is_empty() {
            return Err("Static Group skew cannot move Group masks across affine helpers");
        }
        let skew_helper_id = hierarchy::has_skew(&geometry)
            .then(|| self.available_generated_id(group.id))
            .transpose()?;
        if skew_helper_id.is_some_and(|helper_id| !self.occupied_ids.insert(helper_id)) {
            return Err("Generated Group skew helper identity was reserved concurrently");
        }
        let clocked = !group_has_root_identity_clock(&geometry, self.end);
        if clocked && group_has_dynamic_mask_properties(&masks, self.dynamics) {
            return Err(
                "Nonidentity Group source clock with dynamic Mask properties requires source-owned mask key placement",
            );
        }

        // Skew changes the spatial lowering, not the occurrence's clock. Normalize
        // short/offset Groups before either precomposition classifier sees them.
        let clock = if clocked {
            Some(
                hierarchy_clock::plan(
                    &geometry,
                    self.dynamics,
                    group_clock_domains(&geometry)?,
                    hierarchy::audio_only(&geometry.layers),
                )
                .map_err(|_| "Group source clock cannot be represented exactly")?,
            )
        } else {
            None
        };
        let mut plan = if let Some(helper_id) = skew_helper_id {
            let (source_geometry, source_end, source_duration) =
                clock
                    .as_ref()
                    .map_or((&geometry, self.end, self.duration), |clock| {
                        (
                            clock.geometry(),
                            Time::from_millis(clock.source_duration_millis),
                            clock.source_duration,
                        )
                    });
            hierarchy::classify_with_skew_helper_and_demand(
                source_geometry,
                source_end,
                source_duration,
                hierarchy_dynamics,
                self.resolved_media,
                self.dimensions,
                child_demand.finite_canvas(),
                helper_id,
            )?
        } else if let Some(clock) = &clock {
            hierarchy::classify_precomposition_with_demand(
                clock.geometry(),
                Time::from_millis(clock.source_duration_millis),
                clock.source_duration,
                hierarchy_dynamics,
                self.resolved_media,
                self.dimensions,
                child_demand.finite_canvas(),
            )?
        } else if masks.is_empty()
            && options.transform_3d.is_none()
            // A Null parent cannot supply the composite alpha sampled by a
            // sibling's track matte; its children would paint independently.
            && !self
                .source_variant_eligibility
                .get(&group.id)
                .is_some_and(|facts| facts.referenced_as_matte)
            && group.effects.is_empty()
            && !group
                .layers
                .iter()
                .any(|layer| matches!(layer.data(), LayerData::Adjustment(_)))
        {
            hierarchy::classify_with_demand(
                &geometry,
                self.end,
                self.duration,
                hierarchy_dynamics,
                self.resolved_media,
                self.dimensions,
                child_demand.finite_canvas(),
            )?
        } else {
            match hierarchy::classify_precomposition_with_demand(
                &geometry,
                self.end,
                self.duration,
                hierarchy_dynamics,
                self.resolved_media,
                self.dimensions,
                child_demand.finite_canvas(),
            ) {
                Ok(plan) => plan,
                Err(reason) if radial.is_some() => {
                    self.warn(Some(group.id), format!("Radial Wipe source bounds not proved ({reason}); ordinary Mask fallback retained."));
                    geometry = original_geometry
                        .as_ref()
                        .expect("candidate saved original geometry")
                        .clone();
                    hierarchy::classify_precomposition_with_demand(
                        &geometry,
                        self.end,
                        self.duration,
                        hierarchy_dynamics,
                        self.resolved_media,
                        self.dimensions,
                        child_demand.finite_canvas(),
                    )?
                }
                Err(reason) => return Err(reason),
            }
        };

        let radial = radial
            .filter(|candidate| !geometry.layers.iter().any(|child| child.id() == candidate.guide))
            .and_then(|candidate| {
            let hierarchy::HierarchyPlan::Precomposition(source) = &plan else {
                return None;
            };
            let (size, origin) = source.mask_space();
            if candidate.covers(size, origin) {
                match candidate.native(size, origin) {
                    Ok(effect) => return Some((candidate, effect)),
                    Err(reason) => self.warn(Some(group.id), format!("Radial Wipe export not admitted: {reason}; ordinary Mask fallback retained.")),
                }
            } else {
                self.warn(Some(group.id), "Radial Wipe export not admitted: finite guide does not cover the complete source canvas at every angle; ordinary Mask fallback retained.");
            }
            None
        });
        if radial.is_none()
            && let Some(original) = original_geometry
        {
            geometry = original;
            plan = hierarchy::classify_precomposition_with_demand(
                &geometry,
                self.end,
                self.duration,
                hierarchy_dynamics,
                self.resolved_media,
                self.dimensions,
                child_demand.finite_canvas(),
            )?;
        }

        if matches!(&plan, hierarchy::HierarchyPlan::Precomposition(plan) if plan.consumer_viewport())
        {
            self.warn(Some(group.id), "Oversized 3D source uses a finite consumer output viewport with unchanged world geometry and camera; native raster edge/alpha differences are an approximation, not pixel-exact fidelity.");
        }
        match &plan {
            hierarchy::HierarchyPlan::Precomposition(precomposition) => {
                match precomposition.collapsed_source() {
                    Some(hierarchy::CollapsedSource::OversizedVector) => {
                        if options.transform_3d.is_some() || clock.is_some() {
                            return Err(
                                "Collapsed vector source requires a 2D identity-clock occurrence",
                            );
                        }
                        self.warn(Some(group.id), "Oversized 2D vector source retains its geometry using native collapse transformations; the source canvas is not an input crop. Non-vector and 3D subtrees are excluded.");
                    }
                    // A source clock is the usual reason Text must precompose.
                    // Masks and matte sampling would rasterize the collapsed layer.
                    Some(hierarchy::CollapsedSource::Text) => {
                        if options.transform_3d.is_some()
                            || !masks.is_empty()
                            || self
                                .source_variant_eligibility
                                .get(&group.id)
                                .is_some_and(|facts| facts.referenced_as_matte)
                        {
                            return Err(
                                "Collapsed Text source requires a 2D occurrence without masks or matte consumers",
                            );
                        }
                        self.warn(Some(group.id), "Text has no FX glyph bounds, so its required precomposition uses native collapse transformations on the root canvas instead of a guessed source canvas. Adobe rendering of collapsed Text, including under a source clock, is unverified.");
                    }
                    None => {}
                }
            }
            hierarchy::HierarchyPlan::Parent(_) => {}
        }

        let (source_end, source_duration) =
            clock.as_ref().map_or((self.end, self.duration), |clock| {
                (
                    Time::from_millis(clock.source_duration_millis),
                    clock.source_duration,
                )
            });
        if let Some(clock) = clock {
            options.source_clock = Some(clock.occurrence_clock);
        }
        for warning in omitted_effect_warnings {
            self.warn(Some(group.id), warning);
        }
        if root_output_viewport {
            self.warn(Some(group.id), "Identity Group at the final root uses the output viewport; child sources/effect inputs and any existing root camera are retained. Nested source viewports require separate support checks; independent native edge/alpha fidelity remains unverified.");
        }

        if !group.effects.is_empty() {
            let hierarchy::HierarchyPlan::Precomposition(precomposition) = &plan else {
                return Err("Group effects require a native precomposition occurrence");
            };
            let (source_size, _) = precomposition.mask_space();
            let (effects, styles) = self.lower_effect_stack(
                group.id,
                &group.effects,
                self.dynamics,
                source_size.map(f64::from),
            );
            options.effects = effects;
            options.styles = styles;
        }

        if let Some((candidate, effect)) = &radial {
            // FX masks precede ordinary effects; native Effect Parade must
            // preserve that order. The guide is already excluded from source
            // geometry and is also consumed in the initial guide probe.
            options.effects.insert(0, effect.clone());
            if output_visible {
                self.consumed_guides.insert(candidate.guide);
            }
        } else if !masks.is_empty() {
            let hierarchy::HierarchyPlan::Precomposition(precomposition) = &plan else {
                return Err("Group masks require a native precomposition occurrence");
            };
            let (source_size, origin) = precomposition.mask_space();
            let lowered = lower_group_masks(
                group,
                &masks,
                parent_siblings,
                &geometry.layers,
                source_size,
                self.dynamics,
            );
            for diagnostic in lowered.diagnostics {
                self.warn(Some(group.id), diagnostic);
            }
            if output_visible {
                self.consumed_guides.extend(lowered.consumed_guides);
            }
            options.masks = lowered.masks;
            masks::translate(&mut options.masks, [-origin[0], -origin[1]]);
        }

        let child_start = self.layers.len();
        let previous_precomposition = self.inside_precomposition;
        let previous_clock = (self.end, self.duration);
        if matches!(plan, hierarchy::HierarchyPlan::Precomposition(_)) {
            // Descendant lifetimes are relative to this source, not the root
            // document. Keep exact milliseconds for clock identity checks.
            self.inside_precomposition = true;
            self.end = source_end;
            self.duration = source_duration;
        }
        for child in &geometry.layers {
            self.layer(
                child,
                Some(group.id),
                None,
                &geometry.layers,
                depth + 1,
                output_visible,
                child_demand.propagated(),
            );
        }
        self.inside_precomposition = previous_precomposition;
        (self.end, self.duration) = previous_clock;
        let mut children = self.layers.drain(child_start..).collect();
        omit_dangling_reference_owners(
            &mut children,
            &composition_layer_ids(&geometry.layers),
            &mut self.diagnostics,
            &mut self.omitted_layer_ids,
        )
        .map_err(|_| "Native Group dependency pruning failed")?;
        match plan {
            hierarchy::HierarchyPlan::Parent(plan) => {
                self.layers.extend(
                    plan.finish(children, options)
                        .map_err(|_| "Native Null hierarchy finalization failed")?,
                );
            }
            hierarchy::HierarchyPlan::Precomposition(plan) => {
                self.layers.push(
                    plan.finish(children, options, Some(self.composition_options))
                        .map_err(|_| "Native precomposition finalization failed")?,
                );
            }
        }
        Ok(())
    }

    fn available_generated_id(&self, seed: LayerId) -> Result<LayerId, &'static str> {
        let mut value = seed.value();
        loop {
            value = if value == u64::MAX { 1 } else { value + 1 };
            if value == seed.value() {
                return Err("Generated FX layer identity space is exhausted");
            }
            let candidate = LayerId::new(value);
            if !self.occupied_ids.contains(&candidate) {
                return Ok(candidate);
            }
        }
    }

    fn check_group(&self, group: &GroupLayer) -> Result<(), &'static str> {
        if !group.fills.is_empty()
            || [
                group.padding_top,
                group.padding_right,
                group.padding_bottom,
                group.padding_left,
                group.corner_radius_top_left,
                group.corner_radius_top_right,
                group.corner_radius_bottom_right,
                group.corner_radius_bottom_left,
            ]
            .iter()
            .any(|value| value.value() != 0.0)
        {
            return Err(
                "Group backgrounds, padding or corners cannot be removed without changing semantics",
            );
        }
        let identity_clock = match group.playback.mapping() {
            LayerPlaybackMapping::Linear { input, output } => {
                group.playback.input_offset_ms() == 0 && input == output
            }
            LayerPlaybackMapping::TimeRemap { property } => {
                let keys = property.keyframes();
                group.playback.input_offset_ms() == 0
                    && keys.first().is_some_and(|key| key.time == Time::ZERO)
                    && keys.last().is_some_and(|key| key.time >= self.end)
                    && keys.iter().all(|key| {
                        key.time == key.value && key.easing == PropertyKeyframeEasing::Linear
                    })
            }
        };
        if !identity_clock {
            return Err("Nonidentity source clock requires native timing/remap export");
        }
        Ok(())
    }
}

fn playback_active_range(playback: &LayerPlayback) -> TimeRangeProperty {
    playback.input_range()
}

fn playback_time_remap(playback: &LayerPlayback) -> Option<&TimeRemapProperty> {
    match playback.mapping() {
        LayerPlaybackMapping::TimeRemap { property } => Some(property),
        LayerPlaybackMapping::Linear { .. } => None,
    }
}

fn playback_is_identity(playback: &LayerPlayback) -> bool {
    playback.input_offset_ms() == 0
        && matches!(
            playback.mapping(),
            LayerPlaybackMapping::Linear { input, output } if input == output
        )
}

fn layer_is_hidden(layer: &Layer) -> bool {
    match layer.data() {
        LayerData::Adjustment(value) => value.is_hidden,
        LayerData::Media(value) => value.is_hidden,
        LayerData::Text(value) => value.is_hidden,
        LayerData::Video(value) => value.is_hidden,
        LayerData::Image(value) => value.is_hidden,
        LayerData::Rect(value) => value.is_hidden,
        LayerData::Shape(value) => value.is_hidden,
        LayerData::Group(value) => value.is_hidden,
        LayerData::BooleanOperation(value) => value.is_hidden,
        _ => false,
    }
}

// Structural containment takes precedence over an explicit transform parent,
// matching the FX runtime's layer collection semantics.
fn effective_parent(layer: &Layer, structural_parent: Option<LayerId>) -> Option<LayerId> {
    structural_parent.or_else(|| layer.parent_id())
}

fn inline_vector_group(group: &GroupLayer) -> bool {
    group.layers.iter().all(is_vector_hierarchy)
        && identity(&group.transform)
        && !group.is_hidden
        && !group.motion_blur
        && group.blend_mode == Default::default()
        && group.track_matte.is_none()
        && group.masks.is_empty()
        && group.effects.is_empty()
        && playback_is_identity(&group.playback)
        && group.fills.is_empty()
        && group_layout_is_empty(group)
}

fn effectful_inline_vector_group(group: &GroupLayer) -> bool {
    !group.effects.is_empty()
        && group.layers.iter().all(|layer| {
            let LayerData::Shape(shape) = layer.data() else {
                return false;
            };
            shape.parent.is_none_or(|parent| parent == group.id)
                && !shape.is_hidden
                && !shape.motion_blur
                && shape.track_matte.is_none()
                && shape.masks.is_empty()
                && shape.effects.is_empty()
                && shape.active_range.start == playback_active_range(&group.playback).start
                && shape.active_range.end() >= playback_active_range(&group.playback).end()
        })
        && identity(&group.transform)
        && !group.is_hidden
        && !group.motion_blur
        && group.blend_mode == Default::default()
        && group.track_matte.is_none()
        && group.masks.is_empty()
        && playback_is_identity(&group.playback)
        && playback_active_range(&group.playback).start == Time::ZERO
        && playback_active_range(&group.playback).end() > Time::ZERO
        && group.fills.is_empty()
        && group_layout_is_empty(group)
}

fn group_layout_is_empty(group: &GroupLayer) -> bool {
    [
        group.padding_top.value(),
        group.padding_right.value(),
        group.padding_bottom.value(),
        group.padding_left.value(),
        group.corner_radius_top_left.value(),
        group.corner_radius_top_right.value(),
        group.corner_radius_bottom_right.value(),
        group.corner_radius_bottom_left.value(),
    ]
    .into_iter()
    .all(|value| value == 0.0)
}

fn effect_is_layer_style(record: &fx_schema::EffectRecord) -> bool {
    let payload = match record.data() {
        EffectData::Identified { effect, .. } | EffectData::Legacy(effect) => effect,
    };
    matches!(payload, EffectPayload::Known(effect) if layer_styles::is_layer_style(effect))
}

fn group_has_root_identity_clock(group: &GroupLayer, composition_end: Time) -> bool {
    let active_range = playback_active_range(&group.playback);
    active_range.start == Time::ZERO
        && active_range.end() >= composition_end
        && match group.playback.mapping() {
            LayerPlaybackMapping::Linear { input, output } => {
                group.playback.input_offset_ms() == 0 && input == output
            }
            LayerPlaybackMapping::TimeRemap { property } => {
                let keys = property.keyframes();
                group.playback.input_offset_ms() == 0
                    && keys.first().is_some_and(|key| key.time == Time::ZERO)
                    && keys.last().is_some_and(|key| key.time >= composition_end)
                    && keys.iter().all(|key| {
                        key.time == key.value && key.easing == PropertyKeyframeEasing::Linear
                    })
            }
        }
}

fn group_clock_domains(
    group: &GroupLayer,
) -> Result<hierarchy_clock::ChildClockDomains, &'static str> {
    let active_range = playback_active_range(&group.playback);
    let default_end = active_range.duration.as_millis();
    let lifetime_end = group
        .layers
        .iter()
        .map(|layer| layer.active_range().end().as_millis())
        .max()
        .unwrap_or(default_end);
    // An identity occurrence can only visit its finite active source interval.
    // Imported source primitives deliberately have unbounded lifetimes; those
    // tails must not become the native container duration. Leave child timing
    // intact and retain the full explicit domain for other clocks.
    let lifetime_end = if group_has_root_identity_clock(group, active_range.end()) {
        lifetime_end.min(default_end)
    } else {
        lifetime_end
    };
    let duration = default_end.max(lifetime_end);
    if duration == 0 {
        return Err("Group source clock has no positive explicit child/default domain");
    }
    Ok(hierarchy_clock::ChildClockDomains {
        default_domain: fx_schema::TimeRangeProperty::new(
            Time::ZERO,
            fx_schema::Duration::from_millis(default_end),
        ),
        lifetime_domain: fx_schema::TimeRangeProperty::new(
            Time::ZERO,
            fx_schema::Duration::from_millis(lifetime_end),
        ),
    })
}

fn group_has_dynamic_mask_properties(
    group_masks: &[fx_schema::PathMask],
    dynamics: &[AnimationGraphEntry],
) -> bool {
    let ids = group_masks
        .iter()
        .map(|mask| mask.id)
        .collect::<BTreeSet<_>>();
    dynamics.iter().any(|entry| {
        entry
            .target
            .fx_item_id()
            .is_some_and(|id| ids.contains(&id))
            && effective_constant(&entry.animator).is_none()
    })
}

fn lower_group_masks(
    group: &GroupLayer,
    group_masks: &[fx_schema::PathMask],
    parent_siblings: &[Layer],
    direct_children: &[Layer],
    source_size: [u32; 2],
    dynamics: &[AnimationGraphEntry],
) -> masks::LoweredMasks {
    let identity = identity_fx_transform();
    let mut output = masks::LoweredMasks::default();
    for (index, mask) in group_masks.iter().enumerate() {
        let direct_child = mask
            .layer
            .is_some_and(|id| direct_children.iter().any(|layer| layer.id() == id));
        let (parent, transform, siblings) = if direct_child {
            (Some(group.id), &identity, direct_children)
        } else {
            (group.parent, &group.transform, parent_siblings)
        };
        let mut lowered = masks::lower(
            std::slice::from_ref(mask),
            None,
            masks::MaskOwner {
                id: group.id,
                parent,
                transform,
                source_size,
                clock: playback_is_identity(&group.playback)
                    .then_some(playback_active_range(&group.playback)),
            },
            siblings,
            dynamics,
        );
        if let Some(spec) = lowered.masks.first_mut() {
            spec.name = format!("Mask {}", index + 1);
        }
        output.masks.append(&mut lowered.masks);
        output.consumed_guides.append(&mut lowered.consumed_guides);
        output.diagnostics.extend(
            lowered
                .diagnostics
                .into_iter()
                .map(|message| format!("Mask {}: {message}", index + 1)),
        );
    }
    output
}

fn mask_and_transform(
    layer: &Layer,
) -> Option<(
    &Transform,
    &[fx_schema::PathMask],
    Option<&fx_schema::TextPathOptions>,
)> {
    match layer.data() {
        LayerData::Adjustment(value) => Some((&value.transform, &value.masks, None)),
        LayerData::Group(value) => Some((&value.transform, &value.masks, None)),
        LayerData::Rect(value) => Some((&value.transform, &value.masks, None)),
        LayerData::Shape(value) => Some((&value.transform, &value.masks, None)),
        LayerData::BooleanOperation(value) => Some((&value.transform, &value.masks, None)),
        LayerData::Text(value) => {
            Some((&value.transform, &value.masks, value.path_options.as_ref()))
        }
        LayerData::Image(value) => Some((&value.transform, &value.masks, None)),
        LayerData::Video(value) => Some((&value.transform, &value.masks, None)),
        LayerData::Media(value) => Some((&value.transform, &value.masks, None)),
        _ => None,
    }
}

fn native_layer_options(layer: &Layer) -> Result<NativeLayerOptions, &'static str> {
    use fx_schema::layer::{BlendMode, TrackMatteType};

    let (hidden, blend, matte, masks, motion_blur) = match layer.data() {
        LayerData::Adjustment(value) => (
            value.is_hidden,
            value.blend_mode,
            value.track_matte.as_ref(),
            &value.masks,
            false,
        ),
        LayerData::Group(value) => (
            value.is_hidden,
            value.blend_mode,
            value.track_matte.as_ref(),
            &value.masks,
            value.motion_blur,
        ),
        LayerData::Rect(value) => (
            value.is_hidden,
            value.blend_mode,
            value.track_matte.as_ref(),
            &value.masks,
            value.motion_blur,
        ),
        LayerData::Shape(value) => (
            value.is_hidden,
            value.blend_mode,
            value.track_matte.as_ref(),
            &value.masks,
            value.motion_blur,
        ),
        LayerData::BooleanOperation(value) => (
            value.is_hidden,
            value.blend_mode,
            value.track_matte.as_ref(),
            &value.masks,
            value.motion_blur,
        ),
        LayerData::Text(value) => (
            value.is_hidden,
            value.blend_mode,
            value.track_matte.as_ref(),
            &value.masks,
            value.motion_blur,
        ),
        LayerData::Image(value) => (
            value.is_hidden,
            value.blend_mode,
            value.track_matte.as_ref(),
            &value.masks,
            value.motion_blur,
        ),
        LayerData::Video(value) => (
            value.is_hidden,
            value.blend_mode,
            value.track_matte.as_ref(),
            &value.masks,
            value.motion_blur,
        ),
        LayerData::Media(value) => (
            value.is_hidden,
            value.blend_mode,
            value.track_matte.as_ref(),
            &value.masks,
            value.motion_blur,
        ),
        _ => {
            return Ok(NativeLayerOptions {
                fx_id: layer.id(),
                parent: None,
                matte: None,
                // An Audio-only MOV must not regain video through the common
                // options wrapper. Its audio switch is set by footage lowering.
                enabled: !matches!(layer.data(), LayerData::Audio(_)),
                adjustment_layer: false,
                motion_blur: false,
                blend_mode: 2,
                masks: Vec::new(),
                effects: Vec::new(),
                styles: Vec::new(),
                source_clock: None,
                transform_3d: None,
            });
        }
    };
    let _ = masks;
    let blend_mode = match blend {
        BlendMode::Normal => 2,
        BlendMode::Add => 4,
        BlendMode::Multiply => 5,
        BlendMode::Screen => 6,
        BlendMode::Overlay => 7,
        BlendMode::SoftLight => 8,
        BlendMode::HardLight => 9,
        BlendMode::Darken => 10,
        BlendMode::Lighten => 11,
        BlendMode::ClassicDifference => 12,
        BlendMode::Hue => 13,
        BlendMode::Saturation => 14,
        BlendMode::Color => 15,
        BlendMode::Luminosity => 16,
        BlendMode::ClassicColorDodge => 23,
        BlendMode::ClassicColorBurn => 24,
        BlendMode::Exclusion => 25,
        BlendMode::Difference => 26,
        BlendMode::ColorDodge => 27,
        BlendMode::ColorBurn => 28,
        BlendMode::LinearBurn => 30,
        BlendMode::LinearLight => 31,
        BlendMode::VividLight => 32,
        BlendMode::PinLight => 33,
        BlendMode::HardMix => 34,
        BlendMode::LighterColor => 35,
        BlendMode::DarkerColor => 36,
        BlendMode::Subtract => 37,
        BlendMode::Divide => 38,
    };
    let matte = matte.map(|matte| NativeMatteRef {
        layer: matte.layer,
        mode: match matte.mode {
            TrackMatteType::Alpha => 1,
            TrackMatteType::AlphaInverted => 2,
            TrackMatteType::Luma => 3,
            TrackMatteType::LumaInverted => 4,
        },
    });
    Ok(NativeLayerOptions {
        fx_id: layer.id(),
        parent: None,
        matte,
        enabled: !hidden,
        adjustment_layer: matches!(layer.data(), LayerData::Adjustment(_)),
        motion_blur,
        blend_mode,
        masks: Vec::new(),
        effects: Vec::new(),
        styles: Vec::new(),
        source_clock: None,
        transform_3d: None,
    })
}

fn group_transform_is_animated(group: &GroupLayer, dynamics: &[AnimationGraphEntry]) -> bool {
    has_transform_entries(dynamics, group.id)
        || group.layers.iter().any(|layer| {
            if let LayerData::Group(child) = layer.data() {
                group_transform_is_animated(child, dynamics)
            } else {
                false
            }
        })
}

fn is_vector_hierarchy(layer: &Layer) -> bool {
    match layer.data() {
        LayerData::Rect(_) | LayerData::Shape(_) | LayerData::BooleanOperation(_) => true,
        // Preserve occurrence opacity as a compositing boundary rather than
        // collapsing it into nested paint/group opacity. Other affine values
        // and identity source clocks are checked by vector-program lowering.
        LayerData::Group(group) => {
            group.transform.opacity.value() == 100.0 && group.layers.iter().all(is_vector_hierarchy)
        }
        _ => false,
    }
}

fn has_reflected_gradient(layer: &Layer) -> bool {
    let reflected = |paint: &ShapePaint| {
        matches!(
            paint,
            ShapePaint::Gradient {
                gradient_type: ShapeGradientType::Reflected,
                ..
            }
        )
    };
    match layer.data() {
        LayerData::Rect(rect) => rect.rect.fill_paint.as_ref().is_some_and(reflected),
        LayerData::Shape(shape) => {
            shape.shape.fills.iter().any(|fill| reflected(&fill.paint))
                || shape
                    .shape
                    .strokes
                    .iter()
                    .any(|stroke| reflected(&stroke.paint))
        }
        LayerData::BooleanOperation(boolean) => {
            boolean.fills.iter().any(|fill| reflected(&fill.paint))
                || boolean
                    .strokes
                    .iter()
                    .any(|stroke| reflected(&stroke.paint))
        }
        _ => false,
    }
}

fn collect_reflected_gradient_owners(layer: &Layer, owners: &mut Vec<LayerId>) {
    if has_reflected_gradient(layer) {
        owners.push(layer.id());
    }
    if let LayerData::Group(group) = layer.data() {
        for child in &group.layers {
            collect_reflected_gradient_owners(child, owners);
        }
    }
}

fn vector_group_program(
    group: &GroupLayer,
    transform: &Transform,
    dynamics: &[AnimationGraphEntry],
    depth: usize,
    paint_approximations: &mut Vec<(LayerId, &'static str)>,
) -> Result<VectorLayerSpec, &'static str> {
    if depth >= 48 {
        return Err("Vector Group hierarchy exceeds native depth limit");
    }
    let range = playback_active_range(&group.playback);
    let children = group
        .layers
        .iter()
        .map(|child| {
            vector_hierarchy_group(child, range, dynamics, depth + 1, paint_approximations)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let animations = group_animations(dynamics, group.id, transform)?;
    Ok(VectorLayerSpec {
        name: group.name.clone(),
        transform: identity_solid_transform(),
        transform_animations: TransformAnimations::default(),
        contents: vec![VectorContent::AnimatedGroup(
            VectorGroupSpec {
                name: group.name.clone(),
                blend_mode: group.blend_mode,
                transform: vector_group_transform(transform)?,
                contents: children,
            },
            animations,
        )],
    })
}

fn effectful_vector_group_program(
    group: &GroupLayer,
    dynamics: &[AnimationGraphEntry],
    depth: usize,
    paint_approximations: &mut Vec<(LayerId, &'static str)>,
) -> Result<VectorLayerSpec, &'static str> {
    if depth >= 48 {
        return Err("Vector Group hierarchy exceeds native depth limit");
    }
    let children = group
        .layers
        .iter()
        .map(|child| {
            let LayerData::Shape(shape) = child.data() else {
                return Err("Effectful vector Group accepts only direct Shape children");
            };
            let transform_keys = transform_animations_partitioned(
                dynamics,
                shape.id,
                &shape.transform,
                shape.id,
                false,
                true,
            )?;
            if transform_keys.position.as_ref().is_some_and(|track| {
                track.keys.iter().any(|key| {
                    key.spatial_in
                        .iter()
                        .chain(&key.spatial_out)
                        .any(|value| *value != 0.0)
                })
            }) {
                return Err(
                    "Nested vector Position spatial tangents need an independently addressable layer Transform",
                );
            }
            let geometry_keys = shape_animations(dynamics, shape.id, shape.id)?;
            if shape.shape.strokes.is_empty() && has_stroke_animator(dynamics, shape.id) {
                return Err(
                    "Shape Stroke animator has no owned stroke; static fallback is forbidden",
                );
            }
            let data = LayerData::Shape(shape.clone());
            let paints = paint_controls::materialize(&data, dynamics)?;
            let program = vector_shape_program(
                shape,
                &shape.transform,
                (transform_keys, geometry_keys),
                shape.id,
                (
                    vector_paint_animations(dynamics, shape.id)?,
                    modifier_animations(dynamics, shape.id)?,
                ),
                dynamics,
                &paints,
                None,
                true,
                true,
                paint_approximations,
            )?;
            let mut child_group = vector_program_as_group(program)?;
            child_group.blend_mode = shape.blend_mode;
            Ok(VectorContent::Group(child_group))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(VectorLayerSpec {
        name: group.name.clone(),
        transform: identity_solid_transform(),
        transform_animations: TransformAnimations::default(),
        contents: vec![VectorContent::Group(VectorGroupSpec {
            name: group.name.clone(),
            blend_mode: group.blend_mode,
            transform: vector_group_transform(&group.transform)?,
            contents: children,
        })],
    })
}

fn vector_hierarchy_group(
    layer: &Layer,
    range: fx_schema::TimeRangeProperty,
    dynamics: &[AnimationGraphEntry],
    depth: usize,
    paint_approximations: &mut Vec<(LayerId, &'static str)>,
) -> Result<VectorContent, &'static str> {
    if depth >= 48 {
        return Err("Vector Group hierarchy exceeds native depth limit");
    }
    // Native vector contents inherit the enclosing layer's visibility. A static
    // child covering that whole interval need not have the exact same out point
    // (imported Solid content deliberately has an effectively unbounded span).
    let child_range = layer.active_range();
    // These children are structurally contained; their optional explicit
    // parent field cannot override that ownership in the FX runtime.
    if child_range.start != range.start || child_range.end() < range.end() {
        return Err(
            "Vector hierarchy child clock differs from its Group; exact precomposition timing is required",
        );
    }
    if !matches!(layer.data(), LayerData::Group(_))
        && dynamics
            .iter()
            .filter(|entry| entry.target.layer_id() == Some(layer.id()))
            .any(|entry| {
                entry
                    .target
                    .as_property()
                    .is_none_or(|property| property.property_type() != PropType::ShapePath)
            })
    {
        return Err(
            "Animated child in vector hierarchy needs scoped native key tracks; static fallback is forbidden",
        );
    }
    let program = match layer.data() {
        LayerData::Shape(shape) => {
            let data = LayerData::Shape(shape.clone());
            let paints = paint_controls::materialize(&data, dynamics)?;
            vector_shape_program(
                shape,
                &shape.transform,
                (
                    TransformAnimations::default(),
                    GeometryAnimations::default(),
                ),
                shape.id,
                (PaintTracks::default(), ModifierTracks::default()),
                dynamics,
                &paints,
                None,
                false,
                false,
                paint_approximations,
            )?
        }
        LayerData::Rect(rect) => vector_rect_program(rect, dynamics)?,
        LayerData::BooleanOperation(boolean) => {
            let data = LayerData::BooleanOperation(boolean.clone());
            let paints = paint_controls::materialize(&data, dynamics)?;
            vector_boolean_program(
                boolean,
                &boolean.transform,
                TransformAnimations::default(),
                boolean.id,
                dynamics,
                &paints,
                false,
            )?
        }
        LayerData::Group(group) => {
            if group.is_hidden
                || group.motion_blur
                || group.blend_mode != Default::default()
                || group.track_matte.is_some()
                || !group.masks.is_empty()
                || !group.effects.is_empty()
                || !group.fills.is_empty()
                || group.padding_top.value() != 0.0
                || group.padding_right.value() != 0.0
                || group.padding_bottom.value() != 0.0
                || group.padding_left.value() != 0.0
                || group.corner_radius_top_left.value() != 0.0
                || group.corner_radius_top_right.value() != 0.0
                || group.corner_radius_bottom_right.value() != 0.0
                || group.corner_radius_bottom_left.value() != 0.0
                || !group_has_root_identity_clock(group, range.end())
            {
                return Err(
                    "Nested Group flags/background/clock need exact native hierarchy records",
                );
            }
            let contents = group
                .layers
                .iter()
                .map(|child| {
                    vector_hierarchy_group(child, range, dynamics, depth + 1, paint_approximations)
                })
                .collect::<Result<Vec<_>, _>>()?;
            return Ok(VectorContent::AnimatedGroup(
                VectorGroupSpec {
                    name: group.name.clone(),
                    blend_mode: group.blend_mode,
                    transform: vector_group_transform(&group.transform)?,
                    contents,
                },
                group_animations(dynamics, group.id, &group.transform)?,
            ));
        }
        _ => return Err("Non-vector child cannot be encoded in a native Shape hierarchy"),
    };
    Ok(VectorContent::Group(vector_program_as_group(program)?))
}

fn vector_program_as_group(program: VectorLayerSpec) -> Result<VectorGroupSpec, &'static str> {
    if program.transform_animations != TransformAnimations::default() {
        return Err("Nested vector layer unexpectedly carries layer key tracks");
    }
    Ok(VectorGroupSpec {
        name: program.name,
        blend_mode: Default::default(),
        transform: VectorGroupTransform {
            anchor: program.transform.anchor,
            position: program.transform.position,
            scale: program.transform.scale,
            skew: 0.0,
            skew_axis: 0.0,
            rotation: program.transform.rotation,
            opacity: program.transform.opacity,
        },
        contents: program.contents,
    })
}

fn vector_rect_paints_program(
    layer: &RectLayer,
    transform: &Transform,
    animations: TransformAnimations,
    transform_id: LayerId,
    paints: &paint_controls::PaintMaterialization,
    dynamics: &[AnimationGraphEntry],
    allow_layer_sidecars: bool,
) -> Result<VectorLayerSpec, &'static str> {
    if !allow_layer_sidecars
        && (layer.is_hidden
            || layer.motion_blur
            || layer.blend_mode != Default::default()
            || layer.track_matte.is_some()
            || !layer.masks.is_empty()
            || !layer.effects.is_empty())
    {
        return Err("Rectangle compositing flags require unsupported native records");
    }
    let geometry = rect_geometry::lower(layer, dynamics)?;
    let mut contents = vec![VectorContent::Geometry {
        geometry: geometry.geometry,
        animations: geometry.animations,
    }];
    for fill in paints.fills() {
        contents.push(VectorContent::Paint(VectorPaintSpec::Fill {
            paint: fill.paint.clone(),
            fill_rule: fill.fill_rule,
            blend_mode: fill.blend_mode,
            opacity: fill.opacity * 100.0,
            animations: VectorPaintAnimations::default(),
        }));
    }
    for stroke in paints.strokes() {
        contents.push(VectorContent::Paint(VectorPaintSpec::Stroke {
            paint: stroke.paint.clone(),
            blend_mode: stroke.blend_mode,
            opacity: stroke.opacity * 100.0,
            width: stroke.width.value(),
            cap: stroke.cap,
            join: stroke.join,
            miter_limit: stroke.miter_limit,
            dashes: stroke_dashes(std::slice::from_ref(stroke))?,
            animations: VectorPaintAnimations::default(),
        }));
    }
    if contents.len() == 1 {
        return Err("Rectangle has no enabled paint");
    }
    paints.apply_owned_tracks(&mut contents)?;
    let (layer_transform, contents) = program_transform_with_mode(
        transform,
        animations,
        dynamics,
        transform_id,
        contents,
        false,
    )?;
    Ok(VectorLayerSpec {
        name: layer.name.clone(),
        transform: layer_transform.0,
        transform_animations: layer_transform.1,
        contents,
    })
}

fn vector_rect_program(
    layer: &RectLayer,
    dynamics: &[AnimationGraphEntry],
) -> Result<VectorLayerSpec, &'static str> {
    let data = LayerData::Rect(layer.clone());
    let paints = paint_controls::materialize(&data, dynamics)?;
    vector_rect_paints_program(
        layer,
        &layer.transform,
        TransformAnimations::default(),
        layer.id,
        &paints,
        dynamics,
        false,
    )
}

fn identity_fx_transform() -> Transform {
    Transform {
        anchor_point: [0.0; 2],
        position: Position::TwoD([0.0; 2]),
        scale: [100.0; 2],
        rotation: 0.0,
        skew: 0.0,
        skew_axis: 0.0,
        rotation_x: 0.0,
        rotation_y: 0.0,
        orientation: [0.0; 3],
        opacity: fx_schema::PercentageProperty::new(100.0).expect("100 is a valid percentage"),
    }
}

fn media_has_takeover_placement(layer: &Layer) -> bool {
    match layer.data() {
        LayerData::Image(value) => value.placement.is_some(),
        LayerData::Video(value) => value.placement.is_some(),
        LayerData::Media(value) => value.placement.is_some(),
        _ => false,
    }
}

fn takeover_error(error: AepWriteError) -> &'static str {
    match error {
        AepWriteError::Invalid(message) => message,
        AepWriteError::NoConvertiblePicture(_) => "selected AEP scope has no convertible picture",
        AepWriteError::InvalidDocument(_) => {
            "Takeover graph contains invalid document identity or source planning"
        }
        AepWriteError::Binary(crate::rifx::RifxError::Invalid(message))
        | AepWriteError::Binary(crate::rifx::RifxError::Limit(message))
        | AepWriteError::Record(crate::schema::RecordError::Invalid(message)) => message,
        AepWriteError::Record(crate::schema::RecordError::Length { .. }) => {
            "Takeover native graph constructed a record with an invalid length"
        }
    }
}

fn media_transform(layer: &Layer) -> Option<&Transform> {
    match layer.data() {
        LayerData::Image(value) => Some(&value.transform),
        LayerData::Video(value) => Some(&value.transform),
        LayerData::Media(value) => Some(&value.transform),
        _ => None,
    }
}

fn media_geometry_view(layer: &Layer) -> Result<Layer, serde_json::Error> {
    let mut data = layer.data().clone();
    match &mut data {
        LayerData::Image(value) => value.transform = identity_fx_transform(),
        LayerData::Video(value) => value.transform = identity_fx_transform(),
        LayerData::Media(value) => value.transform = identity_fx_transform(),
        _ => {}
    }
    Layer::from_data(&data)
}

fn media_wrapper_content_view(layer: &Layer) -> Result<Layer, serde_json::Error> {
    let mut data = layer.data().clone();
    macro_rules! clear_wrapper {
        ($value:expr) => {{
            $value.is_hidden = false;
            $value.blend_mode = fx_schema::layer::BlendMode::Normal;
            $value.track_matte = None;
            $value.masks.clear();
            $value.motion_blur = false;
            $value.transform = identity_fx_transform();
        }};
    }
    match &mut data {
        LayerData::Image(value) => clear_wrapper!(value),
        LayerData::Video(value) => clear_wrapper!(value),
        LayerData::Media(value) => clear_wrapper!(value),
        _ => {}
    }
    Layer::from_data(&data)
}

fn selected_media_transform<'a>(
    layer: &'a Layer,
    inherited: Option<(&'a Transform, LayerId)>,
    dynamics: &[AnimationGraphEntry],
) -> Result<(Option<&'a Transform>, LayerId), &'static str> {
    let Some(current) = media_transform(layer) else {
        return Ok((None, layer.id()));
    };
    let selected = choose_transform(
        inherited,
        current,
        layer.id(),
        has_transform_entries(dynamics, layer.id()),
    )?;
    let (transform, owner) = selected.unwrap_or((current, layer.id()));
    Ok((Some(transform), owner))
}

fn is_transform_entry(entry: &AnimationGraphEntry, owner: LayerId) -> bool {
    entry.target.as_property().is_some_and(|property| {
        property.layer_id() == owner && is_transform_property(property.property_type())
    })
}

fn is_transform_property(property: PropType) -> bool {
    matches!(
        property,
        PropType::AnchorPointX
            | PropType::AnchorPointY
            | PropType::PositionX
            | PropType::PositionY
            | PropType::PositionZ
            | PropType::ScaleX
            | PropType::ScaleY
            | PropType::Rotation
            | PropType::RotationX
            | PropType::RotationY
            | PropType::OrientationX
            | PropType::OrientationY
            | PropType::OrientationZ
            | PropType::Opacity
            | PropType::Skew
            | PropType::SkewAxis
    )
}

fn is_native_3d_partition_property(property: PropType) -> bool {
    matches!(
        property,
        PropType::PositionZ
            | PropType::RotationX
            | PropType::RotationY
            | PropType::OrientationX
            | PropType::OrientationY
            | PropType::OrientationZ
    )
}

fn translate_track(track: Option<&mut NumericTrack>, offset: [f64; 2]) -> Result<(), &'static str> {
    let Some(track) = track else {
        return Ok(());
    };
    for key in &mut track.keys {
        if key.values.len() < 2 {
            return Err("Native anchor key has fewer than two dimensions");
        }
        key.values[0] += offset[0];
        key.values[1] += offset[1];
    }
    Ok(())
}

fn identity(transform: &Transform) -> bool {
    transform.anchor_point == [0.0; 2]
        && transform.position == Position::TwoD([0.0; 2])
        && transform.scale == [100.0; 2]
        && transform.rotation == 0.0
        && transform.skew == 0.0
        && transform.skew_axis == 0.0
        && transform.rotation_x == 0.0
        && transform.rotation_y == 0.0
        && transform.orientation == [0.0; 3]
        && transform.opacity.value() == 100.0
}

fn choose_transform<'a>(
    inherited: Option<(&'a Transform, LayerId)>,
    current: &'a Transform,
    current_id: LayerId,
    animated: bool,
) -> Result<Option<(&'a Transform, LayerId)>, &'static str> {
    if identity(current) && !animated {
        Ok(inherited)
    } else if inherited.is_none() {
        Ok(Some((current, current_id)))
    } else {
        Err(
            "Multiple nonidentity transforms on one chain require native hierarchy export, not matrix flattening",
        )
    }
}

fn effective_constant(animator: &PropertyAnimator) -> Option<&PropertyValue> {
    match animator.data() {
        AnimatorData::Constant { value } => Some(value),
        AnimatorData::Keyframes {
            enabled: false,
            disabled_value,
            ..
        } => disabled_value.as_ref(),
        AnimatorData::Keyframes { enabled: true, .. } | AnimatorData::JsScript { .. } => None,
    }
}

fn effective_static_transform_value_matches(
    animator: &PropertyAnimator,
    property: PropType,
    base: &Transform,
) -> bool {
    let expected = match property {
        PropType::Skew => base.skew,
        PropType::SkewAxis => base.skew_axis,
        _ => return false,
    };
    effective_constant(animator)
        .and_then(|value| float_value(value).ok())
        .is_some_and(|value| value == expected)
}

fn effective_static_shape_path<'a>(
    entries: &'a [AnimationGraphEntry],
    id: LayerId,
    base: &'a ShapePath,
) -> Result<&'a ShapePath, &'static str> {
    let Some(entry) = entries.iter().find(|entry| {
        entry.target.as_property().is_some_and(|target| {
            target.layer_id() == id && target.property_type() == PropType::ShapePath
        })
    }) else {
        return Ok(base);
    };
    let Some(value) = effective_constant(&entry.animator) else {
        return match entry.animator.data() {
            AnimatorData::Keyframes { enabled: true, .. } => Err(
                "Keyframed Shape Path export is excluded; only the current static editable path can be authored natively",
            ),
            AnimatorData::Keyframes {
                enabled: false,
                disabled_value: None,
                ..
            } => Err("Disabled Shape Path animator has no runtime-visible disabledValue"),
            AnimatorData::JsScript { .. } => {
                Err("JavaScript Shape Path animators are not native-exportable and are never baked")
            }
            AnimatorData::Constant { .. }
            | AnimatorData::Keyframes {
                enabled: false,
                disabled_value: Some(_),
                ..
            } => Err("Shape Path animator has no effective static value"),
        };
    };
    match value {
        PropertyValue::Path(path) => Ok(path),
        _ => Err("Shape Path animator on a Shape layer must evaluate to a path value"),
    }
}

fn has_entries_for(entries: &[AnimationGraphEntry], id: LayerId) -> bool {
    entries
        .iter()
        .any(|entry| entry.target.layer_id() == Some(id))
}

fn has_transform_entries(entries: &[AnimationGraphEntry], id: LayerId) -> bool {
    entries.iter().any(|entry| {
        entry.target.layer_id() == Some(id)
            && entry
                .target
                .as_property()
                .is_some_and(|property| is_transform_property(property.property_type()))
    })
}

#[derive(Clone, Copy)]
enum NativeTrack<'a> {
    Keyframes(&'a PropertyKeyframeTrack),
    Constant(&'a PropertyValue),
}

fn track<'a>(
    entries: &'a [AnimationGraphEntry],
    id: LayerId,
    property: PropType,
) -> Result<Option<NativeTrack<'a>>, &'static str> {
    let Some(entry) = entries.iter().find(|entry| {
        entry
            .target
            .as_property()
            .is_some_and(|target| target.layer_id() == id && target.property_type() == property)
    }) else {
        return Ok(None);
    };
    if !entry.dependencies.is_empty()
        || entry.random_seed_target.is_some()
        || !entry.layer_refs.is_empty()
    {
        return Err("Dependent animator cannot be represented as a native numeric keyframe track");
    }
    match entry.animator.data() {
        AnimatorData::Keyframes {
            track,
            enabled: true,
            ..
        } => Ok(Some(NativeTrack::Keyframes(track))),
        AnimatorData::Keyframes {
            enabled: false,
            disabled_value: Some(value),
            ..
        } => Ok(Some(NativeTrack::Constant(value))),
        AnimatorData::Keyframes {
            enabled: false,
            disabled_value: None,
            ..
        } => Err("Disabled animator has no runtime-visible disabledValue"),
        AnimatorData::Constant { value } => Ok(Some(NativeTrack::Constant(value))),
        AnimatorData::JsScript { .. } => {
            Err("JavaScript animators are not native-exportable and are never baked")
        }
    }
}

fn native_easing(value: PropertyKeyframeEasing) -> KeyframeEasing {
    match value {
        PropertyKeyframeEasing::Hold => KeyframeEasing::Hold,
        PropertyKeyframeEasing::Linear => KeyframeEasing::Linear,
        PropertyKeyframeEasing::CubicBezier { x1, y1, x2, y2 } => {
            KeyframeEasing::CubicBezier { x1, y1, x2, y2 }
        }
    }
}

fn float_value(value: &PropertyValue) -> Result<f64, &'static str> {
    match value {
        PropertyValue::Float(value) if value.is_finite() => Ok(*value),
        _ => Err("Numeric keyframe has a non-finite or non-float value"),
    }
}

fn constant_track(values: Vec<f64>, easing_dimensions: usize, spatial: bool) -> NumericTrack {
    NumericTrack {
        keys: vec![NumericKeyframe {
            time_millis: 0,
            values,
            easing: vec![KeyframeEasing::Hold; easing_dimensions],
            spatial_in: if spatial { vec![0.0; 3] } else { Vec::new() },
            spatial_out: if spatial { vec![0.0; 3] } else { Vec::new() },
        }],
    }
}

fn scalar_track(
    source: Option<NativeTrack<'_>>,
    divisor: f64,
) -> Result<Option<NumericTrack>, &'static str> {
    match source {
        None => Ok(None),
        Some(NativeTrack::Constant(value)) => Ok(Some(constant_track(
            vec![float_value(value)? / divisor],
            1,
            false,
        ))),
        Some(NativeTrack::Keyframes(track)) => track
            .keyframes()
            .iter()
            .map(|key| {
                Ok(NumericKeyframe {
                    time_millis: key.layer_time().as_millis(),
                    values: vec![float_value(key.value())? / divisor],
                    easing: vec![native_easing(key.easing())],
                    spatial_in: Vec::new(),
                    spatial_out: Vec::new(),
                })
            })
            .collect::<Result<Vec<_>, &'static str>>()
            .map(|keys| Some(NumericTrack { keys })),
    }
}

fn vector_value(value: &PropertyValue) -> Result<[f64; 2], &'static str> {
    let PropertyValue::Vector2(value) = value else {
        return Err("Vector keyframe has a non-vector value");
    };
    if value.iter().any(|value| !value.is_finite()) {
        return Err("Vector keyframe has a non-finite value");
    }
    Ok(*value)
}

fn vector_track(source: Option<NativeTrack<'_>>) -> Result<Option<NumericTrack>, &'static str> {
    match source {
        None => Ok(None),
        Some(NativeTrack::Constant(value)) => Ok(Some(constant_track(
            vector_value(value)?.to_vec(),
            2,
            false,
        ))),
        Some(NativeTrack::Keyframes(track)) => track
            .keyframes()
            .iter()
            .map(|key| {
                Ok(NumericKeyframe {
                    time_millis: key.layer_time().as_millis(),
                    values: vector_value(key.value())?.to_vec(),
                    easing: vec![native_easing(key.easing()); 2],
                    spatial_in: Vec::new(),
                    spatial_out: Vec::new(),
                })
            })
            .collect::<Result<Vec<_>, &'static str>>()
            .map(|keys| Some(NumericTrack { keys })),
    }
}

fn color_value(value: &PropertyValue) -> Result<[f64; 4], &'static str> {
    let PropertyValue::Color(value) = value else {
        return Err("Color keyframe has a non-color value");
    };
    if value
        .iter()
        .any(|component| !component.is_finite() || !(0.0..=1.0).contains(component))
    {
        return Err("Color keyframe component lies outside 0..=1");
    }
    Ok(*value)
}

fn color_track(source: Option<NativeTrack<'_>>) -> Result<Option<NumericTrack>, &'static str> {
    match source {
        None => Ok(None),
        Some(NativeTrack::Constant(value)) => {
            Ok(Some(constant_track(color_value(value)?.to_vec(), 4, false)))
        }
        Some(NativeTrack::Keyframes(track)) => track
            .keyframes()
            .iter()
            .map(|key| {
                Ok(NumericKeyframe {
                    time_millis: key.layer_time().as_millis(),
                    values: color_value(key.value())?.to_vec(),
                    easing: vec![native_easing(key.easing()); 4],
                    spatial_in: Vec::new(),
                    spatial_out: Vec::new(),
                })
            })
            .collect::<Result<Vec<_>, &'static str>>()
            .map(|keys| Some(NumericTrack { keys })),
    }
}

#[derive(Default)]
struct PaintTracks {
    fill_color: Option<NumericTrack>,
    stroke_color: Option<NumericTrack>,
    stroke: VectorPaintAnimations,
}

fn vector_paint_animations(
    entries: &[AnimationGraphEntry],
    id: LayerId,
) -> Result<PaintTracks, &'static str> {
    let stroke = stroke_animations(entries, id)?;
    Ok(PaintTracks {
        fill_color: color_track(track(entries, id, PropType::FillColor)?)?,
        stroke_color: color_track(track(entries, id, PropType::StrokeColor)?)?,
        stroke: VectorPaintAnimations {
            color: None,
            opacity: None,
            width: stroke.width,
            miter_limit: stroke.miter_limit,
            join: stroke.join,
            dash_offset: scalar_track(track(entries, id, PropType::StrokeDashOffset)?, 1.0)?,
        },
    })
}

#[derive(Default)]
struct ModifierTracks {
    round_corners: Option<NumericTrack>,
    offset_paths: Option<NumericTrack>,
    trim_start: Option<NumericTrack>,
    trim_end: Option<NumericTrack>,
    trim_offset: Option<NumericTrack>,
}

fn modifier_animations(
    entries: &[AnimationGraphEntry],
    id: LayerId,
) -> Result<ModifierTracks, &'static str> {
    let scalar = |property| scalar_track(track(entries, id, property)?, 1.0);
    Ok(ModifierTracks {
        round_corners: scalar(PropType::RoundCornersRadius)?,
        offset_paths: scalar(PropType::OffsetPathsAmount)?,
        trim_start: scalar(PropType::TrimStart)?,
        trim_end: scalar(PropType::TrimEnd)?,
        trim_offset: scalar(PropType::TrimOffset)?,
    })
}

fn paired_track<'a>(
    x: Option<NativeTrack<'a>>,
    y: Option<NativeTrack<'a>>,
    base: [f64; 2],
    divisor: f64,
    spatial: bool,
) -> Result<Option<NumericTrack>, &'static str> {
    if x.is_none() && y.is_none() {
        return Ok(None);
    }
    let x_track = match x {
        Some(NativeTrack::Keyframes(track)) => Some(track),
        _ => None,
    };
    let y_track = match y {
        Some(NativeTrack::Keyframes(track)) => Some(track),
        _ => None,
    };
    if !spatial && (x_track.is_some() || y_track.is_some()) {
        // Scale has independent temporal curves but shared native interpolation
        // flags. Split only at authored knots, and give constant components the
        // same interpolation kind rather than rejecting every eased XY track
        // because the unchanged Z component was marked Linear.
        let component_base = |source: Option<NativeTrack<'_>>, fallback| {
            match source {
                Some(NativeTrack::Constant(value)) => float_value(value),
                _ => Ok(fallback),
            }
            .map(|value| value / divisor)
        };
        return effects::merge_tracks(
            &[
                component_base(x, base[0])?,
                component_base(y, base[1])?,
                if divisor == 100.0 { 1.0 } else { 0.0 },
            ],
            &[
                scalar_track(x_track.map(NativeTrack::Keyframes), divisor)?,
                scalar_track(y_track.map(NativeTrack::Keyframes), divisor)?,
                None,
            ],
        );
    }
    if x_track.zip(y_track).is_some_and(|(x, y)| {
        x.keyframes()
            .iter()
            .map(|key| key.layer_time())
            .ne(y.keyframes().iter().map(|key| key.layer_time()))
    }) {
        return Err(
            "Paired native Transform components have different key times; resampling is not allowed",
        );
    }
    let value_at = |source: Option<NativeTrack<'_>>, index: usize, fallback: f64| match source {
        Some(NativeTrack::Keyframes(track)) => float_value(track.keyframes()[index].value()),
        Some(NativeTrack::Constant(value)) => float_value(value),
        None => Ok(fallback),
    };
    let Some(template) = x_track.or(y_track) else {
        let values = vec![
            value_at(x, 0, base[0])? / divisor,
            value_at(y, 0, base[1])? / divisor,
            if divisor == 100.0 { 1.0 } else { 0.0 },
        ];
        return Ok(Some(constant_track(
            values,
            if spatial { 1 } else { 3 },
            spatial,
        )));
    };
    let mut keys = Vec::with_capacity(template.keyframes().len());
    for (index, template_key) in template.keyframes().iter().enumerate() {
        let x_key = x_track.map(|track| &track.keyframes()[index]);
        let y_key = y_track.map(|track| &track.keyframes()[index]);
        // A constant or missing axis cannot alter temporal ease. Share the
        // authored keyed axis's ease rather than resampling either component.
        let x_easing = x_key
            .or(y_key)
            .map_or(PropertyKeyframeEasing::Hold, |key| key.easing());
        let y_easing = y_key
            .or(x_key)
            .map_or(PropertyKeyframeEasing::Hold, |key| key.easing());
        if spatial && x_easing != y_easing {
            return Err(
                "Spatial X/Y easing differs and cannot be represented by one native path ease",
            );
        }
        let values = [
            value_at(x, index, base[0])? / divisor,
            value_at(y, index, base[1])? / divisor,
            if divisor == 100.0 { 1.0 } else { 0.0 },
        ];
        let (spatial_in, spatial_out) = if spatial {
            (
                vec![
                    x_key
                        .and_then(|key| key.spatial_in_tangent())
                        .unwrap_or(0.0),
                    y_key
                        .and_then(|key| key.spatial_in_tangent())
                        .unwrap_or(0.0),
                    0.0,
                ],
                vec![
                    x_key
                        .and_then(|key| key.spatial_out_tangent())
                        .unwrap_or(0.0),
                    y_key
                        .and_then(|key| key.spatial_out_tangent())
                        .unwrap_or(0.0),
                    0.0,
                ],
            )
        } else {
            (Vec::new(), Vec::new())
        };
        keys.push(NumericKeyframe {
            time_millis: template_key.layer_time().as_millis(),
            values: values.to_vec(),
            easing: if spatial {
                vec![native_easing(x_easing)]
            } else {
                vec![
                    native_easing(x_easing),
                    native_easing(y_easing),
                    KeyframeEasing::Linear,
                ]
            },
            spatial_in,
            spatial_out,
        });
    }
    Ok(Some(NumericTrack { keys }))
}

fn stroke_join_value(value: &PropertyValue) -> Result<f64, &'static str> {
    match value {
        PropertyValue::String(value) => match value.as_str() {
            "miter" => Ok(1.0),
            "round" => Ok(2.0),
            "bevel" => Ok(3.0),
            _ => Err("Stroke Join key has an unsupported enum value"),
        },
        _ => Err("Stroke Join key must be an editable string enum"),
    }
}

fn stroke_join_track(
    source: Option<NativeTrack<'_>>,
) -> Result<Option<NumericTrack>, &'static str> {
    match source {
        None => Ok(None),
        Some(NativeTrack::Constant(value)) => Ok(Some(constant_track(
            vec![stroke_join_value(value)?],
            1,
            false,
        ))),
        Some(NativeTrack::Keyframes(track)) => track
            .keyframes()
            .iter()
            .enumerate()
            .map(|(index, key)| {
                if index > 0 && key.easing() != PropertyKeyframeEasing::Hold {
                    return Err("Stroke Join transitions require incoming Hold easing");
                }
                if key.spatial_in_tangent().is_some() || key.spatial_out_tangent().is_some() {
                    return Err("Stroke Join keys cannot carry spatial tangents");
                }
                Ok(NumericKeyframe {
                    time_millis: key.layer_time().as_millis(),
                    values: vec![stroke_join_value(key.value())?],
                    // The first key has no incoming segment; its easing is
                    // unused in FX and is normalized to the discrete mode.
                    easing: vec![KeyframeEasing::Hold],
                    spatial_in: Vec::new(),
                    spatial_out: Vec::new(),
                })
            })
            .collect::<Result<Vec<_>, &'static str>>()
            .map(|keys| Some(NumericTrack { keys })),
    }
}

fn has_stroke_animator(entries: &[AnimationGraphEntry], id: LayerId) -> bool {
    entries.iter().any(|entry| {
        entry.target.as_property().is_some_and(|target| {
            target.layer_id() == id
                && matches!(
                    target.property_type(),
                    PropType::StrokeColor
                        | PropType::StrokeWidth
                        | PropType::StrokeDashOffset
                        | PropType::StrokeMiterLimit
                        | PropType::StrokeJoin
                        | PropType::StrokeEnabled
                )
        })
    })
}

fn stroke_animations(
    entries: &[AnimationGraphEntry],
    id: LayerId,
) -> Result<StrokeAnimations, &'static str> {
    Ok(StrokeAnimations {
        width: scalar_track(track(entries, id, PropType::StrokeWidth)?, 1.0)?,
        miter_limit: scalar_track(track(entries, id, PropType::StrokeMiterLimit)?, 1.0)?,
        join: stroke_join_track(track(entries, id, PropType::StrokeJoin)?)?,
    })
}

fn transform_animations(
    entries: &[AnimationGraphEntry],
    id: LayerId,
    base: &Transform,
    content_id: LayerId,
) -> Result<TransformAnimations, &'static str> {
    transform_animations_partitioned(entries, id, base, content_id, false, false)
}

fn transform_animations_partitioned(
    entries: &[AnimationGraphEntry],
    id: LayerId,
    base: &Transform,
    content_id: LayerId,
    native_3d: bool,
    vector_program: bool,
) -> Result<TransformAnimations, &'static str> {
    let allowed = [
        PropType::AnchorPointX,
        PropType::AnchorPointY,
        PropType::PositionX,
        PropType::PositionY,
        PropType::ScaleX,
        PropType::ScaleY,
        PropType::Rotation,
        PropType::Opacity,
        PropType::RectSize,
        PropType::RectRoundness,
        PropType::ShapePath,
        PropType::EllipseSize,
        PropType::EllipsePosition,
        PropType::PolyStarPosition,
        PropType::PolyStarPoints,
        PropType::PolyStarRotation,
        PropType::PolyStarOuterRadius,
        PropType::PolyStarInnerRadius,
        PropType::PolyStarOuterRoundness,
        PropType::PolyStarInnerRoundness,
        PropType::FillColor,
        PropType::StrokeColor,
        PropType::StrokeWidth,
        PropType::StrokeDashOffset,
        PropType::StrokeMiterLimit,
        PropType::StrokeJoin,
        PropType::RoundCornersRadius,
        PropType::OffsetPathsAmount,
        PropType::TrimStart,
        PropType::TrimEnd,
        PropType::TrimOffset,
        PropType::ShapePath,
    ];
    if entries
        .iter()
        .filter(|entry| entry.target.layer_id() == Some(id))
        .any(|entry| {
            entry.target.as_property().is_none_or(|property| {
                !(allowed.contains(&property.property_type())
                    || native_3d && is_native_3d_partition_property(property.property_type())
                    || vector_program
                        && !native_3d
                        && matches!(
                            property.property_type(),
                            PropType::Skew | PropType::SkewAxis
                        )
                    || effective_static_transform_value_matches(
                        &entry.animator,
                        property.property_type(),
                        base,
                    )
                    || id == content_id
                        && matches!(
                            property.property_type(),
                            PropType::StrokeWidth
                                | PropType::StrokeMiterLimit
                                | PropType::StrokeJoin
                        ))
            })
        })
    {
        return Err("Layer has animator targets outside native 2D Transform support");
    }
    if native_3d {
        return Ok(TransformAnimations::default());
    }
    let position = match base.position {
        Position::TwoD(value) => value,
        Position::ThreeD(_) => return Err("3D Transform animation is not implemented"),
    };
    Ok(TransformAnimations {
        anchor: paired_track(
            track(entries, id, PropType::AnchorPointX)?,
            track(entries, id, PropType::AnchorPointY)?,
            base.anchor_point,
            1.0,
            true,
        )?,
        position: paired_track(
            track(entries, id, PropType::PositionX)?,
            track(entries, id, PropType::PositionY)?,
            position,
            1.0,
            true,
        )?,
        scale: paired_track(
            track(entries, id, PropType::ScaleX)?,
            track(entries, id, PropType::ScaleY)?,
            base.scale,
            100.0,
            false,
        )?,
        rotation: scalar_track(track(entries, id, PropType::Rotation)?, 1.0)?,
        opacity: scalar_track(track(entries, id, PropType::Opacity)?, 100.0)?,
    })
}

fn solid_transform_animations(
    entries: &[AnimationGraphEntry],
    id: LayerId,
    base: &SolidTransform,
    allow_audio: bool,
) -> Result<TransformAnimations, &'static str> {
    solid_transform_animations_partitioned(entries, id, base, allow_audio, false)
}

fn solid_transform_animations_partitioned(
    entries: &[AnimationGraphEntry],
    id: LayerId,
    base: &SolidTransform,
    allow_audio: bool,
    native_3d: bool,
) -> Result<TransformAnimations, &'static str> {
    let allowed = [
        PropType::AnchorPointX,
        PropType::AnchorPointY,
        PropType::PositionX,
        PropType::PositionY,
        PropType::ScaleX,
        PropType::ScaleY,
        PropType::Rotation,
        PropType::Opacity,
    ];
    if entries
        .iter()
        .filter(|entry| entry.target.layer_id() == Some(id))
        .any(|entry| {
            entry.target.as_property().is_none_or(|property| {
                !(allowed.contains(&property.property_type())
                    || native_3d && is_transform_property(property.property_type())
                    || allow_audio && property.property_type() == PropType::AudioVolume)
            })
        })
    {
        return Err("Media has an animator target without a native editable mapping");
    }
    if native_3d {
        return Ok(TransformAnimations::default());
    }
    Ok(TransformAnimations {
        anchor: paired_track(
            track(entries, id, PropType::AnchorPointX)?,
            track(entries, id, PropType::AnchorPointY)?,
            base.anchor,
            1.0,
            true,
        )?,
        position: paired_track(
            track(entries, id, PropType::PositionX)?,
            track(entries, id, PropType::PositionY)?,
            base.position,
            1.0,
            true,
        )?,
        scale: paired_track(
            track(entries, id, PropType::ScaleX)?,
            track(entries, id, PropType::ScaleY)?,
            base.scale,
            100.0,
            false,
        )?,
        rotation: scalar_track(track(entries, id, PropType::Rotation)?, 1.0)?,
        opacity: scalar_track(track(entries, id, PropType::Opacity)?, 100.0)?,
    })
}

fn rect_animations(
    entries: &[AnimationGraphEntry],
    rect_id: LayerId,
    transform_id: LayerId,
    origin: [f64; 2],
    transform: TransformAnimations,
    native_3d: bool,
) -> Result<RectAnimations, &'static str> {
    let content = [
        PropType::RectSize,
        PropType::RectRoundness,
        PropType::StrokeWidth,
        PropType::StrokeMiterLimit,
        PropType::StrokeJoin,
    ];
    let transform_properties = [
        PropType::AnchorPointX,
        PropType::AnchorPointY,
        PropType::PositionX,
        PropType::PositionY,
        PropType::ScaleX,
        PropType::ScaleY,
        PropType::Rotation,
        PropType::Opacity,
    ];
    if entries
        .iter()
        .filter(|entry| entry.target.layer_id() == Some(rect_id))
        .any(|entry| {
            entry.target.as_property().is_none_or(|property| {
                let kind = property.property_type();
                !(content.contains(&kind)
                    || rect_id == transform_id
                        && (transform_properties.contains(&kind)
                            || native_3d && is_native_3d_partition_property(kind)))
            })
        })
    {
        return Err("Rectangle has an animator target without a native editable mapping");
    }
    let size = vector_track(track(entries, rect_id, PropType::RectSize)?)?;
    if size.as_ref().is_some_and(|track| {
        track.keys.iter().any(|key| {
            key.values.iter().any(|value| {
                !value.is_finite() || !(0.0..=65535.0).contains(value) || *value == 0.0
            }) || origin
                .iter()
                .zip(&key.values)
                .any(|(start, extent)| !(start + extent / 2.0).is_finite())
        })
    }) {
        return Err("Animated Rectangle size or derived native center exceeds supported bounds");
    }
    // FX Rect position is its origin; AE Rectangle Position is its center.
    // Keeping Size keys without matching Position keys would expand about the
    // static center and visibly move an origin-anchored rectangle.
    let position = size.as_ref().map(|track| NumericTrack {
        keys: track
            .keys
            .iter()
            .map(|key| {
                let mut key = key.clone();
                key.values = origin
                    .iter()
                    .zip(&key.values)
                    .map(|(start, extent)| start + extent / 2.0)
                    .collect();
                key
            })
            .collect(),
    });
    let roundness = scalar_track(track(entries, rect_id, PropType::RectRoundness)?, 1.0)?;
    if roundness.as_ref().is_some_and(|track| {
        track.keys.iter().any(|key| {
            key.values
                .iter()
                .any(|value| !value.is_finite() || !(0.0..=100000.0).contains(value))
        })
    }) {
        return Err("Animated Rectangle roundness exceeds native bounds");
    }
    Ok(RectAnimations {
        transform,
        size,
        position,
        roundness,
        stroke: stroke_animations(entries, rect_id)?,
    })
}

fn dimension(value: f64) -> Result<u16, &'static str> {
    if !value.is_finite() || value.fract() != 0.0 || !(1.0..=f64::from(u16::MAX)).contains(&value) {
        return Err(
            "Solid dimensions must be positive integer u16 pixels; fractional/negative geometry is not approximated",
        );
    }
    // The integral, finite u16 range was checked above.
    Ok(value as u16)
}

fn shape_animations(
    entries: &[AnimationGraphEntry],
    shape_id: LayerId,
    transform_id: LayerId,
) -> Result<GeometryAnimations, &'static str> {
    let geometry = [
        PropType::ShapePath,
        PropType::StrokeWidth,
        PropType::StrokeMiterLimit,
        PropType::StrokeJoin,
        PropType::EllipseSize,
        PropType::EllipsePosition,
        PropType::PolyStarPosition,
        PropType::PolyStarPoints,
        PropType::PolyStarRotation,
        PropType::PolyStarOuterRadius,
        PropType::PolyStarInnerRadius,
        PropType::PolyStarOuterRoundness,
        PropType::PolyStarInnerRoundness,
        PropType::FillColor,
        PropType::StrokeColor,
        PropType::StrokeDashOffset,
        PropType::RoundCornersRadius,
        PropType::OffsetPathsAmount,
        PropType::TrimStart,
        PropType::TrimEnd,
        PropType::TrimOffset,
        PropType::ShapePath,
    ];
    if shape_id != transform_id
        && entries
            .iter()
            .filter(|entry| entry.target.layer_id() == Some(shape_id))
            .any(|entry| {
                entry
                    .target
                    .as_property()
                    .is_none_or(|property| !geometry.contains(&property.property_type()))
            })
    {
        return Err(
            "Shape has animator targets without native editable geometry or Transform records",
        );
    }
    let property = |kind| track(entries, shape_id, kind);
    let points = property(PropType::PolyStarPoints)?;
    let invalid_points = match points {
        None => false,
        Some(NativeTrack::Constant(value)) => {
            !float_value(value).is_ok_and(|value| value.fract() == 0.0)
        }
        Some(NativeTrack::Keyframes(track)) => {
            let keys = track.keyframes();
            keys.iter()
                .any(|key| !float_value(key.value()).is_ok_and(|value| value.fract() == 0.0))
                || keys.windows(2).any(|pair| {
                    pair[0].value() != pair[1].value()
                        && pair[1].easing() != PropertyKeyframeEasing::Hold
                })
        }
    };
    if invalid_points {
        return Err(
            "Fractional Star points between native keys are excluded; changing point counts require hold keyframes",
        );
    }
    Ok(GeometryAnimations {
        path: path_animation::track(property(PropType::ShapePath)?)?,
        rect_size: None,
        rect_position: None,
        rect_roundness: None,
        ellipse_size: vector_track(property(PropType::EllipseSize)?)?,
        ellipse_position: vector_track(property(PropType::EllipsePosition)?)?,
        star_position: vector_track(property(PropType::PolyStarPosition)?)?,
        star_points: scalar_track(points, 1.0)?,
        star_rotation: scalar_track(property(PropType::PolyStarRotation)?, 1.0)?,
        star_outer_radius: scalar_track(property(PropType::PolyStarOuterRadius)?, 1.0)?,
        star_inner_radius: scalar_track(property(PropType::PolyStarInnerRadius)?, 1.0)?,
        star_outer_roundness: scalar_track(property(PropType::PolyStarOuterRoundness)?, 1.0)?,
        star_inner_roundness: scalar_track(property(PropType::PolyStarInnerRoundness)?, 1.0)?,
    })
}

#[expect(
    clippy::too_many_arguments,
    reason = "callers supply independently lowered transform, geometry, paint and sidecar inputs"
)]
fn vector_shape_program(
    layer: &ShapeLayer,
    transform: &Transform,
    animations: (TransformAnimations, GeometryAnimations),
    transform_id: LayerId,
    tracks: (PaintTracks, ModifierTracks),
    dynamics: &[AnimationGraphEntry],
    paints: &paint_controls::PaintMaterialization,
    sidecar: Option<&mut (NativeTransform3d, Transform3dAnimations)>,
    allow_layer_sidecars: bool,
    force_vector_transform: bool,
    paint_approximations: &mut Vec<(LayerId, &'static str)>,
) -> Result<VectorLayerSpec, &'static str> {
    let (mut animations, geometry_animations) = animations;
    let (paint_tracks, modifier_tracks) = tracks;
    if !allow_layer_sidecars
        && (layer.is_hidden
            || layer.motion_blur
            || layer.blend_mode != Default::default()
            || layer.track_matte.is_some()
            || !layer.masks.is_empty()
            || !layer.effects.is_empty())
    {
        return Err(
            "Shape visibility, motion blur, layer blend, matte, masks or effects require native records not established by the vector sidecars",
        );
    }
    let geometry = shape_geometry(layer, &geometry_animations, dynamics)?;
    let static_path = match &geometry {
        VectorGeometry::Path(path) if geometry_animations.path.is_none() => {
            Some(crate::writer::PathTrack {
                keyframes: vec![crate::writer::PathKeyframe {
                    time_millis: 0,
                    path: path.clone(),
                    easing: KeyframeEasing::Hold,
                }],
            })
        }
        _ => None,
    };
    let path_visibility = geometry_animations
        .path
        .as_ref()
        .or(static_path.as_ref())
        .map(|track| path_animation::visibility(track, !paints.strokes().is_empty()))
        .transpose()?
        .flatten();
    let mut contents = vec![VectorContent::Geometry {
        geometry,
        animations: geometry_animations,
    }];
    if let Some(value) = &layer.shape.round_corners {
        contents.push(VectorContent::Modifier(VectorModifierSpec::RoundCorners {
            value: value.clone(),
            radius: modifier_tracks.round_corners,
        }));
    } else if modifier_tracks.round_corners.is_some() {
        return Err("Round Corners keys target a Shape without the modifier");
    }
    if let Some(value) = layer.shape.offset_paths {
        contents.push(VectorContent::Modifier(VectorModifierSpec::OffsetPaths {
            value,
            amount: modifier_tracks.offset_paths,
        }));
    } else if modifier_tracks.offset_paths.is_some() {
        return Err("Offset Paths keys target a Shape without the modifier");
    }
    if let Some(value) = layer.shape.trim {
        contents.push(VectorContent::Modifier(VectorModifierSpec::TrimPaths {
            value,
            start: modifier_tracks.trim_start,
            end: modifier_tracks.trim_end,
            offset: modifier_tracks.trim_offset,
        }));
    } else if modifier_tracks.trim_start.is_some()
        || modifier_tracks.trim_end.is_some()
        || modifier_tracks.trim_offset.is_some()
    {
        return Err("Trim keys target a Shape without the modifier");
    }
    for fill in paints.fills() {
        if matches!(fill.paint, ShapePaint::Gradient { .. }) && paint_tracks.fill_color.is_some() {
            return Err("FillColor keys cannot retarget editable gradient stops");
        }
        contents.push(VectorContent::Paint(VectorPaintSpec::Fill {
            paint: fill.paint.clone(),
            fill_rule: fill.fill_rule,
            blend_mode: fill.blend_mode,
            opacity: fill.opacity * 100.0,
            animations: VectorPaintAnimations {
                color: paint_tracks.fill_color.clone(),
                ..Default::default()
            },
        }));
    }
    for stroke in paints.strokes() {
        if matches!(stroke.paint, ShapePaint::Gradient { .. })
            && paint_tracks.stroke_color.is_some()
        {
            return Err("StrokeColor keys cannot retarget editable gradient stops");
        }
        let mut paint_animations = paint_tracks.stroke.clone();
        paint_animations.color = paint_tracks.stroke_color.clone();
        contents.push(VectorContent::Paint(VectorPaintSpec::Stroke {
            paint: stroke.paint.clone(),
            blend_mode: stroke.blend_mode,
            opacity: stroke.opacity * 100.0,
            width: stroke.width.value(),
            cap: stroke.cap,
            join: stroke.join,
            miter_limit: stroke.miter_limit,
            dashes: stroke_dashes(std::slice::from_ref(stroke))?,
            animations: paint_animations,
        }));
    }
    if contents.len() == 1 && !layer.is_hidden {
        return Err("Shape has geometry but no enabled paint; native visibility is not invented");
    }
    paints.apply_owned_tracks(&mut contents)?;
    let mut normalized_transform = *transform;
    // Skew/forced vector transforms reconstruct keys from the original FX
    // dynamics; keep their animated opacity on the existing fallback path.
    let preserves_opacity_keys = animations.opacity.is_none()
        || (!force_vector_transform
            && transform.skew == 0.0
            && transform.skew_axis == 0.0
            && !vector_animation::has_skew_tracks(dynamics, transform_id));
    if layer.effects.is_empty()
        && layer.masks.is_empty()
        && layer.blend_mode == Default::default()
        && matches!(layer.transform.position, Position::TwoD(_))
        && layer.transform.rotation_x == 0.0
        && layer.transform.rotation_y == 0.0
        && layer.transform.orientation == [0.0; 3]
    {
        if let Some((native, native_keys)) = sidecar {
            // Independent XY Position followers use a native 2D Transform
            // sidecar. Its opacity is the only live control: the inner vector
            // Transform is identity, so normalizing there loses the product.
            // Keep all other native controls, keys and clocks untouched.
            if !native.is_three_d
                && !force_vector_transform
                && transform.skew == 0.0
                && transform.skew_axis == 0.0
                && !vector_animation::has_skew_tracks(dynamics, transform_id)
                && let Some(opacity) = paint_opacity::normalize_single_paint(
                    &mut contents,
                    native.opacity * 100.0,
                    native_keys.opacity.as_mut(),
                )
            {
                native.opacity = opacity / 100.0;
                paint_approximations.push((layer.id, paint_opacity::NORMALIZATION_DIAGNOSTIC));
            }
        } else if preserves_opacity_keys
            && let Some(opacity) = paint_opacity::normalize_single_paint(
                &mut contents,
                transform.opacity.value(),
                animations.opacity.as_mut(),
            )
        {
            normalized_transform.opacity = fx_schema::PercentageProperty::new(opacity)
                .expect("single-paint normalization checks the percentage range");
            paint_approximations.push((layer.id, paint_opacity::NORMALIZATION_DIAGNOSTIC));
        }
    }
    if paint_opacity::needs_folding(paints) && paint_opacity::fold(&mut contents)? {
        paint_approximations.push((layer.id, paint_opacity::SATURATION_DIAGNOSTIC));
    }
    if let Some(opacity) = path_visibility {
        contents = vec![VectorContent::AnimatedGroup(
            VectorGroupSpec {
                name: "Path visibility".into(),
                transform: VectorGroupTransform::default(),
                blend_mode: Default::default(),
                contents,
            },
            crate::writer::VectorGroupAnimations {
                opacity: Some(opacity),
                ..Default::default()
            },
        )];
    }
    let (layer_transform, contents) = program_transform_with_mode(
        &normalized_transform,
        animations,
        dynamics,
        transform_id,
        contents,
        force_vector_transform,
    )?;
    Ok(VectorLayerSpec {
        name: layer.name.clone(),
        transform: layer_transform.0,
        transform_animations: layer_transform.1,
        contents,
    })
}

fn shape_geometry(
    layer: &ShapeLayer,
    animations: &GeometryAnimations,
    dynamics: &[AnimationGraphEntry],
) -> Result<VectorGeometry, &'static str> {
    let path = if animations.path.is_some() {
        &layer.shape.path
    } else {
        effective_static_shape_path(dynamics, layer.id, &layer.shape.path)?
    };
    if animations.path.is_some()
        && (layer.shape.ellipse.is_some() || layer.shape.poly_star.is_some())
    {
        return Err("Path keys cannot be written on parametric geometry");
    }
    match (&layer.shape.ellipse, &layer.shape.poly_star) {
        (Some(ellipse), None) if path.commands.is_empty() => {
            if animations.star_position.is_some()
                || animations.star_points.is_some()
                || animations.star_rotation.is_some()
                || animations.star_outer_radius.is_some()
                || animations.star_inner_radius.is_some()
                || animations.star_outer_roundness.is_some()
                || animations.star_inner_roundness.is_some()
            {
                return Err("Star animator targets cannot be written on an Ellipse");
            }
            Ok(VectorGeometry::Ellipse(ellipse.clone()))
        }
        (None, Some(star)) if path.commands.is_empty() => {
            if animations.ellipse_size.is_some() || animations.ellipse_position.is_some() {
                return Err("Ellipse animator targets cannot be written on a Star");
            }
            Ok(VectorGeometry::PolyStar(star.clone()))
        }
        (None, None) if !path.commands.is_empty() || animations.path.is_some() => {
            if animations.has_parametric_tracks() {
                return Err("Parametric geometry animators cannot be written on a Path");
            }
            // The checked path encoder accepts exactly the canonical single-contour
            // command stream. It rejects unsupported seam/corner controls rather
            // than selecting an unchecked contour phase.
            Ok(VectorGeometry::Path(
                animations
                    .path
                    .as_ref()
                    .and_then(|track| track.keyframes.first())
                    .map_or_else(|| path.clone(), |key| key.path.clone()),
            ))
        }
        _ => Err("Shape requires exactly one native geometry kind"),
    }
}

#[cfg(test)]
fn program_transform(
    transform: &Transform,
    animations: TransformAnimations,
    dynamics: &[AnimationGraphEntry],
    transform_id: LayerId,
    contents: Vec<VectorContent>,
) -> Result<((SolidTransform, TransformAnimations), Vec<VectorContent>), &'static str> {
    program_transform_with_mode(
        transform,
        animations,
        dynamics,
        transform_id,
        contents,
        false,
    )
}

fn program_transform_with_mode(
    transform: &Transform,
    animations: TransformAnimations,
    dynamics: &[AnimationGraphEntry],
    transform_id: LayerId,
    contents: Vec<VectorContent>,
    force_vector_transform: bool,
) -> Result<((SolidTransform, TransformAnimations), Vec<VectorContent>), &'static str> {
    let Position::TwoD(position) = transform.position else {
        return Err(
            "3D Position requires native 3D layer records not established by vector sidecars",
        );
    };
    if transform.rotation_x != 0.0
        || transform.rotation_y != 0.0
        || transform.orientation != [0.0; 3]
    {
        return Err("3D Rotation/Orientation requires native 3D layer records");
    }
    let finite = transform
        .anchor_point
        .iter()
        .chain(&position)
        .chain(&transform.scale)
        .chain([transform.rotation, transform.skew, transform.skew_axis].iter())
        .all(|value| value.is_finite());
    if !finite {
        return Err("Shape Transform contains a non-finite value");
    }
    let layer = SolidTransform {
        anchor: transform.anchor_point,
        position,
        scale: transform.scale,
        rotation: transform.rotation,
        opacity: transform.opacity.value(),
    };
    let has_skew_keys = vector_animation::has_skew_tracks(dynamics, transform_id);
    let has_skew = transform.skew != 0.0 || transform.skew_axis != 0.0 || has_skew_keys;
    if !has_skew && (!force_vector_transform || animations == TransformAnimations::default()) {
        return Ok(((layer, animations), contents));
    }
    // Vector Position's shared-temporal interpolation cannot represent the
    // layer Position's spatial tangents. Keep those keys on their old path
    // until an exact native vector mapping is established.
    if has_skew
        && (animations.anchor.is_some()
            || animations.position.is_some()
            || animations.rotation.is_some())
    {
        return Err(
            "Animated Anchor/Position/Rotation combined with Skew needs exact native vector interpolation",
        );
    }
    let group_keys = if animations == TransformAnimations::default() && !has_skew_keys {
        None
    } else {
        let keys = program_transform_animations(dynamics, transform_id, transform)?;
        if keys == Default::default() {
            return Err("Animated skewed Transform has no matching source-owned vector keys");
        }
        Some(keys)
    };
    let group = VectorGroupSpec {
        name: "FX Layer Transform".into(),
        blend_mode: Default::default(),
        transform: VectorGroupTransform {
            anchor: transform.anchor_point,
            position,
            scale: transform.scale,
            skew: transform.skew,
            skew_axis: transform.skew_axis,
            rotation: transform.rotation,
            opacity: transform.opacity.value(),
        },
        contents,
    };
    Ok((
        (identity_solid_transform(), TransformAnimations::default()),
        vec![match group_keys {
            Some(keys) => VectorContent::AnimatedGroup(group, keys),
            None => VectorContent::Group(group),
        }],
    ))
}

fn identity_solid_transform() -> SolidTransform {
    SolidTransform {
        anchor: [0.0; 2],
        position: [0.0; 2],
        scale: [100.0; 2],
        rotation: 0.0,
        opacity: 100.0,
    }
}

fn vector_boolean_program(
    layer: &BooleanOperationLayer,
    transform: &Transform,
    animations: TransformAnimations,
    transform_id: LayerId,
    dynamics: &[AnimationGraphEntry],
    paints: &paint_controls::PaintMaterialization,
    allow_layer_sidecars: bool,
) -> Result<VectorLayerSpec, &'static str> {
    if !allow_layer_sidecars
        && (layer.is_hidden
            || layer.motion_blur
            || layer.blend_mode != Default::default()
            || layer.track_matte.is_some()
            || !layer.masks.is_empty()
            || !layer.effects.is_empty())
    {
        return Err(
            "Boolean visibility, motion blur, layer blend, matte, masks or effects require unestablished native records",
        );
    }
    if layer.layers.len() < 2 {
        return Err("Editable Merge Paths requires at least two geometry operands");
    }
    let mut operands = layer
        .layers
        .iter()
        .map(|child| boolean_operand_program(child, layer.id, layer.active_range, dynamics, 0))
        .collect::<Result<Vec<_>, _>>()?;
    if layer.op == fx_schema::layer::BooleanOp::Subtract {
        operands.reverse();
    }
    let mut contents = operands;
    contents.push(VectorContent::Merge(layer.op));
    let modifiers = modifier_animations(dynamics, layer.id)?;
    if modifiers.round_corners.is_some() || modifiers.offset_paths.is_some() {
        return Err("Boolean Round Corners/Offset animator targets have no Boolean FX field");
    }
    if let Some(value) = layer.trim {
        contents.push(VectorContent::Modifier(VectorModifierSpec::TrimPaths {
            value,
            start: modifiers.trim_start,
            end: modifiers.trim_end,
            offset: modifiers.trim_offset,
        }));
    } else if modifiers.trim_start.is_some()
        || modifiers.trim_end.is_some()
        || modifiers.trim_offset.is_some()
    {
        return Err("Trim keys target a Boolean without the modifier");
    }
    let paint_tracks = vector_paint_animations(dynamics, layer.id)?;
    for fill in paints.fills() {
        if matches!(fill.paint, ShapePaint::Gradient { .. }) && paint_tracks.fill_color.is_some() {
            return Err("Boolean FillColor keys cannot retarget editable gradient stops");
        }
        contents.push(VectorContent::Paint(VectorPaintSpec::Fill {
            paint: fill.paint.clone(),
            fill_rule: fill.fill_rule,
            blend_mode: fill.blend_mode,
            opacity: fill.opacity * 100.0,
            animations: VectorPaintAnimations {
                color: paint_tracks.fill_color.clone(),
                ..Default::default()
            },
        }));
    }
    for stroke in paints.strokes() {
        if matches!(stroke.paint, ShapePaint::Gradient { .. })
            && paint_tracks.stroke_color.is_some()
        {
            return Err("Boolean StrokeColor keys cannot retarget editable gradient stops");
        }
        let mut paint_animations = paint_tracks.stroke.clone();
        paint_animations.color = paint_tracks.stroke_color.clone();
        contents.push(VectorContent::Paint(VectorPaintSpec::Stroke {
            paint: stroke.paint.clone(),
            blend_mode: stroke.blend_mode,
            opacity: stroke.opacity * 100.0,
            width: stroke.width.value(),
            cap: stroke.cap,
            join: stroke.join,
            miter_limit: stroke.miter_limit,
            dashes: stroke_dashes(std::slice::from_ref(stroke))?,
            animations: paint_animations,
        }));
    }
    if paints.fills().is_empty() && paints.strokes().is_empty() {
        return Err("Boolean has no owned paint; native visibility is not invented");
    }
    paints.apply_owned_tracks(&mut contents)?;
    let (layer_transform, contents) = program_transform_with_mode(
        transform,
        animations,
        dynamics,
        transform_id,
        contents,
        false,
    )?;
    Ok(VectorLayerSpec {
        name: layer.name.clone(),
        transform: layer_transform.0,
        transform_animations: layer_transform.1,
        contents,
    })
}

fn boolean_operand_program(
    child: &Layer,
    parent: LayerId,
    range: fx_schema::TimeRangeProperty,
    dynamics: &[AnimationGraphEntry],
    depth: usize,
) -> Result<VectorContent, &'static str> {
    if depth >= 48 {
        return Err("Nested Boolean geometry exceeds native depth limit");
    }
    if child.parent_id() != Some(parent) || child.active_range() != range {
        return Err("Boolean operand parent or clock needs native child authoring records");
    }
    // Geometry tracks belong to the operand's path operator, not its vector
    // Transform. Partition only supported parametric targets; everything else
    // must still pass the Transform allowlist rather than disappearing silently.
    let (geometry_dynamics, transform_dynamics): (Vec<_>, Vec<_>) = dynamics
        .iter()
        .filter(|entry| entry.target.layer_id() == Some(child.id()))
        .cloned()
        .partition(|entry| {
            entry
                .target
                .as_property()
                .is_some_and(|target| match child.data() {
                    LayerData::Rect(_) => matches!(
                        target.property_type(),
                        PropType::RectSize | PropType::RectRoundness
                    ),
                    LayerData::Shape(_) => matches!(
                        target.property_type(),
                        PropType::EllipseSize
                            | PropType::EllipsePosition
                            | PropType::PolyStarPosition
                            | PropType::PolyStarPoints
                            | PropType::PolyStarRotation
                            | PropType::PolyStarOuterRadius
                            | PropType::PolyStarInnerRadius
                            | PropType::PolyStarOuterRoundness
                            | PropType::PolyStarInnerRoundness
                            | PropType::ShapePath
                    ),
                    _ => false,
                })
        });
    let (name, transform, contents) = match child.data() {
        LayerData::Shape(shape) => {
            check_boolean_operand_flags(
                shape.is_hidden,
                shape.motion_blur,
                shape.blend_mode,
                shape.track_matte.is_some(),
                !shape.masks.is_empty(),
                !shape.effects.is_empty(),
            )?;
            if shape.shape.round_corners.is_some()
                || shape.shape.offset_paths.is_some()
                || shape.shape.trim.is_some()
            {
                return Err("Boolean Shape operand modifiers need scoped native child operators");
            }
            let animations = shape_animations(&geometry_dynamics, shape.id, shape.id)?;
            (
                shape.name.clone(),
                vector_group_transform(&shape.transform)?,
                vec![VectorContent::Geometry {
                    geometry: shape_geometry(shape, &animations, &geometry_dynamics)?,
                    animations,
                }],
            )
        }
        LayerData::Rect(rect) => {
            check_boolean_operand_flags(
                rect.is_hidden,
                rect.motion_blur,
                rect.blend_mode,
                rect.track_matte.is_some(),
                !rect.masks.is_empty(),
                !rect.effects.is_empty(),
            )?;
            let geometry = rect_geometry::lower(rect, &geometry_dynamics)?;
            (
                rect.name.clone(),
                vector_group_transform(&rect.transform)?,
                vec![VectorContent::Geometry {
                    geometry: geometry.geometry,
                    animations: geometry.animations,
                }],
            )
        }
        LayerData::BooleanOperation(boolean) => {
            check_boolean_operand_flags(
                boolean.is_hidden,
                boolean.motion_blur,
                boolean.blend_mode,
                boolean.track_matte.is_some(),
                !boolean.masks.is_empty(),
                !boolean.effects.is_empty(),
            )?;
            if boolean.trim.is_some() || boolean.layers.len() < 2 {
                return Err("Nested Boolean Trim or operand count needs native scoped records");
            }
            let mut groups = boolean
                .layers
                .iter()
                .map(|operand| {
                    boolean_operand_program(
                        operand,
                        boolean.id,
                        boolean.active_range,
                        dynamics,
                        depth + 1,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;
            if boolean.op == fx_schema::layer::BooleanOp::Subtract {
                groups.reverse();
            }
            let mut contents = groups;
            contents.push(VectorContent::Merge(boolean.op));
            (
                boolean.name.clone(),
                vector_group_transform(&boolean.transform)?,
                contents,
            )
        }
        _ => return Err("Boolean operand kind has no native editable geometry mapping"),
    };
    Ok(VectorContent::AnimatedGroup(
        VectorGroupSpec {
            name,
            blend_mode: Default::default(),
            transform,
            contents,
        },
        group_animations(
            &transform_dynamics,
            child.id(),
            match child.data() {
                LayerData::Shape(shape) => &shape.transform,
                LayerData::Rect(rect) => &rect.transform,
                LayerData::BooleanOperation(boolean) => &boolean.transform,
                _ => return Err("Boolean operand kind has no native editable geometry mapping"),
            },
        )?,
    ))
}

fn check_boolean_operand_flags(
    hidden: bool,
    motion_blur: bool,
    blend_mode: fx_schema::BlendMode,
    matte: bool,
    masks: bool,
    effects: bool,
) -> Result<(), &'static str> {
    if hidden || motion_blur || blend_mode != Default::default() || matte || masks || effects {
        return Err(
            "Boolean operand visibility, motion blur, blend, matte, masks or effects cannot be collapsed into geometry",
        );
    }
    Ok(())
}

fn vector_group_transform(transform: &Transform) -> Result<VectorGroupTransform, &'static str> {
    let Position::TwoD(position) = transform.position else {
        return Err("Boolean operand 3D Position needs native 3D child records");
    };
    if transform.rotation_x != 0.0
        || transform.rotation_y != 0.0
        || transform.orientation != [0.0; 3]
    {
        return Err("Boolean operand 3D Rotation/Orientation needs native 3D child records");
    }
    Ok(VectorGroupTransform {
        anchor: transform.anchor_point,
        position,
        scale: transform.scale,
        skew: transform.skew,
        skew_axis: transform.skew_axis,
        rotation: transform.rotation,
        opacity: transform.opacity.value(),
    })
}

fn rect_stroke_dashes(rect: &fx_schema::RectShape) -> Result<StrokeDashes, &'static str> {
    if !rect.stroke_dashes.is_empty() || rect.stroke_dash_offset != 0.0 {
        return Err(
            "Dashed Rectangle export is unsupported: FX starts the contour at upper-left while native AE Rectangle starts at upper-right, and no source-backed phase conversion has been established",
        );
    }
    Ok(StrokeDashes::default())
}

fn stroke_dashes(strokes: &[ShapeStrokeStyle]) -> Result<StrokeDashes, &'static str> {
    let Some(stroke) = strokes.first() else {
        return Ok(StrokeDashes::default());
    };
    StrokeDashes::new(
        stroke.dashes.iter().map(|dash| dash.value()),
        stroke.dash_offset,
    )
}

fn vector_rect(layer: &RectLayer, transform: &Transform) -> Result<VectorRectSpec, &'static str> {
    let rect = &layer.rect;
    if rect.fill_enabled == rect.stroke_enabled
        || rect.fill_paint.is_some()
        || rect
            .fill_blend_mode
            .is_some_and(|mode| mode != Default::default())
    {
        return Err(
            "Only one solid Fill or Stroke is exportable as a native Rectangle; other paint is not flattened",
        );
    }
    if layer.name.is_empty() || layer.name.len() > 255 || layer.name.contains('\0') {
        return Err("Native Rectangle name must be 1..=255 UTF-8 bytes without NUL");
    }
    let Position::TwoD(position) = transform.position else {
        return Err("3D Transform export is not implemented");
    };
    if transform.skew != 0.0
        || transform.skew_axis != 0.0
        || transform.rotation_x != 0.0
        || transform.rotation_y != 0.0
        || transform.orientation != [0.0; 3]
    {
        return Err("Skew and 3D Transform export are not implemented");
    }
    let center = [
        rect.position[0] + rect.size[0] / 2.0,
        rect.position[1] + rect.size[1] / 2.0,
    ];
    if rect
        .size
        .iter()
        .any(|value| !value.is_finite() || *value <= 0.0 || *value > 65535.0)
        || center.iter().any(|value| !value.is_finite())
        || !rect.roundness.is_finite()
        || !(0.0..=100000.0).contains(&rect.roundness)
        || !rect.stroke_miter_limit.is_finite()
        || rect.stroke_miter_limit < 0.0
        || transform
            .anchor_point
            .iter()
            .chain(&position)
            .chain(&transform.scale)
            .any(|value| !value.is_finite())
        || !transform.rotation.is_finite()
    {
        return Err("Rectangle geometry/Transform exceeds native static value bounds");
    }
    let color = if rect.fill_enabled {
        Some(rect.fill_color)
    } else {
        rect.stroke_color
    };
    if color.is_none_or(|color| {
        color
            .iter()
            .any(|channel| !channel.is_finite() || !(0.0..=1.0).contains(channel))
    }) {
        return Err("Rectangle paint requires a finite RGBA color in 0..=1");
    }
    if rect.stroke_enabled && !rect.stroke_width.value().is_finite() {
        return Err("Rectangle stroke width must be finite");
    }
    Ok(VectorRectSpec {
        name: layer.name.clone(),
        stroke_dashes: rect_stroke_dashes(rect)?,
        size: rect.size,
        position: center,
        roundness: rect.roundness,
        fill_color: rect.fill_enabled.then_some(rect.fill_color),
        stroke_color: rect.stroke_enabled.then_some(color.expect("checked color")),
        stroke_width: rect.stroke_width.value(),
        stroke_join: rect.stroke_join,
        stroke_miter_limit: rect.stroke_miter_limit,
        transform: SolidTransform {
            anchor: transform.anchor_point,
            position,
            scale: transform.scale,
            rotation: transform.rotation,
            opacity: transform.opacity.value(),
        },
    })
}

fn solid(layer: &RectLayer, transform: &Transform) -> Result<SolidLayerSpec, &'static str> {
    let rect = &layer.rect;
    if !rect.fill_enabled
        || rect.fill_paint.is_some()
        || rect
            .fill_blend_mode
            .is_some_and(|mode| mode != Default::default())
        || rect.stroke_enabled
        || rect.roundness != 0.0
        || rect.fill_color[3] != 1.0
    {
        return Err(
            "Only opaque solid-fill Rect content is exportable; paint, stroke, roundness and fill-alpha controls are not baked",
        );
    }
    let Position::TwoD(position) = transform.position else {
        return Err("3D Transform export is not implemented");
    };
    if transform.skew != 0.0
        || transform.skew_axis != 0.0
        || transform.rotation_x != 0.0
        || transform.rotation_y != 0.0
        || transform.orientation != [0.0; 3]
    {
        return Err("Skew and 3D Transform export are not implemented");
    }
    if layer.name.is_empty() || layer.name.len() > 255 || layer.name.contains('\0') {
        return Err("Native solid name must be 1..=255 UTF-8 bytes without NUL");
    }
    if rect
        .fill_color
        .iter()
        .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
    {
        return Err("Solid color lies outside the native 0..=1 range");
    }
    let anchor = [
        transform.anchor_point[0] - rect.position[0],
        transform.anchor_point[1] - rect.position[1],
    ];
    if anchor.iter().any(|value| !value.is_finite()) {
        return Err("Source-relative anchor overflow");
    }
    Ok(SolidLayerSpec {
        name: layer.name.clone(),
        width: dimension(rect.size[0])?,
        height: dimension(rect.size[1])?,
        // Native source RGB uses f32; conversion introduces at most f32 rounding.
        color: [
            rect.fill_color[0] as f32,
            rect.fill_color[1] as f32,
            rect.fill_color[2] as f32,
        ],
        transform: SolidTransform {
            anchor,
            position,
            scale: transform.scale,
            rotation: transform.rotation,
            opacity: transform.opacity.value(),
        },
    })
}

#[cfg(test)]
mod tests;
