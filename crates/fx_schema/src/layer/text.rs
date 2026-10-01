//! Canonical persisted text-layer schema.
use super::{
    model::default_true, BlendMode, PathMask, TextDocument, TextPathAlign, TrackMatte, Transform,
};
use crate::{
    effect::EffectRecord as EffectInstance, FxItemId, LayerId, ScalarProperty, TimeRangeProperty,
};
#[path = "text_declaration.rs"]
mod declaration;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

crate::define_text_layer_schema!();

/// Which run of characters an animator's per-character transform pivots about
/// (AE *More Options > Anchor Point Grouping*).
///
/// Values and order match AE's dropdown and libpag's `AnchorPointGrouping`
/// (`Character = 0, Word, Line, All`), which parses and round-trips the enum but
/// implements none of it — see `include/pag/file.h` and
/// `codec/tags/text/TextMoreOption.cpp`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub enum AnchorPointGrouping {
    /// Every character pivots about itself (AE's default).
    #[default]
    Character,
    /// Every character pivots about its word, so a word rotates as one piece.
    Word,
    /// Every character pivots about its visual line.
    Line,
    /// Every character pivots about the whole text block.
    All,
}

/// The anchor-point grouping the animator stack pivots about, and where inside
/// each group the pivot sits (JRB-1791 item 4).
///
/// The **anchor subset** of AE's *Text > More Options* panel — which is also what
/// libpag calls `TextMoreOptions` (tag 10). Named for its contents rather than for
/// AE's panel because it carries only the two anchor controls, not that panel's
/// `fillAndStroke` / `interCharacterBlending`, which no path in this repo consumes.
/// The PAG importer does not read tag 10 today, so a PAG-sourced text layer arrives
/// with no block and keeps the legacy pivot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub struct TextAnchorOptions {
    /// Stable, caller-minted identity for graph addressing of this block's
    /// animatable vector (`groupingAlignment`) via
    /// `PropertyTarget::FxItemProperty`.
    ///
    /// Required (no serde default) for the same reason as
    /// [`TextPathOptions::id`]: a default would mint `FxItemId(0)` for every
    /// id-less block, collapsing two layers into one entry in
    /// `collect_fx_item_ids` and letting one dynamics target drive both.
    pub id: FxItemId,
    /// The unit each character's transform pivots about. Static — AE does not
    /// keyframe the dropdown.
    #[serde(default)]
    pub anchor_point_grouping: AnchorPointGrouping,
    /// AE *Grouping Alignment*, in **percent** (`[0, 0]` = the group's default
    /// anchor). Animatable through the dynamics graph, which is what lets an
    /// author slide the pivot across a word over time.
    ///
    /// The percentages' denominators are inferred rather than measured — see
    /// [`scene::grouping_alignment_offset`], the single site that reads them.
    #[serde(default)]
    pub grouping_alignment: crate::Vector2Property,
}

impl Default for TextAnchorOptions {
    /// A placeholder-id block with AE's own defaults, so tests and builders can
    /// spell out only the field they care about. Real blocks get a
    /// caller-minted [`FxItemId`], the same convention
    /// [`crate::text_animator::TextAnimator::default`] follows.
    fn default() -> Self {
        Self {
            id: FxItemId::new(0),
            anchor_point_grouping: AnchorPointGrouping::default(),
            grouping_alignment: [0.0, 0.0],
        }
    }
}

/// AE *Text > Path Options* — lay a text layer's glyphs along the outline of
/// a sibling [`crate::RectLayer`], [`crate::ShapeLayer`], or
/// [`crate::BooleanOperationLayer`],
/// referenced by [`LayerId`] (JRB-1219 / ENG-1471).
///
/// The referenced layer is a *guide*: like a track-matte source it is consumed
/// by this reference and does not also paint as a standalone sibling. Its own
/// transform applies — moving the layer moves the
/// text path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub struct TextPathOptions {
    /// Stable, caller-minted identity for graph addressing of this option's
    /// animatable scalars (`firstMargin` / `lastMargin`) via
    /// `PropertyTarget::FxItemProperty`. Animating `firstMargin` scrolls the
    /// text along the path (the canonical AE reveal).
    ///
    /// Required (no serde default): a default would mint the same `FxItemId(0)`
    /// for every id-less block, so two such layers would collapse to one entry
    /// in `collect_fx_item_ids` and a dynamics target for item 0 would drive
    /// every layer's margins. The id must be uniquely caller-minted, matching
    /// how animator selector ids are assigned (and how `path_layer` is
    /// required).
    pub id: FxItemId,
    /// Id of the sibling [`crate::RectLayer`], [`crate::ShapeLayer`], or
    /// [`crate::BooleanOperationLayer`] whose outline the glyphs follow. The
    /// source layer's own transform applies; a [`crate::ShapeLayer`] can
    /// animate its [`crate::PropType::ShapePath`] to morph the guide per frame.
    pub path_layer: LayerId,
    /// Distance in pixels before the first glyph (AE *First Margin*).
    #[serde(default)]
    pub first_margin: ScalarProperty,
    /// Distance in pixels reserved after the last glyph (AE *Last Margin*);
    /// only affects layout together with [`Self::force_alignment`].
    #[serde(default)]
    pub last_margin: ScalarProperty,
    /// Rotate each glyph to the path tangent (AE *Perpendicular To Path*).
    /// When `false`, glyphs stay upright and only their positions follow.
    #[serde(default = "default_true")]
    pub perpendicular_to_path: bool,
    /// Walk the path from its end toward its start (AE *Reverse Path*).
    #[serde(default)]
    pub reverse_path: bool,
    /// Distribute the glyphs to fill the usable path length (AE *Force
    /// Alignment*).
    #[serde(default)]
    pub force_alignment: bool,
    /// Which part of the type meets the guide curve (JRB-1819). Resolved
    /// against the run's font metrics at draw time, so it tracks font-size
    /// changes instead of pinning a pixel distance.
    ///
    /// Defaults to [`TextPathAlign::Bottom`] (baseline-on-curve) — the behavior every project
    /// authored before this field existed already renders.
    ///
    /// `skip_serializing_if` is load-bearing, not tidiness: the generated
    /// Python models carry `additionalProperties: false`, so a consumer pinned
    /// to a wheel that predates this field REJECTS a document containing it.
    /// Emitting the default would mean merely opening and saving an untouched
    /// project made it unreadable downstream. Omitting it keeps every existing
    /// document byte-identical on round-trip, so only a project that actually
    /// uses a non-default alignment carries the field — and those are authored
    /// after the wheel rolls out.
    #[serde(default, skip_serializing_if = "TextPathAlign::is_default")]
    pub align: TextPathAlign,
    /// Signed fine-tune in pixels along the path normal, added on top of
    /// [`Self::align`] (JRB-1819). Negative lifts the run above the curve.
    ///
    /// Animatable, so a run can be made to rise off or settle onto its guide.
    /// Also the escape hatch when a face's recorded cap height does not match
    /// what the eye wants.
    ///
    /// Skipped at its default for the same backward-reader reason as
    /// [`Self::align`].
    #[serde(default, skip_serializing_if = "is_zero_scalar")]
    pub align_offset: ScalarProperty,
}

/// Whether an authored [`ScalarProperty`] is still at its serde default, for
/// `skip_serializing_if` — see [`TextPathOptions::align`] for why omitting
/// defaults matters to downstream readers.
fn is_zero_scalar(value: &ScalarProperty) -> bool {
    *value == 0.0
}
