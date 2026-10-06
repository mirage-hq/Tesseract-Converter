//! Experimental lowering from current editable FX values, never source AEP bytes.

mod animation_index;
mod audio;
mod composition_options;
mod directional_plane;
pub(crate) mod effects;
pub(crate) mod fonts;
mod hierarchy;
mod hierarchy_clock;
mod id_reservations;
mod layer_styles;
mod layout;
mod masks;
pub(crate) mod media;
mod media_clock;
mod mosaic_domain;
mod paint_color;
mod paint_controls;
mod paint_opacity;
mod path_animation;
mod radial_solid_origin;
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

use animation_index::AnimationIndex;
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
    effect::LayerEffect,
    layer::{ShapeGradientType, ShapeLayer, ShapePaint, ShapePathCommand, ShapeStrokeStyle},
};

use crate::writer::{
    AepWriteError, CompositionOptions, GeometryAnimations, KeyframeEasing, LayerReferenceFacts,
    LayerSpec, LayerTiming, NativeLayerOptions, NativeMaskMode, NativeMatteRef, NativeTransform3d,
    NullLayerSpec, NumericKeyframe, NumericTrack, PrecompositionSpec, RectAnimations,
    SolidLayerSpec, SolidTransform, StrokeAnimations, StrokeDashes, Transform3dAnimations,
    TransformAnimations, VectorContent, VectorGeometry, VectorGroupSpec, VectorGroupTransform,
    VectorLayerSpec, VectorModifierSpec, VectorPaintAnimations, VectorPaintSpec, VectorRectSpec,
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
    pub emitted_font_names: BTreeSet<String>,
}

#[derive(Clone, Copy)]
pub(crate) struct ExportDocumentViews<'a> {
    original: &'a EditableFxCompositionDocument,
    prepared: &'a EditableFxCompositionDocument,
    roots: &'a [Layer],
    original_roots: &'a [Layer],
    selected: bool,
    fonts: Option<&'a fonts::ArchiveFonts>,
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
            fonts: None,
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

    pub(crate) fn with_fonts(mut self, fonts: &'a fonts::ArchiveFonts) -> Self {
        self.fonts = Some(fonts);
        self
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
                &AnimationIndex::new(&geometry_dynamics),
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
    // The document envelope retains authored seconds even when its typed
    // duration projects them to integer milliseconds. Preserve that endpoint
    // before applying native composition frame quantization; layer clocks and
    // child source coverage remain on their existing timing paths.
    let original_json = documents
        .original
        .to_json_value()
        .map_err(|error| AepWriteError::InvalidDocument(error.to_string()))?;
    let authored_seconds = original_json["duration"]
        .as_f64()
        .ok_or(AepWriteError::Invalid(
            "missing authored composition duration",
        ))?;
    let (frames, duration) = rate.authored_duration(authored_seconds)?;
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
    let animation_index = AnimationIndex::new(document.composition().dynamics().entries());
    let mut lowerer = Lowerer {
        rate,
        duration,
        end: Time::ZERO.saturating_add(document.duration()),
        dynamics: &animation_index,
        dimensions,
        logical_dimensions: dimensions,
        mosaic_root_adjustments: root_layers
            .iter()
            .filter(|layer| {
                document.background_color().is_none()
                    && matches!(layer.data(), LayerData::Adjustment(_))
            })
            .map(Layer::id)
            .collect(),
        mosaic_cross_layer_inputs: mosaic_domain::has_cross_layer_inputs(root_layers),
        resolved_media,
        fonts: documents.fonts,
        composition_options,
        occupied_ids: id_reservations::IdReservations::new(source_variants.occupied_ids),
        source_variant_eligibility: source_variants.eligibility,
        source_variants: source_variants.decisions,
        consumed_guides: id_reservations::IdReservations::new(BTreeSet::new()),
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
    if (f64::from(duration.ticks()) / 24_576.0 - authored_seconds).abs() > 1.0 / 24_576.0 {
        lowerer.warn(None, format!("Authored duration {authored_seconds}s quantized to {frames} frames at {}fps using native nearest-frame composition rounding; layer clocks are unchanged.", rate.fps()));
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
        || document.composition().has_unknown_fields()
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
    lowerer.consumed_guides =
        id_reservations::IdReservations::new(lowerer.collect_consumed_guides(root_layers));
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
    crate::writer::validate_layers(&lowerer.layers, duration, rate)?;
    let retained_media_paths = emitted_media_paths(&lowerer.layers);
    let emitted_font_names = fonts::emitted_font_names(&lowerer.layers);
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
        emitted_font_names,
    })
}

struct Lowerer<'a> {
    rate: crate::timing::FrameRate,
    duration: crate::timing::Duration24,
    end: Time,
    dynamics: &'a AnimationIndex<'a>,
    dimensions: fx_schema::Dimensions,
    /// FX Group effects retain the document plane through native capture tiles.
    logical_dimensions: fx_schema::Dimensions,
    mosaic_root_adjustments: BTreeSet<LayerId>,
    mosaic_cross_layer_inputs: bool,
    resolved_media: &'a BTreeMap<String, media::ResolvedMediaSource>,
    fonts: Option<&'a fonts::ArchiveFonts>,
    composition_options: CompositionOptions,
    occupied_ids: id_reservations::IdReservations,
    source_variant_eligibility: BTreeMap<LayerId, source_variants::SourceVariantEligibility>,
    source_variants: BTreeMap<LayerId, source_variants::SourceVariantDecision>,
    consumed_guides: id_reservations::IdReservations,
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
    fn validate_source_clock(
        &mut self,
        owner: LayerId,
        clock: &crate::writer::source_clock::SourceClockPlan,
    ) -> Result<(), &'static str> {
        let rounding_error = clock
            .validate_time_remap_at_rate(self.rate)
            .map_err(|_| "Native Time Remap keys exceed the selected composition clock grammar")?;
        if rounding_error > 0.0 {
            self.warn(Some(owner), format!(
                "Time Remap key timing approximated by nearest native property ticks at {}fps (maximum authored-key error {} microseconds). Active in/out points and source values are retained; an inward-rounded final endpoint uses an outside-window same-value Hold guard.",
                self.rate.fps(), rounding_error * 1_000_000.0,
            ));
        }
        Ok(())
    }

    fn warn(&mut self, layer_id: Option<LayerId>, message: impl Into<String>) {
        self.diagnostics.push(ExportDiagnostic {
            layer_id,
            message: message.into(),
        });
    }

    fn collect_consumed_guides(&self, root_layers: &[Layer]) -> BTreeSet<LayerId> {
        if !root_layers.iter().any(has_source_guide_reference) {
            return BTreeSet::new();
        }
        self.probe_consumed_guides(root_layers)
    }

    fn probe_consumed_guides(&self, root_layers: &[Layer]) -> BTreeSet<LayerId> {
        let mut probe = Self {
            rate: self.rate,
            duration: self.duration,
            end: self.end,
            dynamics: self.dynamics,
            dimensions: self.dimensions,
            logical_dimensions: self.logical_dimensions,
            mosaic_root_adjustments: self.mosaic_root_adjustments.clone(),
            mosaic_cross_layer_inputs: self.mosaic_cross_layer_inputs,
            resolved_media: self.resolved_media,
            fonts: self.fonts,
            composition_options: self.composition_options,
            occupied_ids: self.occupied_ids.fork(),
            source_variant_eligibility: self.source_variant_eligibility.clone(),
            source_variants: self.source_variants.clone(),
            consumed_guides: id_reservations::IdReservations::new(BTreeSet::new()),
            inside_precomposition: false,
            layers: Vec::new(),
            diagnostics: Vec::new(),
            omitted_layer_ids: BTreeSet::new(),
        };
        let root_demand = hierarchy::root_demand(self.dimensions, self.end.as_millis());
        for layer in root_layers {
            probe.layer(layer, None, None, root_layers, 0, true, &root_demand);
        }
        probe.consumed_guides.into_set()
    }

    fn inline_vector_group_eligible(&self, group: &GroupLayer, depth: usize) -> bool {
        let mut ignored_approximations = Vec::new();
        !group.layers.is_empty()
            && inline_vector_group(group)
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

    fn suppress_matte_display(&self, layer: &Layer, options: &mut NativeLayerOptions) {
        // FX consumes a matte dependency instead of painting it independently.
        // AE still samples a disabled provider through the owner's matte link.
        if self
            .source_variant_eligibility
            .get(&layer.id())
            .is_some_and(|facts| facts.referenced_as_matte)
        {
            options.enabled = false;
        }
    }

    fn prepare_layer_options(
        &mut self,
        layer: &Layer,
        parent: Option<LayerId>,
        inherited: Option<(&Transform, LayerId)>,
        siblings: &[Layer],
        dynamics: &AnimationIndex<'_>,
        output_visible: bool,
    ) -> Result<(NativeLayerOptions, Option<u16>), &'static str> {
        let mut options = native_layer_options(layer)?;
        self.suppress_matte_display(layer, &mut options);
        let source_size = self.layer_source_size(layer)?;
        let effect_size = match layer.data() {
            LayerData::Rect(rect) => rect.rect.size,
            _ => source_size.map(f64::from),
        };
        // Group effect coordinates belong to the generated precomposition,
        // whose finite dimensions are not available until hierarchy planning.
        if !matches!(layer.data(), LayerData::Group(_)) {
            let mosaic_domain = mosaic_domain::scope(
                self.inside_precomposition,
                parent,
                inherited.is_some(),
                self.mosaic_root_adjustments.contains(&layer.id()),
                self.dimensions == self.logical_dimensions,
                self.mosaic_cross_layer_inputs,
            )
            .map(|scope| {
                mosaic_domain::classify(
                    layer,
                    siblings,
                    dynamics,
                    self.logical_dimensions,
                    self.rate,
                    self.resolved_media,
                    &scope,
                )
            });
            let (effects, styles) = self.lower_effect_stack(
                layer.id(),
                layer.data().effects(),
                dynamics,
                effect_size,
                mosaic_domain.as_ref(),
            );
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
                coordinate_owner: Some(layer.id()),
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
            let content = media_matte_content_view(layer)
                .map_err(|_| "Media matte content view could not be constructed")?;
            let footage =
                media::lower_with_transform(&content, source, self.dimensions, selected_transform)?;
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
        dynamics: &AnimationIndex<'_>,
        size: [f64; 2],
        mosaic_domain: Option<&mosaic_domain::MosaicDomain>,
    ) -> (
        Vec<crate::writer::effects::NativeEffect>,
        Vec<crate::layer_styles::NativeLayerStyle>,
    ) {
        let lowered = effects::lower_at_rate_with_mosaic_domain(
            records,
            dynamics,
            size,
            self.rate,
            mosaic_domain,
        );
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

    fn omit_unlowerable_root_adjustment(
        &mut self,
        layer: &Layer,
        parent: Option<LayerId>,
        inherited: Option<(&Transform, LayerId)>,
        siblings: &[Layer],
        output_visible: bool,
    ) -> bool {
        let LayerData::Adjustment(adjustment) = layer.data() else {
            return false;
        };
        if !output_visible
            || self.inside_precomposition
            || parent.is_some()
            || inherited.is_some()
            || adjustment.masks.is_empty()
        {
            return false;
        }
        let owner_facts = self
            .source_variant_eligibility
            .get(&layer.id())
            .copied()
            .unwrap_or_default();
        if owner_facts.has_external_references()
            || owner_facts.referenced_by_animation_dependency
            || owner_facts.has_unresolved_animation_dependency
        {
            return false;
        }
        // Only source-only root Shape guides can leave with this failed unit.
        // Native matte/parent/animation consumers still need their identity;
        // hidden owners and other failed-owner rollback paths stay unchanged.
        for id in adjustment.masks.iter().filter_map(|mask| mask.layer) {
            let Some(guide) = siblings.iter().find(|guide| guide.id() == id) else {
                return false;
            };
            let LayerData::Shape(shape) = guide.data() else {
                return false;
            };
            let facts = self
                .source_variant_eligibility
                .get(&id)
                .copied()
                .unwrap_or_default();
            if facts.nested_or_parented
                || facts.owns_masks_or_matte
                || facts.referenced_as_parent
                || facts.referenced_as_matte
                || facts.referenced_as_ai_edit_source
                || facts.referenced_as_segment
                || facts.referenced_by_animation_dependency
                || facts.referenced_by_animation_layer_ref
                || facts.has_unresolved_animation_dependency
                || !shape.effects.is_empty()
            {
                return false;
            }
        }
        let transform = identity_fx_transform();
        let lowered = masks::lower(
            &adjustment.masks,
            None,
            masks::MaskOwner {
                coordinate_owner: Some(layer.id()),
                parent: None,
                transform: &transform,
                source_size: [self.dimensions.width, self.dimensions.height],
                clock: Some(layer.active_range()),
            },
            siblings,
            self.dynamics,
        );
        if !lowered.has_omitted_gating_mask {
            return false;
        }
        for diagnostic in lowered.diagnostics {
            self.warn(Some(layer.id()), diagnostic);
        }
        // Source path-mask consumption does not depend on successful export.
        // Reserve it before ordinary per-layer transactions so a later failed
        // Text owner cannot roll back this Adjustment's source-only consumption.
        self.consumed_guides
            .extend(adjustment.masks.iter().filter_map(|mask| mask.layer));
        self.omitted_layer_ids.insert(layer.id());
        self.warn(Some(layer.id()), "owner omitted: root Adjustment has an unlowerable coverage mask; unsafe unmasked effects omitted; source-only guides remain consumed without a native copy of the unsupported mask geometry");
        true
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
        // Drop the effect, not its content. Only adjustments and plain white
        // shader canvases have no independent content under the export policy.
        if effects::has_custom_shader(layer.data().effects()) {
            for record in layer.data().effects() {
                if effects::has_custom_shader(std::slice::from_ref(record))
                    && let Some(message) = effects::unmapped_warning(record)
                {
                    self.warn(Some(layer.id()), message);
                }
            }
            if shader_canvas_owner(layer) {
                self.omitted_layer_ids.insert(layer.id());
                self.warn(
                    Some(layer.id()),
                    "owner omitted: CustomShader adjustment or plain white shader canvas",
                );
                // These source variants cannot own children. Groups are retained
                // with their ordinary parenting/transforms, never promoted here.
                return;
            }
            // Re-enter ordinary lowering with the same owner and children, but
            // without shader records. This also removes shader-only boundaries
            // from vector/hierarchy eligibility; no shader execution occurs.
            let mut wire = layer.wire_value().clone();
            if let Some(records) = wire["effects"].as_array_mut() {
                records.retain(|record| {
                    let payload = record.get("effect").unwrap_or(record);
                    payload.get("type").and_then(serde_json::Value::as_str) != Some("customShader")
                });
            }
            match serde_json::from_value::<Layer>(wire) {
                Ok(unshaded) => {
                    self.layer(
                        &unshaded,
                        parent,
                        inherited,
                        siblings,
                        depth,
                        ancestor_visible,
                        demand,
                    );
                    return;
                }
                Err(error) => self.warn(
                    Some(layer.id()),
                    format!("Cannot remove CustomShader record from owner: {error}"),
                ),
            }
        }
        let output_visible = ancestor_visible && !layer_is_hidden(layer);
        if self.omit_unlowerable_root_adjustment(layer, parent, inherited, siblings, output_visible)
        {
            return;
        }
        let start = self.layers.len();
        let diagnostics_before = self.diagnostics.len();
        let consumed_guides_before = self.consumed_guides.checkpoint();
        let occupied_ids_before = self.occupied_ids.checkpoint();
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
                        crate::writer::validate_layer_payload(output, self.duration, self.rate)
                            .map_err(|error| error.to_string())?;
                    }
                    Ok(())
                });
            if let Err(reason) = result {
                self.layers.truncate(start);
                self.diagnostics.truncate(diagnostics_before);
                self.omitted_layer_ids.insert(layer.id());
                self.consumed_guides.rollback(consumed_guides_before);
                self.occupied_ids.rollback(occupied_ids_before);
                self.warn(
                    Some(layer.id()),
                    format!(
                        "{reason}; layer and its subtree omitted, convertible siblings retained."
                    ),
                );
            }
            return;
        }
        let static_text_skew = matches!(layer.data(), LayerData::Text(text)
            if text.transform.skew != 0.0 || text.transform.skew_axis != 0.0);
        let result = if static_text_skew {
            self.lower_static_text_skew(layer, parent, inherited, output_visible)
        } else {
            self.prepare_layer_options(
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
        }
        .map_err(str::to_owned)
        .and_then(|()| {
            paint_color::retain_supported_tracks(&mut self.layers[start..], &mut self.diagnostics);
            for output in &self.layers[start..] {
                crate::writer::validate_layer_payload(output, self.duration, self.rate)
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
                self.consumed_guides.rollback(consumed_guides_before);
                self.occupied_ids.rollback(occupied_ids_before);
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
                        let lowered = effects::lower_at_rate(
                            &group.effects,
                            self.dynamics,
                            [
                                f64::from(self.dimensions.width),
                                f64::from(self.dimensions.height),
                            ],
                            self.rate,
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
                        if crate::writer::validate_layer_payload(&output, self.duration, self.rate)
                            .is_ok()
                        {
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
                        if crate::writer::validate_layer_payload(&output, self.duration, self.rate)
                            .is_ok()
                        {
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
                // A shader-only FX Adjustment has no native effect to apply;
                // its solid-backed native placeholder may obscure footage.
                let shader_only = effects::omitted_shader_adjustment(&adjustment.effects);
                if shader_only && options.effects.is_empty() && options.styles.is_empty() {
                    return Err(
                        "Adjustment has only unsupported CustomShader effects and no native effects; an effectless solid may obscure supported footage",
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
                    if !self.inside_precomposition
                        && parent.is_none()
                        && inherited.is_none()
                        && transform_animations == TransformAnimations::default()
                        && directional_plane::compensate_static_solid(
                            rect,
                            transform,
                            options,
                            self.dynamics,
                        )
                    {
                        self.warn(Some(rect.id), "Static isolated native Solid Directional Blur controls were converted from FX screen-space into the source plane using the edited owner Scale/Rotation; native/FX sampling kernels remain approximate.");
                    }
                    if rect.rect.position != [0.0; 2]
                        && options
                            .effects
                            .iter()
                            .any(|effect| effect.enabled && effect.match_name == "ADBE Radial Blur")
                    {
                        let rebased = !self.inside_precomposition
                            && parent.is_none()
                            && inherited.is_none()
                            && transform_animations == TransformAnimations::default()
                            && radial_solid_origin::rebase_static_solid(
                                rect,
                                transform,
                                options,
                                self.dynamics,
                            );
                        self.warn(Some(rect.id), if rebased {
                            "Static isolated native Solid Radial Blur center was translated from the edited Rect-local origin into the zero-origin source plane, matching Anchor rebasing; native Zoom/FX sampling kernels remain approximate."
                        } else {
                            "Radial Blur center on a nonzero-origin Solid is outside the static isolated point-rebasing profile; original controls retained as a diagnosed source-plane approximation."
                        });
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
                        modifier_animations(self.dynamics, shape.id, shape.shape.trim.is_some())?,
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
                if self.dynamics.for_layer(boolean.id).any(|entry| {
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
                    self.fonts,
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

    fn lower_static_text_skew(
        &mut self,
        layer: &Layer,
        parent: Option<LayerId>,
        inherited: Option<(&Transform, LayerId)>,
        output_visible: bool,
    ) -> Result<(), &'static str> {
        let LayerData::Text(text_layer) = layer.data() else {
            return Err("Static Text skew dispatch requires Text");
        };
        if effective_parent(layer, parent) != parent {
            return Err("Non-containment transform parent is not yet exportable");
        }
        if inherited.is_some() {
            return Err("Static Text skew cannot flatten an inherited transform");
        }
        if self.dynamics.iter().any(|entry| {
            matches!(&entry.target, fx_schema::PropertyTarget::LayerProperty(target)
                if target.layer_id() == text_layer.id
                    && matches!(target.property_type(), PropType::Skew | PropType::SkewAxis))
        }) {
            return Err("Text skew requires fixed skew and skew axis controls");
        }
        if !text_layer.effects.is_empty() {
            return Err("Static Text skew cannot move effects across affine helpers");
        }
        if !text_layer.masks.is_empty() || text_layer.path_options.is_some() {
            return Err("Static Text skew cannot move masks or Path Text across affine helpers");
        }
        if !text_layer.animators.is_empty() || text_layer.motion_blur {
            return Err("Static Text skew cannot change animators or motion blur raster space");
        }
        // Factor only the constant shear. Animated scale (including zero) stays
        // on the drawable, so no time-dependent decomposition is necessary.
        let mut shear = identity_fx_transform();
        shear.skew = text_layer.transform.skew;
        shear.skew_axis = text_layer.transform.skew_axis;
        let (basis_transform, inner) =
            hierarchy::skew::lower_static_text(&shear, &text_layer.name)?;
        let outer_id = self.available_generated_id(text_layer.id)?;
        self.occupied_ids.insert(outer_id);
        let inner_id = self.available_generated_id(outer_id)?;
        self.occupied_ids.insert(inner_id);
        // Structural containment is finalized by the Group plan. A Null
        // hierarchy assigns its parent to this root; a precomposition keeps
        // it source-local, with no reference to the outer occurrence.
        let mut outer_options = hierarchy::skew::helper_options(outer_id);
        let basis_id = self.available_generated_id(inner_id)?;
        self.occupied_ids.insert(basis_id);
        let mut basis_options = hierarchy::skew::helper_options(basis_id);
        basis_options.parent = Some(outer_id);
        let mut inner_options = hierarchy::skew::helper_options(inner_id);
        inner_options.parent = Some(basis_id);
        let mut text_options = native_layer_options(layer)?;
        if text_options.transform_3d.is_some() {
            return Err("Text skew requires a planar drawable");
        }
        text_options.parent = Some(inner_id);
        text_options.enabled &= output_visible;
        if self
            .source_variant_eligibility
            .get(&layer.id())
            .is_some_and(|facts| facts.referenced_as_matte)
        {
            text_options.enabled = false;
        }
        let mut drawable_transform = text_layer.transform;
        drawable_transform.skew = 0.0;
        drawable_transform.skew_axis = 0.0;
        // The shear helpers move only placement and rotation off the drawable.
        // Reuse native separated Position followers for independent scalar knots,
        // without moving anchor, scale, opacity or Source Text onto the helper.
        let is_placement_entry = |entry: &fx_schema::animator::AnimationGraphEntry| {
            matches!(&entry.target, fx_schema::PropertyTarget::LayerProperty(target)
                if target.layer_id() == text_layer.id
                    && matches!(target.property_type(),
                        PropType::PositionX | PropType::PositionY | PropType::Rotation))
        };
        let placement_entries = self
            .dynamics
            .iter()
            .filter(|entry| is_placement_entry(entry))
            .cloned()
            .collect::<Vec<_>>();
        let mut placement_transform = identity_fx_transform();
        placement_transform.position = drawable_transform.position;
        placement_transform.rotation = drawable_transform.rotation;
        let separated_placement = transform3d::lower(
            &AnimationIndex::new(&placement_entries),
            &placement_transform,
            text_layer.id,
            transform3d::Native2dGeometry::IDENTITY,
        )?;
        let drawable_entries = separated_placement.as_ref().map(|_| {
            self.dynamics
                .iter()
                .filter(|entry| !is_placement_entry(entry))
                .cloned()
                .collect::<Vec<_>>()
        });
        let drawable_index = drawable_entries
            .as_ref()
            .map(|entries| AnimationIndex::new(entries));
        let mut lowered = text::lower_with_native_3d(
            text_layer,
            &drawable_transform,
            text_layer.id,
            drawable_index.as_ref().unwrap_or(self.dynamics),
            None,
            false,
            self.fonts,
        )?;
        if let Some(placement) = separated_placement {
            outer_options.transform_3d = Some((placement.transform, placement.animations));
        }
        let outer_transform = SolidTransform {
            anchor: [0.0; 2],
            position: lowered.spec.transform.position,
            scale: [100.0; 2],
            rotation: lowered.spec.transform.rotation,
            opacity: 100.0,
        };
        let outer_animations = TransformAnimations {
            position: lowered.spec.transform_animations.position.take(),
            rotation: lowered.spec.transform_animations.rotation.take(),
            ..TransformAnimations::default()
        };
        lowered.spec.transform.position = [0.0; 2];
        lowered.spec.transform.rotation = 0.0;
        for diagnostic in lowered.diagnostics {
            self.warn(Some(text_layer.id), diagnostic);
        }
        let range = layer.active_range();
        let start_millis = i64::try_from(range.start.as_millis())
            .map_err(|_| "Layer start exceeds native clock range")?;
        let end_millis = i64::try_from(range.end().min(self.end).as_millis())
            .map_err(|_| "Layer end exceeds native clock range")?;
        if start_millis >= end_millis {
            return Err("Layer has no positive active span inside the composition");
        }
        let timing = LayerTiming {
            start_millis,
            end_millis,
        };
        for (output, options) in [
            (
                LayerSpec::Null(NullLayerSpec {
                    name: format!("{} — Skew placement", text_layer.name),
                    transform: outer_transform,
                    transform_animations: outer_animations,
                }),
                outer_options,
            ),
            (
                LayerSpec::Null(NullLayerSpec {
                    name: format!("{} — Skew factor", text_layer.name),
                    transform: basis_transform,
                    transform_animations: TransformAnimations::default(),
                }),
                basis_options,
            ),
            (LayerSpec::Null(inner), inner_options),
            (LayerSpec::Text(lowered.spec), text_options),
        ] {
            let output = if range.start != Time::ZERO || range.end() < self.end {
                LayerSpec::Timed(Box::new(output), timing)
            } else {
                output
            };
            self.layers
                .push(LayerSpec::Options(Box::new(output), options));
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
                    (parent, output_visible),
                )
            }
            source_variants::SourceVariantDecision::Ready(plan) => match plan.publication {
                source_variants::SourceVariantPublication::DirectOccurrences => {
                    for variant in plan.variants {
                        let variant_index = AnimationIndex::new(&variant.owner_entries);
                        let (mut options, _) = self.prepare_layer_options(
                            &variant.layer,
                            parent,
                            inherited,
                            siblings,
                            &variant_index,
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
                        let (selected, owner) =
                            selected_media_transform(&variant.layer, inherited, &variant_index)?;
                        self.emit_media_occurrence(
                            &variant.layer,
                            &variant_index,
                            options,
                            selected,
                            owner,
                            (parent, output_visible),
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
            self.occupied_ids.as_set(),
            takeover::TakeoverGraphFacts {
                has_source_variants,
                has_external_references: eligibility.has_external_references(),
                has_unsupported_graph_edges: eligibility.has_unsupported_graph_edges(),
            },
        )
        .map_err(takeover_error)?;
        crate::writer::validate_layers(&plan.layers, self.duration, self.rate)
            .map_err(takeover_error)?;
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
        let mut next = seed.value();
        let mut reserve = || loop {
            next = next
                .checked_add(1)
                .ok_or("Takeover synthetic identity space is exhausted")?;
            let candidate = LayerId::new(next);
            // `next` only increases, so the two local choices cannot collide.
            if !self.occupied_ids.contains(&candidate) {
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
        self.suppress_matte_display(layer, &mut options);
        let lowered_effects = effects::lower_at_rate(
            layer.data().effects(),
            self.dynamics,
            [f64::from(handoff.width), f64::from(handoff.height)],
            self.rate,
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
                coordinate_owner: Some(layer.id()),
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
                &AnimationIndex::new(&child_dynamics),
                child_options,
                Some(&identity),
                variant.occurrence_id,
                (None, output_visible),
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
        let mut wrapper_animations = solid_transform_animations(
            &AnimationIndex::new(&wrapper_dynamics),
            layer.id(),
            &wrapper_transform,
            false,
        )?;
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
        dynamics: &AnimationIndex<'_>,
        mut options: NativeLayerOptions,
        selected_transform: Option<&Transform>,
        transform_owner: LayerId,
        (parent, output_visible): (Option<LayerId>, bool),
    ) -> Result<(), &'static str> {
        let request = media::request(layer).ok_or("Media layer has no archive request")?;
        let resolved = self
            .resolved_media
            .get(request.asset_id.as_str())
            .ok_or("Media archive source was not resolved/staged")?;
        let content = media_matte_content_view(layer)
            .map_err(|_| "Media matte content view could not be constructed")?;
        let mut footage = match selected_transform {
            Some(transform) => {
                media::lower_with_transform(&content, resolved, self.dimensions, transform)?
            }
            None => media::lower(&content, resolved, self.dimensions)?,
        };
        // Imported still primitives may be effectively unbounded: their
        // enclosing source composition owns the finite reachable interval.
        // Only unrepresentable native outpoints need this intersection. Keep
        // representable authored tails unchanged, even beyond the composition.
        if let crate::writer::footage::FootageClock::Still {
            start_millis,
            duration_millis,
        } = &mut footage.clock
            && start_millis
                .checked_add(*duration_millis)
                .is_none_or(|end| crate::writer::footage::ticks_from_millis_unsigned(end).is_err())
        {
            *duration_millis =
                (*duration_millis).min(self.end.as_millis().saturating_sub(*start_millis));
            if *duration_millis == 0 {
                return Err("Still image lies outside its native source composition interval");
            }
        }
        if let LayerData::Audio(audio) = layer.data()
            && resolved.format == crate::writer::footage::NativeSourceFormat::Wave
            && audio.source_intrinsic_duration.as_millis().checked_add(1)
                == Some(resolved.duration_millis)
        {
            self.warn(Some(layer.id()), "Native WAVE's final sample rounds its source duration one millisecond beyond the FX-authored duration. The authored source selection and playback clock are retained; no samples were trimmed or retimed.");
        }
        if let crate::writer::footage::FootageClock::Source(clock) = &footage.clock {
            self.validate_source_clock(layer.id(), clock)?;
        }
        if footage.kind == crate::writer::footage::FootageKind::Video
            && !footage.audio_enabled
            && footage.static_source_time_secs.is_none()
            && let crate::writer::footage::FootageClock::Source(clock) = &mut footage.clock
            && !clock.has_time_remap()
        {
            match clock.apply_rounded_millisecond_visibility() {
                Ok(()) => self.warn(Some(layer.id()), "Video rounded-millisecond visibility uses a half-millisecond parent-boundary correction. Nominal source start/stretch and Transform key clocks are retained; continuous native footage sampling does not reproduce FX source-start clamping or rounded source-frame selection exactly."),
                Err(error) => self.warn(Some(layer.id()), format!("Video rounded-millisecond visibility cannot be represented: {error}. The nominal editable source clock and visible interval are retained without shifting source samples or omitting the Video.")),
            }
        }
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
        let audio_gain = match layer.data() {
            LayerData::Audio(audio) => Some(audio.volume.as_f64()),
            LayerData::Video(video) => video.volume.map(|gain| gain.as_f64()),
            _ => None,
        };
        if let Some(gain) = audio_gain
            && audio::exactly_silent(dynamics, layer.id(), gain)?
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
            self.warn(Some(layer.id()), "Audio gain at/below -192 dB is stored at a finite floor; exactly silent Audio/Video uses the static audio-off switch, but zero within mixed zero/nonzero animation is only near-silence.");
        }
        // Apply inherited visibility after lowering levels so muted Video keeps
        // its editable gain tracks, just like Audio. A container's eye switch
        // alone does not mute its source composition's audio in AE.
        footage.audio_enabled &= output_visible;
        if footage.time_remap_requires_source_owned_transform && animations.opacity.is_some() {
            if effective_parent(layer, parent) != parent {
                return Err(
                    "Source-clock Opacity plane has an unproved non-containment transform parent",
                );
            }
            return self.emit_remapped_opacity_source_plane(layer, footage, animations, options);
        }
        media::validate_native(&footage)
            .map_err(|_| "Native footage validation rejected current media semantics")?;
        self.layers.push(LayerSpec::Options(
            Box::new(LayerSpec::Footage(footage, animations)),
            options,
        ));
        Ok(())
    }

    /// A bounded source-clock plane: only full-canvas, silent 2D Video with
    /// source-owned Opacity (and Mosaic) crosses this branch. The general writer
    /// still rejects remapped occurrence Transform keys.
    fn emit_remapped_opacity_source_plane(
        &mut self,
        layer: &Layer,
        mut footage: crate::writer::footage::FootageSpec,
        animations: TransformAnimations,
        mut options: NativeLayerOptions,
    ) -> Result<(), &'static str> {
        let LayerData::Video(video) = layer.data() else {
            return Err("Source-clock Opacity plane requires explicit Video");
        };
        let full_canvas = footage.source.dimensions.map(u32::from)
            == [self.dimensions.width, self.dimensions.height]
            && footage.source_geometry.origin == [0.0; 2]
            && footage.source_geometry.scale == [1.0; 2];
        let only_mosaic = video.effects.iter().all(|record| {
            let payload = match record.data() {
                fx_schema::EffectData::Identified { effect, .. }
                | fx_schema::EffectData::Legacy(effect) => effect,
            };
            matches!(
                payload,
                fx_schema::EffectPayload::Known(fx_schema::LayerEffect::Mosaic { .. })
            )
        });
        // Opacity belongs to the source controls; only geometric TRS must be
        // identity. Preparation may seed the static value from the first key.
        let geometric_transform = Transform {
            opacity: identity_fx_transform().opacity,
            ..video.transform
        };
        let selection_duration = video.source_range.duration.as_millis();
        let full_selection = video.source_range.start == Time::ZERO
            && (selection_duration == footage.source.duration_millis
                || selection_duration.checked_add(1) == Some(footage.source.duration_millis));
        let source_selector = self.dynamics.iter().any(|entry| {
            entry.target.as_property().is_some_and(|property| {
                property.layer_id() == layer.id()
                    && property.property_type() == PropType::MediaSourceAssetId
            })
        });
        if !full_canvas
            || !full_selection
            || source_selector
            || !identity(&geometric_transform)
            || footage.audio_enabled
            || footage.audio_levels_animation.is_some()
            || footage.static_source_time_secs.is_some()
            || footage.frame_blending != crate::writer::footage::NativeFrameBlending::Disabled
            || animations.opacity.as_ref().is_some_and(|track| {
                track.keys.iter().skip(1).any(|key| {
                    key.easing
                        .iter()
                        .any(|ease| *ease != KeyframeEasing::Linear)
                })
            })
            || animations.anchor.is_some()
            || animations.position.is_some()
            || animations.scale.is_some()
            || animations.rotation.is_some()
            || options.transform_3d.is_some()
            || options.motion_blur
            || options.blend_mode != 2
            || options.matte.is_some()
            || !options.masks.is_empty()
            || !options.styles.is_empty()
            || !only_mosaic
            || self
                .source_variant_eligibility
                .get(&layer.id())
                .is_some_and(|facts| {
                    // An incoming parent/tree edge stays on the unchanged outer
                    // occurrence. References consuming this owner's geometry
                    // are not proved by the bounded source plane.
                    let references = source_variants::SourceVariantEligibility {
                        nested_or_parented: false,
                        ..*facts
                    };
                    references.has_external_references() || references.has_unsupported_graph_edges()
                })
        {
            return Err(
                "Source-clock Opacity plane has unproved geometry, audio, effects or dependencies",
            );
        }
        let crate::writer::footage::FootageClock::Source(occurrence_clock) = &footage.clock else {
            return Err("Source-clock Opacity plane has no source clock");
        };
        if !occurrence_clock.has_time_remap() {
            return Err("Source-clock Opacity plane requires keyed Time Remap");
        }
        self.validate_source_clock(layer.id(), occurrence_clock)?;
        let source_millis = footage.source.duration_millis;
        // Adobe save/reopen normalizes a composition duration to its frame
        // cadence. Do not admit a remap whose control hull reaches the portion
        // of the movie outside that native container, or move any source key.
        let (_, normalized_duration) =
            self.rate
                .authored_duration(source_millis as f64 / 1000.0)
                .map_err(|_| "Source-clock Opacity plane native duration is invalid")?;
        let normalized_millis = u64::from(normalized_duration.ticks()) * 1000 / 24_576;
        let remap = playback_time_remap(&video.playback)
            .ok_or("Source-clock Opacity plane requires explicit keyed playback")?;
        crate::writer::source_clock::SourceClockPlan::time_remap_with_offset(
            occurrence_clock.active_range,
            remap,
            normalized_millis,
            video.playback.input_offset_ms(),
        )
        .map_err(
            |_| "Source-clock Opacity plane remap exceeds the native frame-aligned container",
        )?;
        options.source_clock = Some(occurrence_clock.clone());
        let source_duration = hierarchy_clock::duration24(source_millis)
            .map_err(|_| "Source-clock Opacity plane duration exceeds native bounds")?;
        let source_span = TimeRangeProperty {
            start: Time::ZERO,
            duration: fx_schema::Duration::from_millis(source_millis),
        };
        footage.clock = crate::writer::footage::FootageClock::Source(
            crate::writer::source_clock::SourceClockPlan::affine(
                source_span,
                Time::ZERO,
                Time::from_millis(source_millis),
                source_millis,
            )
            .map_err(|_| "Source-clock Opacity plane cannot preserve unit source timing")?,
        );
        footage.time_remap_requires_source_owned_transform = false;
        footage.name = format!("{} source controls", layer.data().name());
        footage.transform.name = footage.name.clone();
        media::validate_native(&footage)
            .map_err(|_| "Source-clock Opacity plane footage validation failed")?;
        let child_id = self.available_generated_id(layer.id())?;
        let mut child_options = native_layer_options(layer)?;
        child_options.fx_id = child_id;
        child_options.parent = None;
        child_options.effects = std::mem::take(&mut options.effects);
        child_options.enabled = true;
        let mut record = crate::schema::CompositionRecord::empty_ae26(
            footage.source.dimensions[0],
            footage.source.dimensions[1],
            source_duration,
        )
        .map_err(|_| "Source-clock Opacity plane composition record failed")?;
        crate::writer::apply_composition_options(&mut record, self.composition_options)
            .map_err(|_| "Source-clock Opacity plane composition options failed")?;
        let width = footage.source.dimensions[0];
        let height = footage.source.dimensions[1];
        self.occupied_ids.insert(child_id);
        self.layers.push(LayerSpec::Options(
            Box::new(LayerSpec::Precomposition(PrecompositionSpec {
                name: layer.data().name().to_owned(),
                collapse_transformations: false,
                width,
                height,
                duration: source_duration,
                transform: identity_solid_transform(),
                transform_animations: TransformAnimations::default(),
                layers: vec![LayerSpec::Options(
                    Box::new(LayerSpec::Footage(footage, animations)),
                    child_options,
                )],
                composition_record: Some(record),
            })),
            options,
        ));
        self.warn(Some(layer.id()), "Video source-owned Opacity and Mosaic remain editable in a noncollapsed full-canvas source-clock precomposition; the original occurrence owns Time Remap and visibility. Continuous FX source-frame selection and between-sample script fidelity remain approximate.");
        Ok(())
    }

    fn lower_group(
        &mut self,
        group: &GroupLayer,
        options: NativeLayerOptions,
        parent_siblings: &[Layer],
        depth: usize,
        output_visible: bool,
        demand: &hierarchy::Demand,
    ) -> Result<(), &'static str> {
        if group.effects.iter().any(|record| {
            let payload = match record.data() {
                EffectData::Identified { effect, .. } | EffectData::Legacy(effect) => effect,
            };
            matches!(payload, EffectPayload::Known(LayerEffect::Twirl { .. }))
        }) {
            let referenced = self
                .source_variant_eligibility
                .get(&group.id)
                .is_some_and(|facts| {
                    facts.referenced_as_noncontainer_parent
                        || facts.referenced_as_matte
                        || facts.referenced_as_mask_guide
                        || facts.referenced_as_text_guide
                        || facts.referenced_as_ai_edit_source
                        || facts.referenced_as_segment
                        || facts.referenced_by_animation_dependency
                        || facts.referenced_by_animation_layer_ref
                });
            let identity_owner_clock = group_has_root_identity_clock(group, self.end)
                && playback_active_range(&group.playback).end() == self.end;
            if !identity_owner_clock {
                self.warn(Some(group.id), "Twirl frame-plane staging declined: own Group clock/window is not full-span identity; moving effects outside would lose their remap or active window. Editable owner, content clock and effects retained on the ordinary native occurrence; effect-clock fidelity remains approximate.");
            }
            if identity_owner_clock
                && !referenced
                && options.transform_3d.is_none()
                && options.parent.is_none()
                && options.matte.is_none()
                && group.masks.is_empty()
                && !group.effects.iter().any(effect_is_layer_style)
            {
                let carrier_id = self.available_generated_id(group.id)?;
                self.occupied_ids.insert(carrier_id);
                let mut content = group.clone();
                content.effects.clear();
                // FX composites owner opacity after the ordered effect image.
                // Only geometry and its tracks belong inside this carrier.
                content.transform.opacity =
                    fx_schema::PercentageProperty::new(100.0).expect("100 is a valid percentage");
                let mut carrier_transform = identity_solid_transform();
                carrier_transform.opacity = group.transform.opacity.value();
                let carrier_animations = TransformAnimations {
                    opacity: scalar_track(
                        track(self.dynamics, group.id, PropType::Opacity)?,
                        100.0,
                    )?,
                    ..TransformAnimations::default()
                };
                let mut child_options = options.clone();
                child_options.enabled = true;
                child_options.blend_mode = 2;
                let child_start = self.layers.len();
                self.lower_group_original(
                    &content,
                    child_options,
                    parent_siblings,
                    depth,
                    output_visible,
                    demand,
                    true,
                )?;
                let children = self.layers.drain(child_start..).collect();
                let (effects, styles) = self.lower_effect_stack(
                    group.id,
                    &group.effects,
                    self.dynamics,
                    [
                        f64::from(self.dimensions.width),
                        f64::from(self.dimensions.height),
                    ],
                    None,
                );
                let mut carrier_options = options;
                carrier_options.fx_id = carrier_id;
                carrier_options.effects = effects;
                carrier_options.styles = styles;
                carrier_options.motion_blur = false;
                let width = u16::try_from(self.dimensions.width)
                    .map_err(|_| "Twirl frame width exceeds native range")?;
                let height = u16::try_from(self.dimensions.height)
                    .map_err(|_| "Twirl frame height exceeds native range")?;
                let mut record =
                    crate::schema::CompositionRecord::empty_ae26(width, height, self.duration)
                        .map_err(|_| "Twirl frame composition record failed")?;
                crate::writer::apply_composition_options(&mut record, self.composition_options)
                    .map_err(|_| "Twirl frame composition options failed")?;
                self.layers.push(LayerSpec::Options(
                    Box::new(LayerSpec::Precomposition(PrecompositionSpec {
                        name: format!("{} Twirl frame", group.name),
                        collapse_transformations: false,
                        width,
                        height,
                        duration: self.duration,
                        transform: carrier_transform,
                        transform_animations: carrier_animations,
                        layers: children,
                        composition_record: Some(record),
                    })),
                    carrier_options,
                ));
                self.warn(Some(group.id), "Twirl frame-plane staging: current transformed content and its source clock are inside a full-frame native carrier; ordered effects and keys are outside on a spatially identity occurrence with post-effect owner opacity. CornerPin uses the full frame. Ancestor transforms, native kernel, radius falloff and frame edges remain approximate.");
                return Ok(());
            }
            self.warn(Some(group.id), "Twirl frame-plane staging unavailable for nonidentity own clock/window, referenced, masked/matted, 3D or Layer Style owner; retained editable native source-local controls differ from FX post-transform frame UV.");
        }
        if !group.layers.iter().any(group_descendant_has_text) {
            return self.lower_group_original(
                group,
                options,
                parent_siblings,
                depth,
                output_visible,
                demand,
                false,
            );
        }
        let diagnostic_count = self.diagnostics.len();
        let occupied_ids = self.occupied_ids.checkpoint();
        let consumed_guides = self.consumed_guides.checkpoint();
        let omitted_ids = self.omitted_layer_ids.clone();
        let result = self.lower_group_original(
            group,
            options.clone(),
            parent_siblings,
            depth,
            output_visible,
            demand,
            false,
        );
        if result != Err("Text/font glyph bounds are not known from the FX text box") {
            return result;
        }
        // Prune only Text whose bounds are still unknown. Verified Point Text
        // must survive an unrelated unbounded Text sibling. The projection is
        // analysis-only: restore original Source Text before ordinary lowering.
        let font_projection = self
            .fonts
            .filter(|_| !hierarchy::has_skew(group))
            .and_then(|fonts| fonts.bounds_geometry(group, self.dynamics).ok());
        let Some((retained, omitted_branches, omitted_descendants)) =
            prune_independent_text_branches(
                font_projection.as_ref().unwrap_or(group),
                &self.source_variant_eligibility,
            )
        else {
            return result;
        };
        let retained = restore_projected_text_content(group, &retained)?;
        self.diagnostics.truncate(diagnostic_count);
        self.occupied_ids.rollback(occupied_ids);
        self.consumed_guides.rollback(consumed_guides);
        self.omitted_layer_ids = omitted_ids;
        self.lower_group_original(
            &retained,
            options,
            parent_siblings,
            depth,
            output_visible,
            demand,
            false,
        )?;
        self.omitted_layer_ids.extend(omitted_descendants);
        for branch in omitted_branches {
            self.warn(Some(branch), "Text-only branch omitted from mixed native precomposition because its FX glyph bounds are unknown; independent visual siblings retained. Text content, its rendering, and any unsupported CustomShader effects on this branch are lost.");
        }
        Ok(())
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "Twirl carrier separates only its owner opacity from existing hierarchy lowering"
    )]
    fn lower_group_original(
        &mut self,
        group: &GroupLayer,
        mut options: NativeLayerOptions,
        parent_siblings: &[Layer],
        depth: usize,
        output_visible: bool,
        demand: &hierarchy::Demand,
        opacity_on_carrier: bool,
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
        let certified_media_domains = if self.inside_precomposition
            && let Some((reachable, domains)) =
                hierarchy_clock::finite_media_remap_view(&geometry, self.end)
        {
            geometry = reachable;
            Some(domains)
        } else {
            None
        };
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
                    // Nested static masks need no effect substitution. A native
                    // Wipe on a transformed or differently sized source plane
                    // cannot currently be imported back as editable clipping.
                    // Keep its exact PathMask on the existing native carrier.
                    if depth > 0 && candidate.angle_track.is_none() {
                        return false;
                    }
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
        let hierarchy_dynamics = if options.transform_3d.is_some() || opacity_on_carrier {
            planar_dynamics = self
                .dynamics
                .iter()
                .filter(|entry| {
                    !entry.target.as_property().is_some_and(|target| {
                        target.layer_id() == group.id
                            && ((options.transform_3d.is_some()
                                && is_transform_property(target.property_type()))
                                || (opacity_on_carrier
                                    && target.property_type() == PropType::Opacity))
                    })
                })
                .cloned()
                .collect::<Vec<_>>();
            Some(AnimationIndex::new(&planar_dynamics))
        } else {
            None
        };
        let hierarchy_dynamics = hierarchy_dynamics.as_ref().unwrap_or(self.dynamics);
        let source_mask_bounds = source_rect_mask_bounds(group, self.dynamics, self.dimensions)
            .or_else(|| logical_bulge_source_bounds(group, self.dynamics, self.logical_dimensions))
            .or_else(|| {
                (!options
                    .transform_3d
                    .as_ref()
                    .is_some_and(|(transform, _)| transform.is_three_d))
                .then(|| logical_corner_pin_source_bounds(group, self.logical_dimensions))
                .flatten()
            });
        if source_mask_bounds.is_some() {
            hierarchy::validate_masked_source(
                group,
                self.dynamics,
                self.resolved_media,
                self.dimensions,
            )?;
        }
        let masks = std::mem::take(&mut geometry.masks);
        let static_rect_mask_crop = !group.motion_blur
            && masks::static_rect_add_crop_certificate(
                &masks,
                masks::MaskOwner {
                    coordinate_owner: Some(group.id),
                    parent: group.parent,
                    transform: &group.transform,
                    source_size: [self.dimensions.width, self.dimensions.height],
                    clock: playback_is_identity(&group.playback)
                        .then_some(playback_active_range(&group.playback)),
                },
                parent_siblings,
                self.dynamics,
            );
        // A direct-child Path lives on this source's local zero clock, not
        // the delayed occurrence's input window. Admit its viewport only after
        // the ordinary native mask lowerer has proved the exact copy.
        let source_path_mask_crop = depth == 0
            && options.transform_3d.is_none()
            && !group.motion_blur
            && source_path_add_mask_crop_certificate(
                group,
                &masks,
                parent_siblings,
                &geometry.layers,
                [self.dimensions.width, self.dimensions.height],
                self.dynamics,
            );
        let viewport_group = source_path_mask_crop.then(|| {
            let mut viewport = group.clone();
            viewport.masks.clear();
            viewport
        });
        let mut child_demand = hierarchy::child_demand(
            group,
            &geometry,
            &masks,
            self.dynamics,
            self.dimensions,
            demand,
            static_rect_mask_crop || source_path_mask_crop,
        );
        let root_output_viewport = depth == 0
            && options.transform_3d.is_none()
            && child_demand.use_root_output_viewport(
                viewport_group.as_ref().unwrap_or(group),
                self.dynamics,
                parent_siblings,
                self.dimensions,
            );
        let nested_output_viewport = depth > 0
            && options.transform_3d.is_none()
            && child_demand.use_nested_output_viewport(
                group,
                self.dynamics,
                parent_siblings,
                self.dimensions,
            );
        let nested_input_viewport = depth > 0
            && options.transform_3d.is_none()
            && !nested_output_viewport
            && child_demand.use_nested_input_viewport(
                group,
                self.dynamics,
                parent_siblings,
                self.dimensions,
            );
        let spatial_occurrence = options
            .transform_3d
            .as_ref()
            .is_some_and(|(transform, _)| transform.is_three_d);
        if !root_output_viewport
            && !nested_output_viewport
            && !nested_input_viewport
            && !spatial_occurrence
        {
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
        let mut clock = if clocked {
            Some(
                hierarchy_clock::plan(
                    &geometry,
                    self.dynamics,
                    certified_media_domains.map_or_else(|| group_clock_domains(&geometry), Ok)?,
                    hierarchy::audio_only(&geometry.layers),
                )
                .map_err(|_| "Group source clock cannot be represented exactly")?,
            )
        } else {
            None
        };
        if let Some(clock) = &clock {
            self.validate_source_clock(group.id, &clock.occurrence_clock)?;
            child_demand.use_checked_source_domain(hierarchy::checked_source_demand(
                group,
                &masks,
                self.dynamics,
                self.dimensions,
                demand,
                static_rect_mask_crop || source_path_mask_crop,
                clock.source_duration_millis,
            ));
        }
        if let Some(bounds) = source_mask_bounds {
            child_demand.use_source_mask_viewport(
                bounds,
                clock
                    .as_ref()
                    .map_or(group.playback.input_range().duration.as_millis(), |clock| {
                        clock.source_duration_millis
                    }),
            );
        }
        // Shape the actual source-local classifier input, after its clock has
        // been checked. This copy supplies bounds only, never emitted content or
        // a crop certificate. Nested occurrences use the same physical outlines.
        let font_source = clock.as_ref().map_or(&geometry, |clock| clock.geometry());
        let font_geometry = if skew_helper_id.is_none()
            // The static-shadow exception encloses the source plane before
            // applying the occurrence's existing native 3D transform. It does
            // not certify projected child Text or bypass near-plane checks.
            && (!spatial_occurrence || hierarchy::has_active_shadow(&group.effects))
            && masks.is_empty()
            && radial.is_none()
            && hierarchy::static_shadow_stack(&group.effects, self.dynamics)
            && font_source.layers.iter().any(group_descendant_has_text)
        {
            self.fonts.and_then(|fonts| {
                // Unsupported font/layout keeps the existing unknown-bounds
                // diagnostic and certified-viewport fallback, not guessed bounds.
                fonts.bounds_geometry(font_source, self.dynamics).ok()
            })
        } else {
            None
        };
        // The certificate is an output boundary, not a guessed Text enclosure.
        // Validate every other child using a reference-closed bounds-only copy;
        // the original geometry below remains the source of emitted content.
        let text_bounds_projection =
            if root_output_viewport && radial.is_none() && !source_path_mask_crop {
                clock.as_ref().and_then(|clock| {
                    root_plain_text_bounds_projection(
                        clock.geometry(),
                        Time::from_millis(clock.source_duration_millis),
                        self.dynamics,
                        &self.source_variant_eligibility,
                    )
                })
            } else {
                None
            };
        // Unknown Text glyph bounds must not discard an otherwise representable
        // clocked root scene. Keep the original geometry when no other visual
        // content survives, and never guess a smaller native source canvas.
        // root_output_viewport comes from root_viewport::canvas, which requires
        // the root owner Transform to be identity and unanimated. Only then are
        // direct child bounds already expressed in final output coordinates.
        let omitted_plain_text = if root_output_viewport
            && clocked
            && radial.is_none()
            && !source_path_mask_crop
            && text_bounds_projection.is_none()
            && font_geometry.is_none()
        {
            let (children, omitted) = omit_plain_clocked_root_text(
                &geometry.layers,
                self.dynamics,
                &self.source_variant_eligibility,
            )
            .map_err(|_| "Clocked root Text omission could not preserve editable children")?;
            if !omitted.is_empty()
                && children.iter().any(|child| {
                    has_visual_descendant(
                        child,
                        0,
                        clock
                            .as_ref()
                            .map_or(0, |clock| clock.source_duration_millis),
                        self.dynamics,
                        self.dimensions,
                    )
                })
            {
                geometry.layers = children;
                clock = Some(
                    hierarchy_clock::plan(
                        &geometry,
                        self.dynamics,
                        certified_media_domains
                            .map_or_else(|| group_clock_domains(&geometry), Ok)?,
                        hierarchy::audio_only(&geometry.layers),
                    )
                    .map_err(|_| "Group source clock cannot be represented exactly")?,
                );
                if let Some(clock) = &clock {
                    self.validate_source_clock(group.id, &clock.occurrence_clock)?;
                }
                omitted
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };
        if root_output_viewport && let Some(clock) = &clock {
            child_demand.use_clocked_root_output_viewport(clock.source_duration_millis);
        }
        // Separated planar X/Y keys use the Transform sidecar too; they have
        // the same bounded inverse demand as ordinary 2D occurrences.
        let text_canvas = if let Some(bounds) = source_mask_bounds {
            Ok(bounds)
        } else if !spatial_occurrence {
            child_demand.text_canvas(group, parent_siblings)
        } else {
            Err("3D Text consumer has no certified planar preimage")
        };
        let text_canvas = text_canvas.ok();
        let mut approximate_viewport = false;
        let classification = (|| -> Result<hierarchy::HierarchyPlan, &'static str> {
            let classification_geometry = font_geometry.as_ref().unwrap_or(&geometry);
            Ok(if let Some(helper_id) = skew_helper_id {
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
                    text_canvas,
                    helper_id,
                )?
            } else if let Some(clock) = &clock {
                hierarchy::classify_precomposition_with_demand(
                    font_geometry
                        .as_ref()
                        .or(text_bounds_projection.as_ref())
                        .unwrap_or(clock.geometry()),
                    Time::from_millis(clock.source_duration_millis),
                    clock.source_duration,
                    hierarchy_dynamics,
                    self.resolved_media,
                    self.dimensions,
                    child_demand.finite_canvas(),
                    text_canvas,
                    clock
                        .visible_source_interval
                        .and_then(|range| hierarchy::SourceInterval::new(range, self.rate).ok()),
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
                    classification_geometry,
                    self.end,
                    self.duration,
                    hierarchy_dynamics,
                    self.resolved_media,
                    self.dimensions,
                    child_demand.finite_canvas(),
                    text_canvas,
                )?
            } else {
                match hierarchy::classify_precomposition_with_demand(
                    classification_geometry,
                    self.end,
                    self.duration,
                    hierarchy_dynamics,
                    self.resolved_media,
                    self.dimensions,
                    child_demand.finite_canvas(),
                    text_canvas,
                    None,
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
                            text_canvas,
                            None,
                        )?
                    }
                    Err(reason) => return Err(reason),
                }
            })
        })();
        let mut plan = match classification {
            Ok(plan) => plan,
            Err(hierarchy::NATIVE_CANVAS_OVERFLOW)
                if skew_helper_id.is_none()
                    && radial.is_none()
                    && original_geometry.is_none()
                    && !spatial_occurrence
                    && masks.is_empty()
                    && !self
                        .source_variant_eligibility
                        .get(&group.id)
                        .is_some_and(|facts| {
                            facts.referenced_as_matte
                                || facts.referenced_as_mask_guide
                                || facts.referenced_as_text_guide
                                || facts.referenced_as_ai_edit_source
                                || facts.referenced_as_segment
                                || facts.referenced_by_animation_layer_ref
                                || facts.has_unsupported_graph_edges()
                        }) =>
            {
                let candidate = hierarchy::approximate_child_demand(
                    group,
                    self.dynamics,
                    self.dimensions,
                    demand,
                    parent_siblings,
                    clock.as_ref().map(|clock| clock.source_duration_millis),
                )
                .ok_or(hierarchy::NATIVE_CANVAS_OVERFLOW)?;
                let (source_geometry, source_end, source_duration, source_interval) =
                    clock.as_ref().map_or(
                        (
                            font_geometry.as_ref().unwrap_or(&geometry),
                            self.end,
                            self.duration,
                            None,
                        ),
                        |clock| {
                            (
                                font_geometry.as_ref().unwrap_or(clock.geometry()),
                                Time::from_millis(clock.source_duration_millis),
                                clock.source_duration,
                                clock.visible_source_interval.and_then(|range| {
                                    hierarchy::SourceInterval::new(range, self.rate).ok()
                                }),
                            )
                        },
                    );
                let rescued = hierarchy::classify_precomposition_with_demand(
                    source_geometry,
                    source_end,
                    source_duration,
                    hierarchy_dynamics,
                    self.resolved_media,
                    self.dimensions,
                    candidate.finite_canvas(),
                    None,
                    source_interval,
                )?;
                child_demand = candidate;
                approximate_viewport = true;
                self.warn(Some(group.id), "Oversized source viewport approximation: finite output-derived working plane retains editable native content; offscreen blur, motion-blur and source-sampling boundary contributions may differ. Native pixel equivalence is unverified.");
                rescued
            }
            Err(reason) => return Err(reason),
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
                text_canvas,
                None,
            )?;
        }

        if font_geometry.is_some()
            && matches!(&plan, hierarchy::HierarchyPlan::Precomposition(plan) if plan.collapsed_source().is_some())
        {
            return Err("Font-derived bounds cannot certify collapsed vector-only content");
        }
        if font_geometry.is_some() {
            self.warn(Some(group.id), "Finite source Text bounds use outlines shaped from the hash-verified embedded physical font and the emitted native Source Text timeline; original editable Text and composite opacity/motion blur are retained. Unsupported document tracks keep their existing diagnostics/static base; whitespace contributes no painted bounds. Native font substitution is not certified by these bounds.");
        }
        if matches!(&plan, hierarchy::HierarchyPlan::Precomposition(plan) if plan.consumer_viewport())
        {
            self.warn(Some(group.id), "Oversized 3D source uses a finite consumer output viewport with unchanged world geometry and camera; native raster edge/alpha differences are an approximation, not pixel-exact fidelity.");
        }
        match &plan {
            hierarchy::HierarchyPlan::Precomposition(precomposition) => {
                match precomposition.collapsed_source() {
                    Some(hierarchy::CollapsedSource::OversizedVector) => {
                        // A short canonical identity occurrence still needs a
                        // checked source/visibility plan. That plan does not
                        // retime its vectors; only nonidentity playback does.
                        if options.transform_3d.is_some() || !playback_is_identity(&group.playback)
                        {
                            return Err(
                                "Collapsed vector source requires a 2D identity-clock occurrence",
                            );
                        }
                        self.warn(Some(group.id), "Oversized 2D vector source retains its geometry using native collapse transformations; the source canvas is not an input crop. Non-vector and 3D subtrees are excluded.");
                    }
                    // A source clock is the usual reason Text must precompose.
                    // Masks and matte sampling would rasterize the collapsed layer.
                    Some(hierarchy::CollapsedSource::Text) => {
                        if options
                            .transform_3d
                            .as_ref()
                            .is_some_and(|(transform, _)| transform.is_three_d)
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
            hierarchy::HierarchyPlan::Parent(_) => {
                if !masks.is_empty()
                    || self
                        .source_variant_eligibility
                        .get(&group.id)
                        .is_some_and(|facts| facts.referenced_as_matte)
                {
                    return Err(
                        "Native Null controls cannot supply masks or composite matte alpha",
                    );
                }
                if clock
                    .as_ref()
                    .is_some_and(|clock| clock.occurrence_clock.has_time_remap())
                {
                    return Err(
                        "Native Null controls require an affine occurrence clock; Time Remap is unsupported",
                    );
                }
            }
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
        if nested_input_viewport {
            self.warn(Some(group.id), "Nested identity 2D source uses validated content intersected with its consumer preimage plus defined effect-kernel support; geometry, effects, matte and source clocks remain authored. RGB window acceptance is separate from general edge/alpha fidelity.");
        }
        if nested_output_viewport {
            self.warn(Some(group.id), "Nested identity 2D Group uses its exact propagated final-output viewport; authored child geometry, source clocks and effect inputs are retained. Unknown, transformed or externally referenced consumer domains remain unsupported; general native edge/alpha fidelity is unverified.");
        }

        if !group.effects.is_empty() {
            let hierarchy::HierarchyPlan::Precomposition(precomposition) = &plan else {
                return Err("Group effects require a native precomposition occurrence");
            };
            let (source_size, source_origin) = precomposition.mask_space();
            let (mut effects, styles) = self.lower_effect_stack(
                group.id,
                &group.effects,
                self.dynamics,
                source_size.map(f64::from),
                None,
            );
            normalize_group_bulge_frame(
                &mut effects,
                &group.effects,
                self.dynamics,
                self.logical_dimensions,
                source_origin,
            );
            if approximate_viewport {
                normalize_approximate_group_radial_frame(
                    &mut effects,
                    &group.effects,
                    self.dynamics,
                    self.logical_dimensions,
                    source_origin,
                    self.rate,
                );
            }
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
        for id in omitted_plain_text {
            self.warn(Some(id), "Plain Text omitted from clocked root: FX has no glyph bounds for this mixed precomposition; the native Text profile is unverified. Other eligible visual siblings are retained; the text and its motion are absent.");
            self.omitted_layer_ids.insert(id);
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

fn group_descendant_has_text(layer: &Layer) -> bool {
    match layer.data() {
        LayerData::Text(_) => true,
        LayerData::Group(group) => group.layers.iter().any(group_descendant_has_text),
        _ => false,
    }
}

// Only static plain Text beneath full-span Null-compatible ancestors may be
// absent from bounds analysis. Effects, masks and spatial inputs must retain
// their ordinary enclosure checks; no boxSize is used as a glyph rectangle.
fn root_plain_text_bounds_layer_is_safe(
    layer: &Layer,
    end: Time,
    dynamics: &AnimationIndex<'_>,
) -> bool {
    match layer.data() {
        LayerData::Text(text) => {
            text.effects.is_empty()
                && text.masks.is_empty()
                && text.track_matte.is_none()
                && !text.motion_blur
                && text.blend_mode == Default::default()
                && text.animators.is_empty()
                && text.path_options.is_none()
                && text.anchor_options.is_none()
                && !has_entries_for(dynamics, text.id)
                && !transform3d::requires_native_3d(dynamics, &text.transform, text.id)
        }
        LayerData::Group(group) if group.layers.iter().any(group_descendant_has_text) => {
            group.effects.is_empty()
                && group.masks.is_empty()
                && group.track_matte.is_none()
                && !group.motion_blur
                && group.blend_mode == Default::default()
                && group.transform.opacity.value() == 100.0
                && group_has_root_identity_clock(group, end)
                && !has_entries_for(dynamics, group.id)
                && !transform3d::requires_native_3d(dynamics, &group.transform, group.id)
                && group
                    .layers
                    .iter()
                    .all(|child| root_plain_text_bounds_layer_is_safe(child, end, dynamics))
        }
        _ => true,
    }
}

fn root_plain_text_bounds_projection(
    group: &GroupLayer,
    source_end: Time,
    dynamics: &AnimationIndex<'_>,
    references: &BTreeMap<LayerId, source_variants::SourceVariantEligibility>,
) -> Option<GroupLayer> {
    if !group
        .layers
        .iter()
        .all(|child| root_plain_text_bounds_layer_is_safe(child, source_end, dynamics))
    {
        return None;
    }
    prune_independent_text_branches(group, references).map(|(geometry, _, _)| geometry)
}

// A font projection may replace Text with classifier Rects. Recover the native
// source content by identity after pruning; never emit those bounds proxies.
fn restore_projected_text_content(
    original: &GroupLayer,
    retained: &GroupLayer,
) -> Result<GroupLayer, &'static str> {
    let mut restored = original.clone();
    restored.layers = retained
        .layers
        .iter()
        .map(|layer| {
            let source = original
                .layers
                .iter()
                .find(|source| source.id() == layer.id())
                .ok_or("Text bounds projection lost its source identity")?;
            if let (LayerData::Group(source), LayerData::Group(retained)) =
                (source.data(), layer.data())
            {
                let restored = restore_projected_text_content(source, retained)?;
                Layer::from_data(&LayerData::Group(restored))
                    .map_err(|_| "Text bounds recovery could not preserve editable children")
            } else {
                Ok(source.clone())
            }
        })
        .collect::<Result<_, _>>()?;
    Ok(restored)
}

// Omission callers use this copy only after a failed glyph-bounds proof and
// restore original content before emission. The certified-root caller uses it
// for bounds only; neither path guesses glyph extents.
fn prune_independent_text_branches(
    group: &GroupLayer,
    references: &BTreeMap<LayerId, source_variants::SourceVariantEligibility>,
) -> Option<(GroupLayer, Vec<LayerId>, BTreeSet<LayerId>)> {
    fn unsupported_custom_shader(record: &fx_schema::EffectRecord) -> bool {
        let payload = match record.data() {
            EffectData::Identified { effect, .. } | EffectData::Legacy(effect) => effect,
        };
        matches!(payload, EffectPayload::Known(effect)
            if crate::effects::catalog::effect_type(effect) == "customShader"
                && effects::unmapped_warning(record).is_some())
    }

    fn safe_group(group: &GroupLayer, dropping_text_branch: bool) -> bool {
        !group.is_hidden
            && group.blend_mode == Default::default()
            && group.track_matte.is_none()
            && group.masks.is_empty()
            // Never discard an effect on a retained group. An unsupported
            // CustomShader can go only with its entire independent Text branch.
            && (group.effects.is_empty()
                || (dropping_text_branch
                    && group.effects.iter().all(unsupported_custom_shader)))
            && !group.motion_blur
            && group.fills.is_empty()
            && group_layout_is_empty(group)
    }

    fn safe_text_branch(layer: &Layer) -> bool {
        match layer.data() {
            LayerData::Text(_) => true,
            LayerData::Group(group) => {
                safe_group(group, true) && group.layers.iter().all(safe_text_branch)
            }
            _ => false,
        }
    }

    fn prune(
        group: &mut GroupLayer,
        references: &BTreeMap<LayerId, source_variants::SourceVariantEligibility>,
        branches: &mut Vec<LayerId>,
        descendants: &mut BTreeSet<LayerId>,
    ) -> Option<()> {
        let owner = references.get(&group.id)?;
        if !safe_group(group, false)
            || owner.referenced_as_matte
            || owner.referenced_as_mask_guide
            || owner.referenced_as_text_guide
            || owner.referenced_as_ai_edit_source
            || owner.referenced_as_segment
            || owner.referenced_by_animation_dependency
            || owner.referenced_by_animation_layer_ref
            || owner.has_unresolved_animation_dependency
        {
            return None;
        }
        let mut retained = Vec::with_capacity(group.layers.len());
        for child in &group.layers {
            if hierarchy::text_only_branch(child) {
                if !safe_text_branch(child) {
                    return None;
                }
                let mut ids = BTreeSet::new();
                collect_source_layer_ids(std::slice::from_ref(child), &mut ids);
                for id in &ids {
                    let facts = references.get(id)?;
                    if facts.referenced_as_parent
                        || facts.referenced_as_matte
                        || facts.referenced_as_mask_guide
                        || facts.referenced_as_text_guide
                        || facts.referenced_as_ai_edit_source
                        || facts.referenced_as_segment
                        || facts.referenced_by_animation_dependency
                        || facts.referenced_by_animation_layer_ref
                        || facts.has_unresolved_animation_dependency
                        || facts.owns_masks_or_matte
                    {
                        return None;
                    }
                }
                descendants.extend(ids);
                branches.push(child.id());
                continue;
            }
            if let LayerData::Group(child_group) = child.data()
                && child_group.layers.iter().any(group_descendant_has_text)
            {
                let mut copy = child_group.clone();
                prune(&mut copy, references, branches, descendants)?;
                if copy != *child_group {
                    retained.push(Layer::from_data(&LayerData::Group(copy)).ok()?);
                    continue;
                }
            }
            retained.push(child.clone());
        }
        group.layers = retained;
        Some(())
    }

    let mut retained = group.clone();
    let mut branches = Vec::new();
    let mut descendants = BTreeSet::new();
    prune(&mut retained, references, &mut branches, &mut descendants)?;
    (!branches.is_empty() && !retained.layers.is_empty()).then_some((
        retained,
        branches,
        descendants,
    ))
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

fn has_visual_descendant(
    layer: &Layer,
    source_start_ms: u64,
    source_end_ms: u64,
    dynamics: &AnimationIndex<'_>,
    canvas: fx_schema::Dimensions,
) -> bool {
    let LayerData::Rect(rect) = layer.data() else {
        // Nested transforms and generated Shape bounds are not certified for
        // this destructive fallback. Retain Text rather than infer visibility.
        return false;
    };
    let active = layer.active_range();
    if layer_is_hidden(layer)
        || active.start.as_millis() > source_start_ms
        || active.end().as_millis() < source_end_ms
        || source_start_ms >= source_end_ms
        || dynamics.for_layer(layer.id()).next().is_some()
        || !rect.masks.is_empty()
        || rect.track_matte.is_some()
        || !rect.effects.is_empty()
        || rect.motion_blur
        || rect.blend_mode != Default::default()
        || rect.rect.stroke_enabled
        || rect.rect.roundness != 0.0
        || rect.rect.fill_blend_mode.unwrap_or_default() != Default::default()
        || !rect.rect.fill_enabled
        || rect.rect.size.iter().any(|size| *size <= 0.0)
        || rect.transform.rotation != 0.0
        || rect.transform.skew != 0.0
        || rect.transform.opacity.value() <= 0.0
    {
        return false;
    }
    let painted = rect.rect.fill_paint.as_ref().map_or_else(
        || rect.rect.fill_color[3] > 0.0,
        |paint| matches!(paint, ShapePaint::Solid { color } if color[3] > 0.0),
    );
    if !painted {
        return false;
    }
    let Ok(Some(bounds)) =
        hierarchy::all_time_layer_bounds(layer, dynamics, &BTreeMap::new(), canvas)
    else {
        return false;
    };
    [f64::from(canvas.width), f64::from(canvas.height)]
        .into_iter()
        .enumerate()
        .all(|(axis, size)| {
            bounds.min[axis] < size && bounds.max[axis] > 0.0 && bounds.min[axis] < bounds.max[axis]
        })
}

fn omit_plain_clocked_root_text(
    layers: &[Layer],
    dynamics: &AnimationIndex<'_>,
    eligibility: &BTreeMap<LayerId, source_variants::SourceVariantEligibility>,
) -> Result<(Vec<Layer>, Vec<LayerId>), serde_json::Error> {
    let mut retained = Vec::with_capacity(layers.len());
    let mut omitted = Vec::new();
    for layer in layers {
        match layer.data() {
            LayerData::Text(text)
                if text.effects.is_empty()
                    && text.masks.is_empty()
                    && text.track_matte.is_none()
                    && text.animators.is_empty()
                    && text.path_options.is_none()
                    && !text.motion_blur
                    && text.blend_mode == Default::default()
                    && !has_entries_for(dynamics, text.id)
                    && eligibility.get(&text.id).is_some_and(|facts| {
                        !facts.owns_masks_or_matte
                            && !facts.referenced_as_parent
                            && !facts.referenced_as_matte
                            && !facts.referenced_as_mask_guide
                            && !facts.referenced_as_text_guide
                            && !facts.referenced_as_ai_edit_source
                            && !facts.referenced_as_segment
                            && !facts.referenced_by_animation_dependency
                            && !facts.referenced_by_animation_layer_ref
                            && !facts.has_unresolved_animation_dependency
                    }) =>
            {
                omitted.push(text.id);
            }
            LayerData::Group(group)
                if group.effects.is_empty()
                    && group.masks.is_empty()
                    && group.track_matte.is_none()
                    && group.fills.is_empty()
                    && group.blend_mode == Default::default()
                    && !group.motion_blur
                    && group.transform.opacity.value() == 100.0
                    && group_layout_is_empty(group)
                    && eligibility.get(&group.id).is_some_and(|facts| {
                        !facts.referenced_as_matte
                            && !facts.referenced_as_mask_guide
                            && !facts.referenced_as_text_guide
                            && !facts.referenced_as_ai_edit_source
                            && !facts.referenced_as_segment
                            && !facts.referenced_by_animation_dependency
                            && !facts.referenced_by_animation_layer_ref
                            && !facts.has_unresolved_animation_dependency
                    }) =>
            {
                let (children, child_omissions) =
                    omit_plain_clocked_root_text(&group.layers, dynamics, eligibility)?;
                if child_omissions.is_empty() {
                    retained.push(layer.clone());
                } else {
                    let mut kept_group = group.clone();
                    kept_group.layers = children;
                    retained.push(Layer::from_data(&LayerData::Group(kept_group))?);
                    omitted.extend(child_omissions);
                }
            }
            _ => retained.push(layer.clone()),
        }
    }
    Ok((retained, omitted))
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
    dynamics: &AnimationIndex<'_>,
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
    dynamics: &AnimationIndex<'_>,
) -> masks::LoweredMasks {
    let identity = identity_fx_transform();
    let mut output = masks::LoweredMasks::default();
    for (index, mask) in group_masks.iter().enumerate() {
        let direct_child = mask
            .layer
            .and_then(|id| direct_children.iter().find(|layer| layer.id() == id));
        let source_clock = direct_child.and_then(|guide| {
            group_mask_source_clock(group).filter(|clock| guide.active_range() == *clock)
        });
        let (parent, transform, siblings) = if let Some(guide) = direct_child {
            // Implicit child parents are local to this source stack. An
            // explicit foreign parent remains different from the owner here.
            (
                guide.parent_id().filter(|parent| *parent == group.id),
                &identity,
                direct_children,
            )
        } else {
            (group.parent, &group.transform, parent_siblings)
        };
        let mut lowered = masks::lower(
            std::slice::from_ref(mask),
            None,
            masks::MaskOwner {
                coordinate_owner: if source_clock.is_some()
                    && direct_child.is_some_and(|guide| {
                        guide.parent_id().is_none_or(|parent| parent == group.id)
                    }) {
                    None
                } else {
                    Some(group.id)
                },
                parent,
                transform,
                source_size,
                clock: source_clock.or_else(|| {
                    playback_is_identity(&group.playback)
                        .then_some(playback_active_range(&group.playback))
                }),
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

// Only an exact unit-rate, offset-free Group source has this local phase.
fn group_mask_source_clock(group: &GroupLayer) -> Option<fx_schema::TimeRangeProperty> {
    let active = group.playback.input_range();
    let exact_source_zero_shift = matches!(
        group.playback.mapping(),
        fx_schema::LayerPlaybackMapping::Linear { input, output }
            if *input == active && output.start == Time::ZERO
                && input.duration == output.duration
    );
    (group.playback.input_offset_ms() == 0
        && (playback_is_identity(&group.playback) || exact_source_zero_shift))
        .then(|| fx_schema::TimeRangeProperty::new(Time::ZERO, active.duration))
}

/// FX Group Radial Blur centers use the logical composition point domain.
/// A rescued source translates content by -origin; translate its centers too,
/// rather than retargeting UV controls to the smaller native canvas. Ordinary
/// sources and all non-Radial controls retain their existing lowering path.
fn normalize_approximate_group_radial_frame(
    native: &mut [crate::writer::effects::NativeEffect],
    records: &[fx_schema::EffectRecord],
    dynamics: &AnimationIndex<'_>,
    logical: fx_schema::Dimensions,
    origin: [f64; 2],
    rate: crate::timing::FrameRate,
) {
    let logical_effects = effects::lower_at_rate(
        records,
        dynamics,
        [f64::from(logical.width), f64::from(logical.height)],
        rate,
    );
    let mut replacements = logical_effects
        .effects
        .into_iter()
        .filter(|effect| effect.match_name == "ADBE Radial Blur");
    for effect in native
        .iter_mut()
        .filter(|effect| effect.match_name == "ADBE Radial Blur")
    {
        let Some(mut replacement) = replacements.next() else {
            break;
        };
        if let Some(center) = replacement
            .properties
            .iter_mut()
            .find(|property| property.match_name == "ADBE Radial Blur-0002")
        {
            for (value, offset) in center.values.iter_mut().zip(origin) {
                *value -= offset;
            }
            if let Some(track) = &mut center.animation {
                for key in &mut track.keys {
                    for (value, offset) in key.values.iter_mut().zip(origin) {
                        *value -= offset;
                    }
                }
            }
        }
        *effect = replacement;
    }
}

/// Finite source alpha support, proved from the actual native mask copy rather than glyphs.
/// Group geometry effects use the root document plane, not a native capture tile.
/// Shifting an already-lowered Point's base/keys preserves its interpolation;
/// spatial tangents are vectors and do not change under this translation.
fn normalize_group_bulge_frame(
    native: &mut [crate::writer::effects::NativeEffect],
    records: &[fx_schema::EffectRecord],
    dynamics: &AnimationIndex<'_>,
    logical: fx_schema::Dimensions,
    origin: [f64; 2],
) {
    let logical_effects = effects::lower(
        records,
        dynamics,
        [f64::from(logical.width), f64::from(logical.height)],
    );
    let mut replacements = logical_effects
        .effects
        .into_iter()
        .filter(|effect| effect.match_name == "ADBE Bulge");
    for effect in native
        .iter_mut()
        .filter(|effect| effect.match_name == "ADBE Bulge")
    {
        let Some(mut replacement) = replacements.next() else {
            break;
        };
        if let Some(center) = replacement
            .properties
            .iter_mut()
            .find(|property| property.match_name == "ADBE Bulge-0003")
        {
            for (value, offset) in center.values.iter_mut().zip(origin) {
                *value -= offset;
            }
            if let Some(track) = &mut center.animation {
                for key in &mut track.keys {
                    for (value, offset) in key.values.iter_mut().zip(origin) {
                        *value -= offset;
                    }
                }
            }
        }
        *effect = replacement;
    }
}

// Group Corner Pin uses the logical composition plane, not the union of child
// bounds. Keep the native source viewport in that same plane before mapping points.
fn logical_corner_pin_source_bounds(
    group: &GroupLayer,
    logical: fx_schema::Dimensions,
) -> Option<hierarchy::Bounds> {
    if group.motion_blur
        || !group.fills.is_empty()
        || !group_layout_is_empty(group)
        || !group.masks.is_empty()
        || group.track_matte.is_some()
    {
        return None;
    }
    let first = group
        .effects
        .iter()
        .find_map(|record| match record.data() {
            EffectData::Identified { enabled: false, .. } => None,
            EffectData::Identified { effect, .. } | EffectData::Legacy(effect) => Some(effect),
        })?;
    if !matches!(first, EffectPayload::Known(LayerEffect::CornerPin { .. })) {
        return None;
    }
    Some(hierarchy::Bounds {
        min: [0.0; 2],
        max: [f64::from(logical.width), f64::from(logical.height)],
    })
}

/// The active, unpinned FX Bulge shader samples only the logical root plane:
/// its explicit edge_detect zeros destination samples outside [0,1). The native
/// counterpart can therefore keep that full input plane without glyph bounds.
/// Spatial effects before this clip still need their own input-support proof.
fn logical_bulge_source_bounds(
    group: &GroupLayer,
    dynamics: &AnimationIndex<'_>,
    logical: fx_schema::Dimensions,
) -> Option<hierarchy::Bounds> {
    static_logical_bulge(group, dynamics)?;
    Some(hierarchy::Bounds {
        min: [0.0; 2],
        max: [f64::from(logical.width), f64::from(logical.height)],
    })
}

/// Outside the native/FX Bulge ellipse the operator is identity. Its nonzero
/// output is enclosed by the logical input rectangle plus that ellipse's hull.
fn logical_bulge_output_bounds(
    group: &GroupLayer,
    dynamics: &AnimationIndex<'_>,
    logical: fx_schema::Dimensions,
) -> Option<hierarchy::Bounds> {
    let LayerEffect::Bulge {
        center_x,
        center_y,
        horizontal_radius,
        vertical_radius,
        ..
    } = static_logical_bulge(group, dynamics)?
    else {
        return None;
    };
    let size = [f64::from(logical.width), f64::from(logical.height)];
    let center = [center_x * size[0], center_y * size[1]];
    let radius = [horizontal_radius * size[0], vertical_radius * size[1]];
    let mut bounds = logical_bulge_source_bounds(group, dynamics, logical)?;
    bounds.include(hierarchy::Bounds {
        min: [center[0] - radius[0], center[1] - radius[1]],
        max: [center[0] + radius[0], center[1] + radius[1]],
    });
    bounds
        .min
        .iter()
        .chain(bounds.max.iter())
        .all(|value| value.is_finite())
        .then_some(bounds)
}

fn static_logical_bulge<'a>(
    group: &'a GroupLayer,
    dynamics: &AnimationIndex<'_>,
) -> Option<&'a LayerEffect> {
    if group.motion_blur
        || !group.fills.is_empty()
        || !group_layout_is_empty(group)
        || !group.masks.is_empty()
        || group.track_matte.is_some()
        || !masked_text_inputs_are_pointwise(&group.layers)
    {
        return None;
    }
    let mut bulge = None;
    for record in &group.effects {
        let (id, payload) = match record.data() {
            EffectData::Identified { enabled: false, .. } => continue,
            EffectData::Identified { id, effect, .. } => (Some(*id), effect),
            EffectData::Legacy(effect) => (None, effect),
        };
        if let EffectPayload::Known(
            effect @ LayerEffect::Bulge {
                center_x,
                center_y,
                horizontal_radius,
                vertical_radius,
                bulge_height,
                pinning,
            },
        ) = payload
        {
            if bulge.is_some() || *pinning || !bulge_height.is_finite() || *bulge_height <= 0.0 || *bulge_height > 1.0
                || !center_x.is_finite() || !center_y.is_finite()
                || !(0.0..=1.0).contains(center_x) || !(0.0..=1.0).contains(center_y)
                || !horizontal_radius.is_finite() || *horizontal_radius <= 0.0
                || !vertical_radius.is_finite() || *vertical_radius <= 0.0
                || dynamics.iter().any(|entry| matches!(&entry.target, fx_schema::PropertyTarget::EffectProperty(target) if Some(target.effect_id()) == id))
            { return None; }
            bulge = Some(effect);
        } else if hierarchy::pointwise_effect_stack(std::slice::from_ref(record)).is_err() {
            return None;
        }
    }
    bulge
}

fn source_rect_mask_bounds(
    group: &GroupLayer,
    dynamics: &AnimationIndex<'_>,
    canvas: fx_schema::Dimensions,
) -> Option<hierarchy::Bounds> {
    if group.motion_blur
        || !group.fills.is_empty()
        || !group_layout_is_empty(group)
        || group.track_matte.is_some()
        || hierarchy::pointwise_effect_stack(&group.effects).is_err()
    {
        return None;
    }
    let clock = group_mask_source_clock(group)?;
    let (mask, suffix) = group.masks.split_first()?;
    if mask.legacy_path.is_some() {
        // Keep already-known source canvases unchanged. This certificate
        // rescues unknown Text support, not ordinary finite vector geometry.
        return group
            .layers
            .iter()
            .any(group_descendant_has_text)
            .then(|| inline_add_mask_source_bounds(group, dynamics, canvas))
            .flatten();
    }
    // Subsequent Subtract/Intersect coverage can only reduce the first hard
    // Add mask's support. Every suffix must still be emitted, never omitted.
    for mask in suffix {
        let guide = group
            .layers
            .iter()
            .find(|guide| Some(guide.id()) == mask.layer)?;
        if !matches!(
            mask.mode,
            fx_schema::MaskMode::Subtract | fx_schema::MaskMode::Intersect
        ) || mask.legacy_path.is_some()
            || guide.active_range() != clock
            || guide.parent_id().is_some_and(|parent| parent != group.id)
        {
            return None;
        }
    }
    let guide = group
        .layers
        .iter()
        .find(|guide| Some(guide.id()) == mask.layer)?;
    if guide.active_range() != clock || guide.parent_id().is_some_and(|parent| parent != group.id) {
        return None;
    }
    if !matches!(guide.data(), LayerData::Rect(rect)
        if rect.rect.roundness == 0.0 && rect.rect.size.into_iter().all(|v| v.is_finite() && v > 0.0))
        || mask.legacy_path.is_some()
        || mask.mode != fx_schema::MaskMode::Add
        || mask.inverted
        || mask.feather != [0.0; 2]
        || mask.opacity.value() != 1.0
        || mask.expansion != 0.0
        || dynamics
            .iter()
            .any(|entry| entry.target.fx_item_id() == Some(mask.id))
    {
        return None;
    }
    let lowered = lower_group_masks(
        group,
        &group.masks,
        &[],
        &group.layers,
        [canvas.width, canvas.height],
        dynamics,
    );
    if lowered.masks.len() != group.masks.len()
        || suffix
            .iter()
            .zip(lowered.masks.iter().skip(1))
            .any(|(mask, native)| {
                !mask
                    .layer
                    .is_some_and(|id| lowered.consumed_guides.contains(&id))
                    || !matches!(
                        (mask.mode, native.mode),
                        (
                            fx_schema::MaskMode::Subtract,
                            crate::writer::NativeMaskMode::Subtract
                        ) | (
                            fx_schema::MaskMode::Intersect,
                            crate::writer::NativeMaskMode::Intersect
                        )
                    )
            })
    {
        return None;
    }
    let native = lowered.masks.first()?;
    if !matches!(native.mode, crate::writer::NativeMaskMode::Add)
        || native.inverted
        || native.feather != [0.0; 2]
        || native.opacity != 1.0
        || native.expansion != 0.0
        || native.feather_track.is_some()
        || native.opacity_track.is_some()
        || native.expansion_track.is_some()
        || !lowered.consumed_guides.contains(&guide.id())
    {
        return None;
    }
    let mut bounds = hierarchy::path_bounds(&native.path.commands).ok()?;
    if let Some(track) = &native.path_track {
        if track
            .keyframes
            .iter()
            .any(|key| key.easing != crate::writer::KeyframeEasing::Hold)
        {
            return None;
        }
        for key in &track.keyframes {
            bounds.include(hierarchy::path_bounds(&key.path.commands).ok()?);
        }
    }
    Some(bounds)
}

/// A static inline hard Add mask is already in source-local coordinates.
/// Its native control hull encloses masked alpha, independently of glyph bounds.
/// Unknown Text inputs may not have spatial effects before this mask: those
/// could pull pixels across the proposed source boundary before alpha is clipped.
fn inline_add_mask_source_bounds(
    group: &GroupLayer,
    dynamics: &AnimationIndex<'_>,
    canvas: fx_schema::Dimensions,
) -> Option<hierarchy::Bounds> {
    let [mask] = group.masks.as_slice() else {
        return None;
    };
    if mask.legacy_path.is_none()
        || mask.layer.is_some()
        || mask.mode != fx_schema::MaskMode::Add
        || mask.inverted
        || mask.feather != [0.0; 2]
        || mask.expansion != 0.0
        || !mask.opacity.value().is_finite()
        || mask.opacity.value().min(1.0) != 1.0
        || dynamics
            .iter()
            .any(|entry| entry.target.fx_item_id() == Some(mask.id))
        || !masked_text_inputs_are_pointwise(&group.layers)
    {
        return None;
    }
    let lowered = lower_group_masks(
        group,
        &group.masks,
        &[],
        &group.layers,
        [canvas.width, canvas.height],
        dynamics,
    );
    let [native] = lowered.masks.as_slice() else {
        return None;
    };
    if !matches!(native.mode, crate::writer::NativeMaskMode::Add)
        || native.inverted
        || native.feather != [0.0; 2]
        || native.opacity != 1.0
        || native.expansion != 0.0
        || native.path_track.is_some()
        || native.feather_track.is_some()
        || native.opacity_track.is_some()
        || native.expansion_track.is_some()
        || !lowered.consumed_guides.is_empty()
    {
        return None;
    }
    hierarchy::path_bounds(&native.path.commands).ok()
}

fn masked_text_inputs_are_pointwise(layers: &[Layer]) -> bool {
    layers.iter().all(|layer| match layer.data() {
        LayerData::Text(text) => hierarchy::pointwise_effect_stack(&text.effects).is_ok(),
        LayerData::Group(group) => {
            hierarchy::pointwise_effect_stack(&group.effects).is_ok()
                && masked_text_inputs_are_pointwise(&group.layers)
        }
        LayerData::Adjustment(adjustment) => {
            hierarchy::pointwise_effect_stack(&adjustment.effects).is_ok()
        }
        _ => true,
    })
}

fn source_path_add_mask_crop_certificate(
    group: &GroupLayer,
    group_masks: &[fx_schema::PathMask],
    parent_siblings: &[Layer],
    direct_children: &[Layer],
    source_size: [u32; 2],
    dynamics: &AnimationIndex<'_>,
) -> bool {
    let [mask] = group_masks else {
        return false;
    };
    let Some(clock) = group_mask_source_clock(group) else {
        return false;
    };
    let Some(guide) = mask
        .layer
        .and_then(|id| direct_children.iter().find(|layer| layer.id() == id))
    else {
        return false;
    };
    if !matches!(guide.data(), LayerData::Shape(_))
        || guide.active_range() != clock
        || mask.legacy_path.is_some()
        || mask.mode != fx_schema::MaskMode::Add
        || mask.inverted
        || mask.feather != [0.0; 2]
        || mask.opacity.value() != 1.0
        || mask.expansion != 0.0
        || dynamics
            .iter()
            .any(|entry| entry.target.fx_item_id() == Some(mask.id))
    {
        return false;
    }
    let lowered = lower_group_masks(
        group,
        group_masks,
        parent_siblings,
        direct_children,
        source_size,
        dynamics,
    );
    if lowered.masks.is_empty() {
        return false;
    }
    // Native mask alpha is pointwise in its source pixel domain. This does
    // not bound glyphs or the moving Path; it bounds only what the final
    // consumer can see, while retaining the authored native Path keys.
    lowered.masks.iter().all(|native| {
        matches!(native.mode, crate::writer::NativeMaskMode::Add)
            && !native.inverted
            && native.feather == [0.0; 2]
            && native.opacity == 1.0
            && native.expansion == 0.0
            && native.feather_track.is_none()
            && native.opacity_track.is_none()
            && native.expansion_track.is_none()
    }) && lowered.consumed_guides.len() == 1
        && lowered.consumed_guides.contains(&guide.id())
        && matches!(lowered.diagnostics.as_slice(), [message]
            if message.contains("was copied into same-layer native geometry")
                || message.contains("positive compound contours into native Add masks"))
}

// Only cross-layer masks and Text paths can consume source guide IDs. Inline
// masks (including generated media crops) cannot, so guide-free documents need
// no speculative native lowering/serialization before their real export pass.
fn has_source_guide_reference(layer: &Layer) -> bool {
    if mask_and_transform(layer).is_some_and(|(_, masks, text_path)| {
        text_path.is_some() || masks.iter().any(|mask| mask.layer.is_some())
    }) {
        return true;
    }
    match layer.data() {
        LayerData::Group(group) => group.layers.iter().any(has_source_guide_reference),
        _ => false,
    }
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

fn group_transform_is_animated(group: &GroupLayer, dynamics: &AnimationIndex<'_>) -> bool {
    has_transform_entries(dynamics, group.id)
        || group.layers.iter().any(|layer| {
            if let LayerData::Group(child) = layer.data() {
                group_transform_is_animated(child, dynamics)
            } else {
                false
            }
        })
}

/// Source-data policy: childless Rect/Shape with only opaque white fill and
/// no stroke is a shader canvas. Never infer this from a group/root's bounds.
fn shader_canvas_owner(layer: &Layer) -> bool {
    if !effects::has_custom_shader(layer.data().effects()) {
        return false;
    }
    match layer.data() {
        LayerData::Adjustment(_) => true,
        LayerData::Rect(rect) => {
            rect.rect.fill_enabled
                && !rect.rect.stroke_enabled
                && rect.rect.fill_paint.as_ref().map_or(
                    rect.rect.fill_color == [1.0; 4],
                    |paint| matches!(paint, ShapePaint::Solid { color } if *color == [1.0; 4]),
                )
        }
        LayerData::Shape(shape) => {
            shape.shape.strokes.is_empty()
                && shape.shape.fills.len() == 1
                && shape.shape.fills[0].opacity == 1.0
                && matches!(&shape.shape.fills[0].paint, ShapePaint::Solid { color } if *color == [1.0; 4])
        }
        _ => false,
    }
}

fn is_vector_hierarchy(layer: &Layer) -> bool {
    if shader_canvas_owner(layer) {
        // Omitted shader canvases must remain owner-local, not flattened paint.
        return false;
    }
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
    dynamics: &AnimationIndex<'_>,
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
    dynamics: &AnimationIndex<'_>,
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
                    modifier_animations(dynamics, shape.id, shape.shape.trim.is_some())?,
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
    dynamics: &AnimationIndex<'_>,
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
        && dynamics.for_layer(layer.id()).any(|entry| {
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
    dynamics: &AnimationIndex<'_>,
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
    dynamics: &AnimationIndex<'_>,
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

/// Shared NativeLayerOptions owns the admitted matte relation. Keep the strict
/// Image and legacy footage content lowerers independent of that relation
/// without clearing any other media visibility, compositing, caption or source
/// policy. Canonical Video keeps its matte: its lowerer admits only Alpha.
fn media_matte_content_view(layer: &Layer) -> Result<Layer, serde_json::Error> {
    let mut data = layer.data().clone();
    match &mut data {
        LayerData::Image(value) => value.track_matte = None,
        LayerData::Media(value) => value.track_matte = None,
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
    dynamics: &AnimationIndex<'_>,
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
    entries: &'a AnimationIndex<'_>,
    id: LayerId,
    base: &'a ShapePath,
) -> Result<&'a ShapePath, &'static str> {
    let Some(entry) = entries.first(fx_schema::property::Property::new(id, PropType::ShapePath))
    else {
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

fn has_entries_for(entries: &AnimationIndex<'_>, id: LayerId) -> bool {
    entries.for_layer(id).next().is_some()
}

fn has_transform_entries(entries: &AnimationIndex<'_>, id: LayerId) -> bool {
    entries.for_layer(id).any(|entry| {
        entry
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
    entries: &'a AnimationIndex<'_>,
    id: LayerId,
    property: PropType,
) -> Result<Option<NativeTrack<'a>>, &'static str> {
    let Some(entry) = entries.first(fx_schema::property::Property::new(id, property)) else {
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
    entries: &AnimationIndex<'_>,
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
    entries: &AnimationIndex<'_>,
    id: LayerId,
    include_trim: bool,
) -> Result<ModifierTracks, &'static str> {
    let scalar = |property| scalar_track(track(entries, id, property)?, 1.0);
    // FX drops scalar Trim writes when the Shape has no modifier. Parsing an
    // inert track must not reject the geometry or invent a native modifier.
    let trim = |property| {
        if include_trim {
            scalar(property)
        } else {
            Ok(None)
        }
    };
    Ok(ModifierTracks {
        round_corners: scalar(PropType::RoundCornersRadius)?,
        offset_paths: scalar(PropType::OffsetPathsAmount)?,
        trim_start: trim(PropType::TrimStart)?,
        trim_end: trim(PropType::TrimEnd)?,
        trim_offset: trim(PropType::TrimOffset)?,
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

fn has_stroke_animator(entries: &AnimationIndex<'_>, id: LayerId) -> bool {
    entries.for_layer(id).any(|entry| {
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
    entries: &AnimationIndex<'_>,
    id: LayerId,
) -> Result<StrokeAnimations, &'static str> {
    Ok(StrokeAnimations {
        width: scalar_track(track(entries, id, PropType::StrokeWidth)?, 1.0)?,
        miter_limit: scalar_track(track(entries, id, PropType::StrokeMiterLimit)?, 1.0)?,
        join: stroke_join_track(track(entries, id, PropType::StrokeJoin)?)?,
    })
}

fn transform_animations(
    entries: &AnimationIndex<'_>,
    id: LayerId,
    base: &Transform,
    content_id: LayerId,
) -> Result<TransformAnimations, &'static str> {
    transform_animations_partitioned(entries, id, base, content_id, false, false)
}

fn transform_animations_partitioned(
    entries: &AnimationIndex<'_>,
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
    if entries.for_layer(id).any(|entry| {
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
                        PropType::StrokeWidth | PropType::StrokeMiterLimit | PropType::StrokeJoin
                    ))
        })
    }) {
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
    entries: &AnimationIndex<'_>,
    id: LayerId,
    base: &SolidTransform,
    allow_audio: bool,
) -> Result<TransformAnimations, &'static str> {
    solid_transform_animations_partitioned(entries, id, base, allow_audio, false)
}

fn solid_transform_animations_partitioned(
    entries: &AnimationIndex<'_>,
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
    if entries.for_layer(id).any(|entry| {
        entry.target.as_property().is_none_or(|property| {
            !(allowed.contains(&property.property_type())
                || native_3d && is_transform_property(property.property_type())
                || allow_audio && property.property_type() == PropType::AudioVolume)
        })
    }) {
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
    entries: &AnimationIndex<'_>,
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
    if entries.for_layer(rect_id).any(|entry| {
        entry.target.as_property().is_none_or(|property| {
            let kind = property.property_type();
            !(content.contains(&kind)
                || rect_id == transform_id
                    && (transform_properties.contains(&kind)
                        || native_3d && is_native_3d_partition_property(kind)))
        })
    }) {
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
    entries: &AnimationIndex<'_>,
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
        && entries.for_layer(shape_id).any(|entry| {
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
    dynamics: &AnimationIndex<'_>,
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
        .map(|track| {
            // Butt caps cannot paint the one-vertex placeholders used by absent
            // native contour slots. Keep the shared paint stack, including its
            // gradient, opacity and dashes, rather than copying Stroke per slot.
            // Geometry modifiers are excluded because they can expand a point.
            let may_paint_empty_slots = !paints.strokes().is_empty()
                && (paints
                    .strokes()
                    .iter()
                    .any(|stroke| stroke.cap != fx_schema::ShapeLineCap::Butt)
                    || layer.shape.round_corners.is_some()
                    || layer.shape.offset_paths.is_some()
                    || layer.shape.trim.is_some());
            path_animation::visibility(track, may_paint_empty_slots)
        })
        .transpose();
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
    } else if [PropType::TrimStart, PropType::TrimEnd, PropType::TrimOffset]
        .into_iter()
        .any(|property| {
            dynamics
                .first(fx_schema::property::Property::new(layer.id, property))
                .is_some()
        })
    {
        paint_approximations.push((
            layer.id,
            "Trim controls target a Shape without the modifier; inert controls omitted, untrimmed editable owner retained as in FX playback",
        ));
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
    let path_visibility = match path_visibility {
        Ok(visibility) => visibility.flatten(),
        Err(reason) => {
            contents = path_animation::partial_stroke_slots(&contents).ok_or(reason)?;
            paint_approximations.push((layer.id, path_animation::PARTIAL_STROKE_DIAGNOSTIC));
            None
        }
    };
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
    dynamics: &AnimationIndex<'_>,
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
    // FX generators replace the explicit Path. Actual documents retain a
    // close-only placeholder; it contributes no contour and is not a second
    // geometry source. Keep real Path commands and Path animations guarded.
    let placeholder_path = path
        .commands
        .iter()
        .all(|command| matches!(command, ShapePathCommand::Close));
    match (&layer.shape.ellipse, &layer.shape.poly_star) {
        (Some(ellipse), None) if placeholder_path => {
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
    dynamics: &AnimationIndex<'_>,
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
    dynamics: &AnimationIndex<'_>,
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
    dynamics: &AnimationIndex<'_>,
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
    let modifiers = modifier_animations(dynamics, layer.id, true)?;
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
    dynamics: &AnimationIndex<'_>,
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
    let (geometry_dynamics, transform_dynamics): (Vec<_>, Vec<_>) =
        dynamics.for_layer(child.id()).cloned().partition(|entry| {
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
            let geometry_index = AnimationIndex::new(&geometry_dynamics);
            let animations = shape_animations(&geometry_index, shape.id, shape.id)?;
            (
                shape.name.clone(),
                vector_group_transform(&shape.transform)?,
                vec![VectorContent::Geometry {
                    geometry: shape_geometry(shape, &animations, &geometry_index)?,
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
            let geometry = rect_geometry::lower(rect, &AnimationIndex::new(&geometry_dynamics))?;
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
            &AnimationIndex::new(&transform_dynamics),
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
