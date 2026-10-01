//! Canonical persisted track-matte and path-mask schema.
use super::{ShapePath, TrackMatteType};
use crate::{FxItemId, LayerId, NonNegativeProperty, ScalarProperty, Vector2Property};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Inline track matte definition. References another layer in the same
/// composition by id; the referenced layer is evaluated as the matte
/// source and is filtered out of normal child iteration so it doesn't also
/// render as regular content. Sources normally share the masked layer's parent;
/// a Group may instead consume one of its direct children. The reference lives
/// in the layer tree (root or any nested group), keeping a single source of
/// truth instead of an owned copy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub struct TrackMatte {
    /// Which channel of the matte layer's render gates this layer's pixels
    /// (alpha / alphaInverted / luma / lumaInverted).
    #[serde(default)]
    pub mode: TrackMatteType,
    /// Layer id of the mask source. Must resolve to a layer in this
    /// composition; dangling ids are dropped with a warn-once.
    pub layer: LayerId,
}

/// Boolean-combination mode for a stacked [`PathMask`] (JRB-1200).
///
/// Mirrors PAG's `MaskMode` (`crates/pag/src/render/masks.rs`) — the eight AE
/// mask modes. The mask stack lowers to the scene [`scene::Mask`] IR exactly
/// as the PAG renderer composes its mask blocks: `Add`/`Accum`/`Lighten`
/// union, `Subtract`/`Intersect`/`Darken` nest an `Alpha` / `AlphaInverted`
/// mask, and `Difference` XORs via an `EvenOdd` fill (see
/// [`crate::masks::evaluate_path_masks`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub enum MaskMode {
    /// Inert — contributes no coverage (lets a mask be disabled in place).
    None,
    /// Union the mask shape into the accumulated coverage.
    #[default]
    Add,
    /// Remove the mask shape from the accumulated coverage.
    Subtract,
    /// Keep only the overlap with the accumulated coverage.
    Intersect,
    /// `max(a, b)` — union for binary shape masks (same as `Add`).
    Lighten,
    /// `min(a, b)` — intersection for binary shape masks (same as `Intersect`).
    Darken,
    /// XOR the mask shape against the accumulated coverage (`EvenOdd`).
    Difference,
    /// Additive accumulation — union for binary shape masks (same as `Add`).
    Accum,
}

/// A single stacked vector-path mask on a layer (JRB-1200 / ENG-1471): takes
/// its outline geometry from a same-parent [`RectLayer`], [`ShapeLayer`], or
/// [`BooleanOperationLayer`] referenced by [`LayerId`]. A Group may instead
/// consume a direct child as its guide. Unlike [`TrackMatte`], a path mask
/// samples vector geometry rather than rendered alpha. `feather` / `expansion`
/// / `opacity` animate via `fxItemProperty` targets keyed by the mask's fx-item
/// `id`.
///
/// The referenced layer is a guide: like a track-matte source it is consumed
/// by this reference and does not also paint as regular content. Its own
/// transform applies — moving the layer moves the mask (the outline is mapped
/// into the masked layer's local content space via [`resolve_shape_ref_path`]).
/// A layer may carry a whole [`Vec<PathMask>`] stack; the stack lowers to one
/// scene
/// [`scene::Mask`] via [`crate::masks::evaluate_path_masks`].
///
/// The [`FxItemId`] gives the mask a stable, composition-unique identity so an
/// fx-item animator can address its `feather` / `expansion` / `opacity`
/// (JRB-1373; applied per frame via [`Self::apply_animated_property`], see
/// [`Self::ANIMATABLE_PROPERTIES`]). A referenced [`ShapeLayer`]'s outline is
/// animated with [`crate::PropType::ShapePath`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub struct PathMask {
    /// Stable, composition-unique identity of this mask instance.
    pub id: FxItemId,
    /// Controls this mask's configured contribution to the stack. The wire
    /// value `none` keeps the mask configured but disables its contribution.
    /// Other modes combine evaluated coverage with masks above it.
    #[serde(default)]
    pub mode: MaskMode,
    /// Invert the mask coverage (`AlphaInverted` on the composed source).
    #[serde(default)]
    pub inverted: bool,
    /// Id of the sibling [`RectLayer`], [`ShapeLayer`], or
    /// [`BooleanOperationLayer`] whose outline is the mask geometry. The
    /// source layer's own transform applies.
    ///
    /// `Option` only for backward compatibility: documents authored before
    /// the ENG-1471 shape-layer-reference migration shipped carry no `layer`
    /// reference at all — their outline was inline on [`Self::legacy_path`]
    /// instead (JRB-1591; ENG-1471's commit message claimed that pre-migration
    /// wire shape "was never used in the wild", which real PROD projects
    /// contradict). Always `Some` for masks authored through the mutation API
    /// (`set_layer_masks` rejects a `None` — see
    /// [`crate::FXCompositionMutationError::PathMaskMissingShapeRef`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerId>,
    /// Pre-ENG-1471 inline outline geometry, in the *masked* layer's own
    /// local content space (that migration's commit message: "`PathMask.path:
    /// ShapePath` → `PathMask.layer: LayerId`"). Accepted on read only, for
    /// documents persisted before the migration shipped (JRB-1591) — real
    /// PROD projects still carry this shape, so `project_extract_assets`
    /// hard-failed parsing them with a `deny_unknown_fields` "unknown field
    /// `path`" error. Round-trips as-is (a legacy mask that is loaded and
    /// re-saved untouched keeps working) since there is no sibling shape
    /// layer to migrate it onto; [`crate::masks::evaluate_path_masks`] uses it
    /// directly — unlike [`Self::layer`], it needs no per-frame lookup
    /// against a guide layer or affine remap into the consumer's local space,
    /// since it was already authored there. Mutually exclusive with
    /// [`Self::layer`] in practice: an old document has this and no `layer`;
    /// every document written by this build (or read back unmodified) has
    /// exactly one of the two.
    #[serde(default, rename = "path", skip_serializing_if = "Option::is_none")]
    pub legacy_path: Option<ShapePath>,
    /// Soft-edge feather radii `[x, y]` in pixels (`[0, 0]` = hard edge).
    #[serde(default)]
    pub feather: Vector2Property,
    /// Grow (`+`) / shrink (`-`) the mask boundary in pixels before feather.
    #[serde(default)]
    pub expansion: ScalarProperty,
    /// Mask opacity in `0..=1` (defaults to fully opaque).
    #[serde(default = "default_mask_opacity")]
    pub opacity: NonNegativeProperty,
}

fn default_mask_opacity() -> NonNegativeProperty {
    NonNegativeProperty::new(1.0).expect("1.0 is a valid non-negative opacity")
}
