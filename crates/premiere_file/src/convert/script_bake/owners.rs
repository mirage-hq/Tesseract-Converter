//! Where export writes the keys of each scripted target.
//!
//! One walk over the layer tree gives every layer the role under which the
//! writer places it (`export_layers`, `graphic`, `nested`, `adjustment`), or
//! the reason it places no keyed owner. A target is baked only when its
//! owner's role has a native key binding for it. The table mirrors the
//! writer's selection and reuses its predicates, so that no script is
//! evaluated for an owner that export drops for a reason no key can change,
//! and no key makes the writer drop an owner that it exports without keys (a
//! keyed shape or Color Matte omits its occurrence). Decisions that keys can
//! change (a nest's animation, a stage group, a mask guide) are left to the
//! writer, which reads the baked document;
//! [`super::BakedDocument::report_discarded`] reports what it drops.
//!
//! The index never fails. A layer that the writer rejects, such as a video
//! without its source range, rejects the export when the writer reaches it,
//! and only then; a script on it is not baked.

use std::collections::{BTreeMap, BTreeSet};

use fx_schema::{
    AnimationGraph, EffectData, EffectId, EffectPayload, EffectRecord, GroupLayer, Layer,
    LayerData, LayerEffect, LayerId, PropType, PropertyTarget,
};

use super::super::{
    adjustment,
    effects::{effect_spec, effect_type, invert_output_partner},
    graphic::{bezier_keys_verified, graphic_objects, source_text_field},
    nested::{unsupported_group_fields, unsupported_nest_depth},
    tesseract_to_premiere::{mask_guide_ids, scripted_wipe_axis, source_frame, stage_layers},
    timing::is_plain_group_playback,
    video_data,
};
use crate::schema::{
    text::{GraphicParamSpec, TEXT_PARAMS, VECTOR_MOTION_PARAMS},
    EffectParamBinding, PrAnimatedProperty,
};

/// The fit tolerance cap of a percentage, degree or native effect unit.
const UNIT_TOLERANCE: f64 = 0.1;
/// The fit tolerance cap of a pixel coordinate.
const PIXEL_TOLERANCE: f64 = 0.5;

/// The fit tolerance cap of a point coordinate that is a fraction of a
/// `frame` in pixels, such as a Corner Pin corner's: half a pixel of the
/// frame's longer side. Both coordinates of a point fit with one cap
/// (`Shape::Point`), which keeps each within half a pixel of its own side.
fn frame_fraction_tolerance([width, height]: [u32; 2]) -> f64 {
    PIXEL_TOLERANCE / f64::from(width.max(height))
}

/// How the native keys of one scripted axis must look.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Rules {
    /// The largest fit error, in the target's FX units; the shared fitter can
    /// choose a tighter tolerance relative to the observed range.
    pub(super) tolerance_cap: f64,
    /// Whether the native parameter holds cubic Bezier easing.
    pub(super) cubic: bool,
    /// Whether export writes the keys as scalar keys, which reject a cubic
    /// segment into a key that starts a Hold (`export_scalar_keys`).
    pub(super) scalar_keys: bool,
    /// The values outside which export fails or rejects the track outright.
    pub(super) range: Option<ValueRange>,
}

/// Values that export writes for a target, and fails or rejects outside.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ValueRange {
    /// Opacity and Linear Wipe completion, from 0 to 100.
    Percentage,
    /// Audio gain.
    Nonnegative,
}

impl ValueRange {
    pub(super) fn contains(self, value: f64) -> bool {
        match self {
            Self::Percentage => (0.0..=100.0).contains(&value),
            Self::Nonnegative => value >= 0.0,
        }
    }

    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Percentage => "0 to 100 range",
            Self::Nonnegative => "nonnegative range",
        }
    }
}

impl Rules {
    fn scalar(tolerance_cap: f64) -> Self {
        Self {
            tolerance_cap,
            cubic: true,
            scalar_keys: true,
            range: None,
        }
    }

    fn with_cubic(mut self, cubic: bool) -> Self {
        self.cubic = cubic;
        self
    }

    fn within(mut self, range: ValueRange) -> Self {
        self.range = Some(range);
        self
    }
}

/// The native keys that one scripted target writes.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Binding {
    /// One scalar track.
    Scalar(Rules),
    /// One of the two FX tracks that one native control keys together.
    Pair {
        pair: Pair,
        rules: Rules,
        /// The control's other FX track.
        partner: PropertyTarget,
        /// Whether this track is the control's first: its X, or an Invert's
        /// output white.
        first: bool,
    },
}

impl Binding {
    pub(super) fn rules(&self) -> &Rules {
        match self {
            Self::Scalar(rules) | Self::Pair { rules, .. } => rules,
        }
    }
}

/// A native control that keys two FX tracks together.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Pair {
    /// Motion or Vector Motion Position, one point. A static partner keeps
    /// its value, `partner_static`, at every key.
    Position { partner_static: f64 },
    /// Premiere's uniform Scale: both axes take the X axis's keys.
    Scale,
    /// A Corner Pin corner, one point; export keeps a static partner.
    Corner,
    /// The two outputs of a Levels in Invert's form: one Blend With Original
    /// track when they are the effect's only animation and the output black
    /// is the output white's complement, and otherwise two Levels outputs.
    Invert,
}

impl Pair {
    /// Why the control's two tracks bake together.
    pub(super) fn reason(self) -> &'static str {
        match self {
            Self::Position { .. } => "Premiere keys Position as one point",
            Self::Scale => "Premiere Scale is uniform",
            Self::Corner => "Premiere keys a Corner Pin corner as one point",
            Self::Invert => "Premiere keys an Invert's two Levels outputs as one track",
        }
    }
}

/// A scripted target's native binding on its owner.
#[derive(Debug)]
pub(super) struct Bound<'d> {
    pub(super) owner: &'d Owner<'d>,
    pub(super) binding: Binding,
}

/// Why export writes a layer's keys, or why it writes none.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Role {
    /// A video clip; a root clip also writes its sound's Volume keys.
    Video {
        root: bool,
    },
    /// A group that exports as one stage clip or a nested sequence.
    Placement,
    /// An adjustment layer: its Opacity and effects.
    Adjustment,
    /// A root group of text and shape layers: Vector Motion and clip Opacity.
    GraphicGroup,
    /// A root text graphic, or the one text of a root graphic group.
    Text,
    /// A rectangle that a mask uses as its Linear Wipe (`wipe`) or Crop guide.
    Guide {
        wipe: Option<PropType>,
    },
    /// A root audio layer.
    Audio,
    Unbound(String),
}

/// One layer whose properties or effects a script may target.
#[derive(Debug)]
pub(super) struct Owner<'d> {
    pub(super) layer: &'d Layer,
    role: Role,
    /// The canvas, in pixels, of the sequence that exports the owner.
    canvas: [u32; 2],
    /// Why the owner's own clock is not the clock that a layer-time script
    /// reads, if it is not.
    clock: Option<&'static str>,
}

impl Owner<'_> {
    /// The owner-local window that layer-time scripts and keys share, in
    /// milliseconds from the owner's active start.
    pub(super) fn window_ms(&self) -> u64 {
        self.layer.active_range().duration.as_millis()
    }

    /// How omissions name the owner, as the writer names layers.
    pub(super) fn record(&self) -> String {
        format!("layer {} ({:?})", self.layer.id(), self.layer.name())
    }
}

/// Where a layer list exports, for the roles of its layers.
#[derive(Debug, Clone)]
enum Place {
    Root,
    /// Inside a group that exports as a nest or stage clip, `depth` groups deep.
    Nest {
        depth: usize,
    },
    /// The objects of a root graphic group; `one_text` when its only object
    /// is a text, whose own keys a graphic writes.
    Graphic {
        one_text: bool,
    },
    /// Inside a layer that export does not place.
    Unexported(String),
}

const AUDIO_IN_NEST: &str = "audio inside a nested sequence is not converted";
const TEXT_IN_NEST: &str = "text inside a nested sequence is not exported";
const GRAPHIC_IN_NEST: &str = "a graphic inside a nested sequence is not exported";
const NO_CLIP: &str = "a group without a video or adjustment layer exports no nested sequence";
const SHAPE: &str = "graphic shapes export without keys; a keyed shape omits its graphic";
const KEYED_OBJECT: &str =
    "an object of a graphic with several objects has no keys; keying one omits the graphic";
const STILL: &str = "a still image exports without keys";
const MATTE: &str = "a Color Matte exports without keys; an animated one is omitted";
const RECT_IN_NEST: &str = "a rectangle inside a nested sequence is not exported";
const UNSUPPORTED_LAYER: &str = "this layer type is not exported";

/// Every layer and identified effect that a script may target.
#[derive(Debug)]
pub(super) struct Owners<'d> {
    layers: BTreeMap<LayerId, Owner<'d>>,
    effects: BTreeMap<EffectId, (LayerId, &'d EffectRecord)>,
}

impl<'d> Owners<'d> {
    /// The owners of a document whose root sequence is `canvas` pixels.
    pub(super) fn new(layers: &'d [Layer], dynamics: &AnimationGraph, canvas: [u32; 2]) -> Self {
        let mut owners = Self {
            layers: BTreeMap::new(),
            effects: BTreeMap::new(),
        };
        owners.collect(layers, &Place::Root, None, None, dynamics, canvas);
        owners
    }

    /// Indexes `layers`, one list in `place` of a sequence of `canvas`
    /// pixels; `group_guides` are the guides of its group's own masks, which
    /// are among these layers.
    fn collect(
        &mut self,
        layers: &'d [Layer],
        place: &Place,
        inherited_clock: Option<&'static str>,
        group_owner: Option<&'d GroupLayer>,
        dynamics: &AnimationGraph,
        canvas: [u32; 2],
    ) {
        // The guides that the writer's layer export skips, as in `export_layers`.
        let guides: BTreeSet<_> = mask_guide_ids(layers)
            .into_iter()
            .chain(
                group_owner
                    .into_iter()
                    .flat_map(|group| group.masks.iter().filter_map(|mask| mask.layer)),
            )
            .collect();
        for layer in layers {
            let (role, children) =
                role(layer, layers, place, &guides, group_owner, dynamics, canvas);
            let clock = inherited_clock.or_else(|| own_clock(layer));
            for effect in layer.effects() {
                if let EffectData::Identified { id, .. } = effect.data() {
                    self.effects.insert(*id, (layer.id(), effect));
                }
            }
            self.layers.insert(
                layer.id(),
                Owner {
                    layer,
                    role,
                    canvas,
                    clock,
                },
            );
            if let Some(child_layers) = layer.child_layers() {
                let owner = match layer.data() {
                    LayerData::Group(group) => Some(group),
                    _ => None,
                };
                // Export writes a nest's sequence at the canvas of the sequence
                // that places it (`nested::export_group`), the only nest canvas
                // that it supports.
                self.collect(child_layers, &children, clock, owner, dynamics, canvas);
            }
        }
    }

    /// The owner and native binding of a scripted `target`, or why export
    /// writes no keys for it.
    pub(super) fn bind(&self, target: &PropertyTarget) -> std::result::Result<Bound<'_>, String> {
        let (owner, binding) = match target {
            PropertyTarget::LayerProperty(property) => {
                let owner = self
                    .layers
                    .get(&property.layer_id())
                    .ok_or_else(|| "the target layer does not exist".to_owned())?;
                (owner, layer_binding(owner, property.property_type())?)
            }
            PropertyTarget::EffectProperty(target) => {
                let (layer, effect) = self
                    .effects
                    .get(&target.effect_id())
                    .ok_or_else(|| "the target effect does not exist".to_owned())?;
                let owner = &self.layers[layer];
                (owner, effect_binding(owner, effect, target.param_name())?)
            }
            PropertyTarget::FxItemProperty(_) => {
                return Err("mask and layer-style properties have no native keys".to_owned())
            }
        };
        if let Some(reason) = owner.clock {
            return Err(reason.to_owned());
        }
        Ok(Bound { owner, binding })
    }
}

/// The role of `layer` in `place`, and where its children export.
fn role(
    layer: &Layer,
    layers: &[Layer],
    place: &Place,
    guides: &BTreeSet<LayerId>,
    group_owner: Option<&GroupLayer>,
    dynamics: &AnimationGraph,
    canvas: [u32; 2],
) -> (Role, Place) {
    let inside = |reason: &str| Place::Unexported(reason.to_owned());
    let unbound = |reason: &str| (Role::Unbound(reason.to_owned()), inside(reason));
    let (root, depth) = match place {
        Place::Root => (true, 0),
        Place::Nest { depth } => (false, *depth),
        Place::Graphic { one_text } => {
            return match layer.data() {
                LayerData::Text(_) if *one_text => (Role::Text, inside(UNSUPPORTED_LAYER)),
                LayerData::Text(_) => unbound(KEYED_OBJECT),
                _ => unbound(SHAPE),
            }
        }
        // A layer that no container could export gives its own reason; one
        // that would export in another container gives its container's.
        Place::Unexported(reason) => {
            let own = match layer.data() {
                LayerData::Shape(_) => SHAPE,
                LayerData::Image(_) => STILL,
                LayerData::Audio(_) => AUDIO_IN_NEST,
                LayerData::Text(_) => TEXT_IN_NEST,
                LayerData::Group(group) if graphic_objects(group, dynamics).is_some() => {
                    GRAPHIC_IN_NEST
                }
                LayerData::Rect(rect) if !guides.contains(&rect.id) => RECT_IN_NEST,
                LayerData::BooleanOperation(_) | LayerData::Pag(_) | LayerData::AiEdit(_) => {
                    UNSUPPORTED_LAYER
                }
                _ => reason,
            };
            return (Role::Unbound(own.to_owned()), inside(reason));
        }
    };
    match video_data(layer) {
        Ok(Some(_)) => return (Role::Video { root }, inside(UNSUPPORTED_LAYER)),
        // The writer rejects the export if it reaches this layer.
        Err(error) => return unbound(&error.to_string()),
        Ok(None) => {}
    }
    match layer.data() {
        LayerData::Audio(_) if root => (Role::Audio, inside(UNSUPPORTED_LAYER)),
        LayerData::Audio(_) => unbound(AUDIO_IN_NEST),
        LayerData::Adjustment(adjustment) => {
            match adjustment::unexported_reason(adjustment, layers, dynamics, canvas) {
                None => (Role::Adjustment, inside(UNSUPPORTED_LAYER)),
                Some(reason) => unbound(&reason),
            }
        }
        LayerData::Group(group) if graphic_objects(group, dynamics).is_some() => {
            if !root {
                return unbound(GRAPHIC_IN_NEST);
            }
            let one_text = matches!(group.layers.as_slice(), [child] if matches!(child.data(), LayerData::Text(_)));
            (Role::GraphicGroup, Place::Graphic { one_text })
        }
        LayerData::Group(group) => {
            // A stage group may export as one clip, which reads other fields
            // than a nest; the writer decides between the two.
            let reason = if stage_layers(group).is_some() {
                None
            } else {
                unsupported_group_fields(group, dynamics)
                    .or_else(|| unsupported_nest_depth(depth))
                    .or_else(|| (!places_clip(group, dynamics, canvas)).then(|| NO_CLIP.to_owned()))
            };
            match reason {
                Some(reason) => (Role::Unbound(reason.clone()), Place::Unexported(reason)),
                None => (Role::Placement, Place::Nest { depth: depth + 1 }),
            }
        }
        LayerData::Text(_) if root => (Role::Text, inside(UNSUPPORTED_LAYER)),
        LayerData::Text(_) => unbound(TEXT_IN_NEST),
        LayerData::Rect(rect) if guides.contains(&rect.id) => (
            Role::Guide {
                wipe: scripted_wipe_axis(rect, layers, group_owner, dynamics, canvas),
            },
            inside(UNSUPPORTED_LAYER),
        ),
        LayerData::Rect(_) if root => unbound(MATTE),
        LayerData::Rect(_) => unbound(RECT_IN_NEST),
        LayerData::Shape(_) => unbound(SHAPE),
        LayerData::Image(_) => unbound(STILL),
        LayerData::Video(_)
        | LayerData::Media(_)
        | LayerData::BooleanOperation(_)
        | LayerData::Pag(_)
        | LayerData::AiEdit(_) => unbound(UNSUPPORTED_LAYER),
    }
}

/// Whether export can place a clip inside `group`, through groups only: a
/// video or an adjustment layer. The writer places a nest only around at
/// least one placed clip. An invalid video counts: the writer rejects the
/// export when it reaches it.
fn places_clip(group: &GroupLayer, dynamics: &AnimationGraph, canvas: [u32; 2]) -> bool {
    group.layers.iter().any(|layer| match layer.data() {
        LayerData::Group(inner) => places_clip(inner, dynamics, canvas),
        LayerData::Adjustment(adjustment) => {
            adjustment::unexported_reason(adjustment, &group.layers, dynamics, canvas).is_none()
        }
        _ => !matches!(video_data(layer), Ok(None)),
    })
}

/// Why `layer`'s own clock differs from its active-start clock, which a
/// layer-time script and its keys share, if it does. The runtime reads a
/// playback remap, and holds a Posterize Time; the writer converts neither.
fn own_clock(layer: &Layer) -> Option<&'static str> {
    // Affine media animation reads the authored input clock, independently
    // of the source rate. Its local zero must coincide with the visible start.
    let media_clock = |playback: &fx_schema::LayerPlayback| {
        matches!(playback.mapping(), fx_schema::LayerPlaybackMapping::Linear { input, .. }
            if i128::from(playback.input_range().start.as_millis())
                + i128::from(playback.input_offset_ms())
                == i128::from(input.start.as_millis()))
    };
    let remaps_clock = match layer.data() {
        LayerData::Group(group) => !is_plain_group_playback(&group.playback),
        LayerData::Video(video) => !media_clock(&video.playback),
        LayerData::Audio(audio) => !media_clock(&audio.playback),
        // Preserve the prior guard for historical or retained playback fields.
        _ => layer
            .wire_value()
            .get("playback")
            .is_some_and(|playback| !playback.is_null()),
    };
    if remaps_clock {
        return Some("the owner or an enclosing group remaps its clock with playback");
    }
    layer
        .effects()
        .iter()
        .any(|effect| {
            matches!(
                effect.data(),
                EffectData::Identified {
                    enabled: true,
                    effect: EffectPayload::Known(LayerEffect::PosterizeTime { .. }),
                    ..
                } | EffectData::Legacy(EffectPayload::Known(LayerEffect::PosterizeTime { .. }))
            )
        })
        .then_some("Posterize Time on the owner or an enclosing group holds its clock")
}

/// The static Position of a layer with Motion, which a Position with one
/// scripted axis keeps on the other.
fn static_position(layer: &Layer) -> std::result::Result<[f64; 2], String> {
    let transform = match layer.data() {
        LayerData::Group(group) => &group.transform,
        LayerData::Text(text) => &text.transform,
        LayerData::Rect(rect) => &rect.transform,
        _ => {
            return match video_data(layer) {
                Ok(Some(video)) => Ok(video.transform.position.xy_array()),
                Ok(None) => Err("this layer has no Motion".to_owned()),
                Err(error) => Err(error.to_string()),
            }
        }
    };
    Ok(transform.position.xy_array())
}

/// The frame, in pixels, of which the points of `owner`'s effects are
/// fractions: that of the clip that export writes them on (`EffectHost`). A
/// video's is its `sourceRect`, which must be valid as export requires; a
/// nest placement's and an adjustment layer's is the canvas of the sequence
/// that holds them, the only nest and adjustment frame that export supports.
fn effect_frame(owner: &Owner<'_>) -> std::result::Result<[u32; 2], String> {
    match video_data(owner.layer) {
        Ok(Some(video)) => source_frame(video.source.frame_rect).map_err(|error| error.to_string()),
        Ok(None) => Ok(owner.canvas),
        Err(error) => Err(error.to_string()),
    }
}

/// The native binding of the layer property `property` on `owner`.
fn layer_binding(owner: &Owner<'_>, property: PropType) -> std::result::Result<Binding, String> {
    let motion =
        |specs: Option<&[GraphicParamSpec]>| -> std::result::Result<Option<Binding>, String> {
            let cubic = |animated| specs.is_none_or(|specs| bezier_keys_verified(specs, animated));
            let pair = |pair, rules, partner| Binding::Pair {
                pair,
                rules,
                partner: PropertyTarget::layer(owner.layer.id(), partner),
                first: matches!(property, PropType::PositionX | PropType::ScaleX),
            };
            Ok(match property {
                PropType::Opacity => Some(Binding::Scalar(
                    Rules::scalar(UNIT_TOLERANCE)
                        .with_cubic(cubic(PrAnimatedProperty::Opacity))
                        .within(ValueRange::Percentage),
                )),
                PropType::Rotation => Some(Binding::Scalar(
                    Rules::scalar(UNIT_TOLERANCE).with_cubic(cubic(PrAnimatedProperty::Rotation)),
                )),
                PropType::PositionX | PropType::PositionY => {
                    let [x, y] = static_position(owner.layer)?;
                    let (partner, partner_static) = match property {
                        PropType::PositionX => (PropType::PositionY, y),
                        _ => (PropType::PositionX, x),
                    };
                    Some(pair(
                        Pair::Position { partner_static },
                        Rules {
                            scalar_keys: false,
                            ..Rules::scalar(PIXEL_TOLERANCE)
                                .with_cubic(cubic(PrAnimatedProperty::Position))
                        },
                        partner,
                    ))
                }
                PropType::ScaleX | PropType::ScaleY => Some(pair(
                    Pair::Scale,
                    Rules::scalar(UNIT_TOLERANCE)
                        .with_cubic(cubic(PrAnimatedProperty::UniformScale)),
                    match property {
                        PropType::ScaleX => PropType::ScaleY,
                        _ => PropType::ScaleX,
                    },
                )),
                _ => None,
            })
        };
    let binding = match &owner.role {
        Role::Unbound(reason) => return Err(reason.clone()),
        Role::Audio | Role::Video { root: true } if property == PropType::AudioVolume => {
            // Linear gain: the writer rejects negative gains and splits every
            // curve into Linear pieces itself, so only relative fitting applies.
            Some(Binding::Scalar(Rules {
                scalar_keys: false,
                ..Rules::scalar(f64::INFINITY).within(ValueRange::Nonnegative)
            }))
        }
        Role::Video { root: false } if property == PropType::AudioVolume => {
            return Err(AUDIO_IN_NEST.to_owned())
        }
        Role::Audio => None,
        Role::Video { .. } | Role::Placement => motion(None)?,
        // An adjustment's Opacity keys write through the clip path; FX ignores
        // its geometric transform.
        Role::Adjustment => (property == PropType::Opacity).then(|| {
            Binding::Scalar(Rules::scalar(UNIT_TOLERANCE).within(ValueRange::Percentage))
        }),
        // The clip Opacity of a graphic group holds Bezier keys; its Vector
        // Motion follows its verified parameters.
        Role::GraphicGroup if property == PropType::Opacity => motion(None)?,
        Role::GraphicGroup => motion(Some(&VECTOR_MOTION_PARAMS))?,
        Role::Text if source_text_field(property).is_some() => {
            return Err(format!(
                "{property} animates Source Text, whose native keys are text document snapshots, not scalar or paired controls"
            ))
        }
        Role::Text => motion(Some(&TEXT_PARAMS))?,
        // A Linear Wipe's completion is its guide's one Scale track.
        Role::Guide { wipe: Some(axis) } => (property == *axis)
            .then(|| Binding::Scalar(Rules::scalar(UNIT_TOLERANCE).within(ValueRange::Percentage))),
        // A Crop guide keeps the frame tracks of its video's Motion.
        Role::Guide { wipe: None } if property == PropType::Opacity => None,
        Role::Guide { wipe: None } => motion(None)?,
    };
    binding.ok_or_else(|| format!("{property} has no native key binding on this owner"))
}

/// The native binding of the parameter `param` of the FX effect `effect` on
/// `owner`: a clip, nest or adjustment layer writes the keys of the
/// parameters that its native effect binds ([`effect_spec`]).
fn effect_binding(
    owner: &Owner<'_>,
    effect: &EffectRecord,
    param: &str,
) -> std::result::Result<Binding, String> {
    // The effect's own mapping precedes its host's, which could change.
    let EffectData::Identified {
        id,
        effect: payload,
        ..
    } = effect.data()
    else {
        return Err("a legacy effect record has no animation target".to_owned());
    };
    let spec = effect_spec(payload)
        .ok_or_else(|| format!("{} has no Premiere effect mapping", effect_type(payload)))?;
    let bound = spec
        .bound_param(param)
        .ok_or_else(|| format!("{} {param} has no native key binding", effect_type(payload)))?;
    match &owner.role {
        Role::Video { .. } | Role::Placement | Role::Adjustment => {}
        Role::Unbound(reason) => return Err(reason.clone()),
        _ => return Err("effects on this owner export without keys".to_owned()),
    }
    let pair = |pair, rules, partner, first| Binding::Pair {
        pair,
        rules,
        partner: PropertyTarget::effect_param(*id, partner),
        first,
    };
    if let Some(partner) = invert_output_partner(payload, param) {
        // A tenth of one level: finer than both whole Levels outputs and a
        // tenth of one percent of Blend With Original (0.255 levels).
        return Ok(pair(
            Pair::Invert,
            Rules::scalar(UNIT_TOLERANCE),
            partner,
            param == "outputWhite",
        ));
    }
    Ok(match bound.binding {
        Some(EffectParamBinding::Colour { .. }) => {
            return Err(
                "native colour keys couple three channels, outside scalar and paired script baking"
                    .to_owned(),
            );
        }
        Some(EffectParamBinding::TileCount { .. }) => {
            return Err(
                "native Replicate Count keys couple four tile fields, outside scalar and paired script baking"
                    .to_owned(),
            );
        }
        Some(EffectParamBinding::Scalar("amount"))
            if matches!(
                payload,
                EffectPayload::Known(LayerEffect::TintTritone { .. })
            ) =>
        {
            Binding::Scalar(Rules::scalar(UNIT_TOLERANCE).within(ValueRange::Percentage))
        }
        Some(EffectParamBinding::Point { x, y }) => pair(
            Pair::Corner,
            Rules {
                scalar_keys: false,
                ..Rules::scalar(frame_fraction_tolerance(effect_frame(owner)?))
            },
            if param == x { y } else { x },
            param == x,
        ),
        // Half of one whole native step, which export rounds to.
        Some(EffectParamBinding::Integer { divisor, .. }) => {
            Binding::Scalar(Rules::scalar(0.5 / f64::from(divisor)))
        }
        Some(EffectParamBinding::Scalar(_) | EffectParamBinding::ScaledScalar { .. }) | None => {
            Binding::Scalar(Rules::scalar(UNIT_TOLERANCE))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::own_clock;
    use fx_schema::{Duration, Layer, LayerPlayback, Time, TimeRangeProperty};
    use serde_json::json;

    fn range(start: u64, duration: u64) -> TimeRangeProperty {
        TimeRangeProperty::new(Time::from_millis(start), Duration::from_millis(duration))
    }

    fn media(kind: &str, playback: &LayerPlayback) -> Layer {
        let mut wire = json!({
            "type": kind, "id": 1, "name": "Clocked owner", "playback": playback,
            "sourceRange": {"start": 2000, "duration": 4000},
            "sourceIntrinsicDuration": 10000, "volume": 0,
            "source": {"assetId": "source"}
        });
        if kind == "Video" {
            wire["transform"] = json!({
                "anchorPoint": [0, 0], "position": [0, 0], "scale": [100, 100],
                "rotation": 0, "opacity": 100
            });
        }
        serde_json::from_value(wire).unwrap()
    }

    #[test]
    fn media_input_clocks_accept_signed_offsets_that_cancel_the_authored_origin() {
        for offset in [-500, 0, 500] {
            let input_start = u64::try_from(1000 + offset).unwrap();
            // The source runs at 2x; layer-time animation still reads the 1x input clock.
            let playback = LayerPlayback::linear(
                range(1000, 2000),
                range(input_start, 2000),
                range(2000, 4000),
                offset,
            )
            .unwrap();
            for kind in ["Video", "Audio"] {
                assert_eq!(
                    own_clock(&media(kind, &playback)),
                    None,
                    "{kind}, offset {offset}"
                );
            }
        }
    }

    #[test]
    fn media_input_clocks_reject_a_mismatched_authored_origin() {
        for offset in [-500, 500] {
            let input_start = u64::try_from(1000 + offset + 1).unwrap();
            let playback = LayerPlayback::linear(
                range(1000, 2000),
                range(input_start, 2000),
                range(2000, 4000),
                offset,
            )
            .unwrap();
            for kind in ["Video", "Audio"] {
                assert_eq!(
                    own_clock(&media(kind, &playback)),
                    Some("the owner or an enclosing group remaps its clock with playback"),
                    "{kind}, offset {offset}",
                );
            }
        }
    }

    #[test]
    fn media_time_remap_and_enabled_posterize_time_still_reject_scripts() {
        let property = serde_json::from_value(json!({"keyframes": [
            {"id": "a", "time": 1000, "value": 2000, "easing": {"type": "linear"}},
            {"id": "b", "time": 3000, "value": 4000, "easing": {"type": "linear"}}
        ], "before": "inactive", "after": "inactive"}))
        .unwrap();
        let remapped = LayerPlayback::remapped(range(1000, 2000), property, 0).unwrap();
        let linear =
            LayerPlayback::linear(range(1000, 2000), range(1000, 2000), range(2000, 4000), 0)
                .unwrap();
        for kind in ["Video", "Audio"] {
            assert_eq!(
                own_clock(&media(kind, &remapped)),
                Some("the owner or an enclosing group remaps its clock with playback"),
            );
        }
        for enabled in [false, true] {
            let mut wire = media("Video", &linear).wire_value().clone();
            wire["effects"] = json!([{"id": 7, "enabled": enabled,
                    "effect": {"type": "posterizeTime", "frameRate": 12}}]);
            let layer: Layer = serde_json::from_value(wire).unwrap();
            assert_eq!(
                own_clock(&layer),
                enabled
                    .then_some("Posterize Time on the owner or an enclosing group holds its clock"),
            );
        }
    }

    #[test]
    fn group_scripts_require_a_zero_origin_unit_clock() {
        for (input_start, output_start, offset, output_duration, plain) in [
            (1000, 0, 0, 2000, true),
            (500, 0, -500, 2000, true),
            (1000, 1, 0, 2000, false),
            (1000, 0, 0, 4000, false),
        ] {
            let playback = LayerPlayback::linear(
                range(1000, 2000),
                range(input_start, 2000),
                range(output_start, output_duration),
                offset,
            )
            .unwrap();
            let group = serde_json::from_value(json!({
                "type": "Group", "id": 1, "name": "Clocked group", "playback": playback,
                "transform": {"anchorPoint": [0, 0], "position": [0, 0], "scale": [100, 100],
                    "rotation": 0, "opacity": 100},
                "layers": []
            }))
            .unwrap();
            assert_eq!(own_clock(&group).is_none(), plain);
        }
    }
}
