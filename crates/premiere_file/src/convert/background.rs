//! Preserve explicit black canvases without assuming the FX document background.
use crate::error::{unsupported, Result};
use fx_schema::{
    BlendMode, GroupLayer, Layer, LayerId, NonNegativeProperty, PercentageProperty, RectLayer,
    RectShape, TimeRangeProperty, Transform,
};

/// A visible, unblended group at the document root that holds `layers` as
/// they are: no mask, matte, playback, effects or box model (padding, fills
/// and corner radii, which only a converted graphic uses). Callers set the
/// fields that their placement carries.
pub(crate) fn plain_group(
    id: LayerId,
    name: String,
    active_range: TimeRangeProperty,
    transform: Transform,
    layers: Vec<Layer>,
) -> Result<GroupLayer> {
    Ok(GroupLayer {
        id,
        name,
        description: String::new(),
        is_hidden: false,
        parent: None,
        blend_mode: BlendMode::Normal,
        track_matte: None,
        masks: Vec::new(),
        playback: fx_schema::LayerPlayback::linear(
            active_range,
            active_range,
            TimeRangeProperty::new(fx_schema::Time::ZERO, active_range.duration),
            0,
        )
        .map_err(unsupported)?,
        effects: Vec::new(),
        motion_blur: false,
        padding_top: NonNegativeProperty::default(),
        padding_right: NonNegativeProperty::default(),
        padding_bottom: NonNegativeProperty::default(),
        padding_left: NonNegativeProperty::default(),
        fills: Vec::new(),
        corner_radius_top_left: NonNegativeProperty::default(),
        corner_radius_top_right: NonNegativeProperty::default(),
        corner_radius_bottom_right: NonNegativeProperty::default(),
        corner_radius_bottom_left: NonNegativeProperty::default(),
        transform,
        layers,
    })
}

pub(crate) fn identity_transform() -> Transform {
    Transform {
        anchor_point: [0.0, 0.0],
        position: fx_schema::Position::xy(0.0, 0.0),
        scale: [100.0, 100.0],
        rotation: 0.0,
        skew: 0.0,
        skew_axis: 0.0,
        rotation_x: 0.0,
        rotation_y: 0.0,
        orientation: [0.0; 3],
        opacity: PercentageProperty::new(100.0).expect("100 is a valid percentage"),
    }
}

pub(crate) fn black_shape(width: u32, height: u32) -> RectShape {
    RectShape {
        size: [width.into(), height.into()],
        position: [0.0, 0.0],
        roundness: 0.0,
        fill_enabled: true,
        fill_color: [0.0, 0.0, 0.0, 1.0],
        fill_paint: None,
        fill_blend_mode: None,
        stroke_enabled: false,
        stroke_color: None,
        stroke_width: Default::default(),
        stroke_dashes: Vec::new(),
        stroke_dash_offset: 0.0,
        stroke_join: Default::default(),
        stroke_miter_limit: 4.0,
    }
}

/// Validate an actual bottommost opaque rectangle, independently of its label.
/// The extracted rectangle uses Premiere's native black background instead of
/// an emitted clip. Its declared clock must still be representable.
pub(crate) fn validate_black_canvas(
    actual: &RectLayer,
    width: u32,
    height: u32,
) -> Result<std::ops::Range<i64>> {
    let expected = RectLayer {
        id: actual.id,
        name: actual.name.clone(),
        description: actual.description.clone(),
        active_range: actual.active_range,
        is_hidden: false,
        parent: None,
        blend_mode: BlendMode::Normal,
        track_matte: None,
        masks: Vec::new(),
        effects: Vec::new(),
        motion_blur: false,
        transform: identity_transform(),
        rect: black_shape(width, height),
    };
    if actual != &expected {
        return Err(unsupported(
            "bottom rectangle must be an unmodified opaque full-frame black canvas",
        ));
    }
    let end = actual
        .active_range
        .start
        .checked_add_duration(actual.active_range.duration)
        .ok_or_else(|| unsupported("black canvas range overflows"))?;
    let start = super::timing::ticks_from_time(actual.active_range.start, "black canvas start")?;
    let end = super::timing::ticks_from_time(end, "black canvas end")?;
    Ok(start..end)
}
