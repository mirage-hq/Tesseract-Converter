//! Canonical AE26 working cameras and their static property grammar.
//!
//! These are not user camera layers. AE rejects a project with the wrong number
//! of Default/Side/Custom cameras even when its timeline is empty. The recipes
//! below are an original interpretation of locally AE-authored AEP/AEPX pairs;
//! no specimen file, subtree, or reference-library builder is loaded here.

use crate::{
    rifx::{Chunk, RifxError},
    schema::{
        panel_records::EmptyListHeader,
        view_records::{StaticPropertyRecord, StaticPropertyValues, ViewLayerRecord},
    },
};

// All call sites are the literal non-LIST tags in this private AE26 recipe;
// neither a file nor caller input controls the tag.
fn raw(kind: [u8; 4], bytes: impl Into<Vec<u8>>) -> Chunk {
    Chunk::data(kind, bytes).expect("writer raw chunk identifiers are never LIST")
}

fn list(kind: [u8; 4], children: Vec<Chunk>) -> Chunk {
    Chunk::list(kind, children)
}

fn string(value: &str) -> Chunk {
    raw(*b"Utf8", value.as_bytes().to_vec())
}

pub(super) fn name_record(value: &str) -> Result<Chunk, RifxError> {
    if value.len() > 40 || !value.is_ascii() {
        return Err(RifxError::Invalid("invalid AE property match name"));
    }
    let mut bytes = [0; 40];
    bytes[..value.len()].copy_from_slice(value.as_bytes());
    Ok(raw(*b"tdmn", bytes))
}

pub(super) fn name_payload(value: &str) -> Result<Chunk, RifxError> {
    let size = u32::try_from(value.len()).map_err(|_| RifxError::Limit("property name"))?;
    let mut data = Vec::new();
    data.extend_from_slice(b"Utf8");
    data.extend_from_slice(&size.to_be_bytes());
    data.extend_from_slice(value.as_bytes());
    if size & 1 != 0 {
        data.push(0);
    }
    // Unlike tdgp, tdsn is a RAW chunk with one embedded Utf8 record.
    Ok(raw(*b"tdsn", data))
}

pub(super) fn group(
    discriminator: u32,
    display: &str,
    entries: Vec<(&str, Chunk)>,
) -> Result<Chunk, RifxError> {
    let mut children = vec![
        raw(*b"tdsb", discriminator.to_be_bytes()),
        name_payload(display)?,
    ];
    for (name, child) in entries {
        children.push(name_record(name)?);
        children.push(child);
    }
    children.push(name_record("ADBE Group End")?);
    Ok(list(*b"tdgp", children))
}

/// Vector contents are indexed collections, unlike their named child groups.
/// Independently authored AE files set the collection bit in `tdsb`.
pub(super) fn indexed_group(
    display: &str,
    entries: Vec<(&str, Chunk)>,
) -> Result<Chunk, RifxError> {
    group(0x401, display, entries)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ValueKind {
    Scalar,
    /// Keyed AE Time Remap has leaf flags 1; its static leaf has flags 3.
    TimeRemap,
    /// Native fixed-point plugin sliders/angles, not Transform scalar records.
    EffectScalar,
    /// Native floating-point plugin sliders have unrestricted animation flags.
    EffectFloat,
    /// Native integer plugin sliders use mode 4/subtype 4 with unrestricted flags.
    EffectInteger,
    /// Hidden native plugin sentinel, not a checkbox control.
    EffectRoot,
    /// Native checkbox/popup keys retain their discrete-control descriptor flags.
    EffectToggle,
    /// Plugin Color keys use native storage flags distinct from generic Color.
    EffectColor,
    Angle,
    Toggle,
    Spatial,
    Scale,
    Pair,
    /// Mask Feather/Opacity have native mode 4, subtype 6, not generic floats.
    MaskFeather,
    MaskOpacity,
    MaskExpansion,
    Color,
    /// Native two-component vector Size/Scale record.
    VectorPair,
    /// Native two-component spatial Position/Anchor record.
    VectorSpatial,
    /// Native bounded vector numeric record.
    VectorScalar,
    /// Native vector angular record.
    VectorAngle,
    /// Native bounded vector skew record.
    VectorSkew,
    /// Native vector enum record.
    VectorEnum,
    /// Range Selector Mode uses native enum flags with the discrete-mode bit.
    TextSelectorMode,
    /// Native vector color record.
    VectorColor,
    Orientation,
    Bevel,
    Refraction,
}

#[cfg(test)]
pub(super) fn property(
    kind: ValueKind,
    values: &[f64],
    bounds: Option<(f64, f64)>,
) -> Result<Chunk, RifxError> {
    property_with_animation(kind, values, bounds, None)
}

#[cfg(test)]
pub(super) fn property_with_animation(
    kind: ValueKind,
    values: &[f64],
    bounds: Option<(f64, f64)>,
    animation: Option<&super::keyframes::Track>,
) -> Result<Chunk, RifxError> {
    property_with_clock(
        kind,
        values,
        bounds,
        animation,
        super::keyframes::PropertyClock::DEFAULT,
    )
}

pub(super) fn property_with_clock(
    kind: ValueKind,
    values: &[f64],
    bounds: Option<(f64, f64)>,
    animation: Option<&super::keyframes::Track>,
    clock: super::keyframes::PropertyClock,
) -> Result<Chunk, RifxError> {
    property_with_scalar_units(kind, values, bounds, animation, clock, None)
}

pub(super) fn property_with_scalar_units(
    kind: ValueKind,
    values: &[f64],
    bounds: Option<(f64, f64)>,
    animation: Option<&super::keyframes::Track>,
    clock: super::keyframes::PropertyClock,
    scalar_units: Option<&[i32]>,
) -> Result<Chunk, RifxError> {
    if scalar_units.is_some() && (kind != ValueKind::Scalar || animation.is_none()) {
        return Err(RifxError::Invalid(
            "source ticks require animated scalar property",
        ));
    }
    property_with_native_units(kind, values, bounds, animation, clock, scalar_units, false)
}

pub(super) fn property_with_scale_units(
    values: &[f64],
    animation: Option<&super::keyframes::Track>,
    clock: super::keyframes::PropertyClock,
    scale_units: Option<&[i32]>,
) -> Result<Chunk, RifxError> {
    if scale_units.is_some() && animation.is_none() {
        return Err(RifxError::Invalid("source ticks require animated Scale"));
    }
    property_with_native_units(
        ValueKind::Scale,
        values,
        Some((0.0, 0.0)),
        animation,
        clock,
        scale_units,
        true,
    )
}

pub(super) fn property_with_follower_units(
    values: &[f64],
    bounds: Option<(f64, f64)>,
    animation: Option<&super::keyframes::Track>,
    clock: super::keyframes::PropertyClock,
    units: Option<&[i32]>,
) -> Result<Chunk, RifxError> {
    if units.is_some() && animation.is_none() {
        return Err(RifxError::Invalid(
            "source ticks require animated Position follower",
        ));
    }
    property_with_native_units(
        ValueKind::Scalar,
        values,
        bounds,
        animation,
        clock,
        units,
        true,
    )
}

fn property_with_native_units(
    kind: ValueKind,
    values: &[f64],
    bounds: Option<(f64, f64)>,
    animation: Option<&super::keyframes::Track>,
    clock: super::keyframes::PropertyClock,
    units: Option<&[i32]>,
    allow_hold: bool,
) -> Result<Chunk, RifxError> {
    // Descriptor variants and value-slot counts agree across the independent
    // AE26 empty, resized, duration and solid cases. Extra slots are zeroed
    // static-value/tangent defaults, not a copied payload.
    let (components, selection, variant, flags, mode, subtype, spatial, slots) = match kind {
        ValueKind::Scalar | ValueKind::TimeRemap | ValueKind::Angle => {
            (1, 1, 0, 0x1ffff, 8, 9, false, 5)
        }
        ValueKind::EffectScalar => (1, 1, 0, u32::MAX, 4, 6, false, 5),
        ValueKind::EffectFloat => (1, 1, 0, u32::MAX, 8, 9, false, 5),
        ValueKind::EffectInteger => (1, 1, 0, u32::MAX, 4, 4, false, 5),
        ValueKind::EffectToggle => (1, 1, 0, 0x10004, 4, 4, false, 5),
        ValueKind::Toggle => (1, 1, 0, 0xffff0004, 4, 4, false, 5),
        ValueKind::EffectRoot => (1, 1, 0, 0x10000, 4, 4, false, 5),
        ValueKind::Spatial => (3, 15, 3, u32::MAX, 8, 9, true, 9),
        ValueKind::Scale => (3, 1, 0, 0x1ffff, 8, 9, false, 15),
        ValueKind::Pair | ValueKind::VectorPair => (2, 1, 0, u32::MAX, 8, 9, false, 10),
        ValueKind::MaskFeather => (2, 1, 0, u32::MAX, 4, 6, false, 10),
        ValueKind::MaskOpacity => (1, 1, 0, u32::MAX, 4, 6, false, 5),
        ValueKind::Color | ValueKind::EffectColor | ValueKind::VectorColor => {
            (4, 7, 0, 0x2ffff, 1, 1, false, 12)
        }
        ValueKind::VectorSpatial => (2, 15, 3, u32::MAX, 8, 9, false, 6),
        ValueKind::VectorScalar => (1, 1, 0, u32::MAX, 4, 8, false, 5),
        ValueKind::VectorAngle => (1, 1, 0, 0x2ffff, 8, 9, false, 5),
        ValueKind::VectorSkew | ValueKind::MaskExpansion => (1, 1, 0, u32::MAX, 8, 9, false, 5),
        ValueKind::VectorEnum => (1, 1, 0, 0x20000, 4, 4, false, 5),
        ValueKind::TextSelectorMode => (1, 1, 0, 0x20004, 4, 4, false, 5),
        ValueKind::Orientation => (1, 7, 0, 0x60007, 0x10018, 0, false, 3),
        ValueKind::Bevel => (1, 1, 0, 0x20000, 4, 4, false, 5),
        ValueKind::Refraction => (1, 1, 0, u32::MAX, 8, 9, false, 5),
    };
    if values.len() > slots {
        return Err(RifxError::Invalid("too many static value slots"));
    }
    let mut descriptor = StaticPropertyRecord::new(
        components, selection, variant, flags, mode, subtype, spatial,
    );
    if matches!(
        kind,
        ValueKind::Color
            | ValueKind::EffectColor
            | ValueKind::VectorColor
            | ValueKind::VectorSpatial
            | ValueKind::Orientation
    ) {
        descriptor.set_initialized();
    }
    let mut padded = vec![0.0; slots];
    padded[..values.len()].copy_from_slice(values);
    let mut descriptor_bytes = descriptor.encode();
    descriptor_bytes[12..16].copy_from_slice(&clock.ticks().to_be_bytes());
    if kind == ValueKind::EffectRoot {
        descriptor_bytes[72] = 128;
    }
    if animation.is_some() {
        descriptor_bytes = match kind {
            ValueKind::MaskFeather | ValueKind::MaskOpacity | ValueKind::EffectScalar => {
                super::keyframes::animated_vector_descriptor(descriptor_bytes, 0, 0, u32::MAX, 4, 6)
            }
            ValueKind::VectorPair | ValueKind::EffectFloat => {
                super::keyframes::animated_vector_descriptor(descriptor_bytes, 0, 0, u32::MAX, 8, 9)
            }
            ValueKind::EffectInteger => {
                super::keyframes::animated_vector_descriptor(descriptor_bytes, 0, 0, u32::MAX, 4, 4)
            }
            ValueKind::EffectToggle => {
                super::keyframes::animated_vector_descriptor(descriptor_bytes, 0, 0, 0x10004, 4, 4)
            }
            ValueKind::VectorSpatial => super::keyframes::animated_vector_descriptor(
                descriptor_bytes,
                14,
                3,
                u32::MAX,
                8,
                9,
            ),
            ValueKind::VectorScalar => {
                super::keyframes::animated_vector_descriptor(descriptor_bytes, 0, 0, u32::MAX, 4, 8)
            }
            ValueKind::VectorAngle => {
                super::keyframes::animated_vector_descriptor(descriptor_bytes, 0, 0, 0x2ffff, 8, 9)
            }
            ValueKind::VectorSkew | ValueKind::MaskExpansion => {
                super::keyframes::animated_vector_descriptor(descriptor_bytes, 0, 0, u32::MAX, 8, 9)
            }
            ValueKind::VectorEnum => {
                super::keyframes::animated_vector_descriptor(descriptor_bytes, 0, 0, 0x20000, 4, 4)
            }
            ValueKind::TextSelectorMode => {
                super::keyframes::animated_vector_descriptor(descriptor_bytes, 0, 0, 0x20004, 4, 4)
            }
            ValueKind::VectorColor => {
                super::keyframes::animated_vector_descriptor(descriptor_bytes, 6, 0, 0x2ffff, 1, 1)
            }
            ValueKind::Color | ValueKind::EffectColor => {
                super::keyframes::animated_color_descriptor(descriptor_bytes)
            }
            _ => super::keyframes::animated_descriptor(descriptor_bytes, spatial),
        };
    }
    let native_vector = matches!(
        kind,
        ValueKind::EffectScalar
            | ValueKind::EffectFloat
            | ValueKind::EffectInteger
            | ValueKind::EffectToggle
            | ValueKind::EffectColor
            | ValueKind::MaskFeather
            | ValueKind::MaskOpacity
            | ValueKind::MaskExpansion
            | ValueKind::VectorPair
            | ValueKind::VectorSpatial
            | ValueKind::VectorScalar
            | ValueKind::VectorAngle
            | ValueKind::VectorSkew
            | ValueKind::VectorEnum
            | ValueKind::TextSelectorMode
            | ValueKind::VectorColor
    );
    let mut children = vec![
        raw(
            *b"tdsb",
            (if native_vector
                || matches!(kind, ValueKind::Spatial | ValueKind::Angle)
                || (kind == ValueKind::TimeRemap && animation.is_some())
            {
                1_u32
            } else {
                3
            })
            .to_be_bytes(),
        ),
        name_payload("-_0_/-")?,
        raw(*b"tdb4", descriptor_bytes),
    ];
    if let Some(track) = animation {
        let keyframes = if let Some(units) = units {
            match kind {
                ValueKind::Scalar if allow_hold => {
                    super::keyframes::scalar_step_list_with_units(track, units)?
                }
                ValueKind::Scalar => super::keyframes::linear_scalar_list_with_units(track, units)?,
                ValueKind::Scale => super::keyframes::scale_list_with_units(track, units)?,
                _ => {
                    return Err(RifxError::Invalid(
                        "source ticks require Scale or scalar property",
                    ));
                }
            }
        } else if matches!(kind, ValueKind::VectorSpatial) {
            let spatial_track = super::keyframes::spatial_2d_track(track)?;
            super::keyframes::list_with_clock(&spatial_track, usize::from(components), true, clock)?
        } else if matches!(kind, ValueKind::VectorColor) {
            super::keyframes::vector_color_list_with_clock(track, clock)?
        } else if matches!(kind, ValueKind::EffectColor) {
            match super::keyframes::effect_color_list_with_clock(track, clock) {
                Ok(keys) => keys,
                // Lowering validates the default clock, but the owning composition
                // can overflow or collide at its final clock. Keep the authored
                // static control rather than aborting otherwise convertible content.
                Err(_) => return property_with_clock(kind, values, bounds, None, clock),
            }
        } else if matches!(kind, ValueKind::Color) {
            super::keyframes::color_list_with_clock(track, clock)?
        } else {
            super::keyframes::list_with_clock(track, usize::from(components), spatial, clock)?
        };
        children.push(keyframes);
    } else {
        children.push(raw(*b"cdat", StaticPropertyValues::new(padded)?.encode()));
    }
    if let Some((min, max)) = bounds {
        children.push(raw(*b"tdum", min.to_be_bytes()));
        children.push(raw(*b"tduM", max.to_be_bytes()));
    }
    Ok(list(*b"tdbs", children))
}

fn transform(
    center: [f64; 3],
    position: Option<[f64; 3]>,
    side: bool,
    marker: bool,
    clock: super::keyframes::PropertyClock,
) -> Result<Chunk, RifxError> {
    let property =
        |kind, values: &[f64], bounds| property_with_clock(kind, values, bounds, None, clock);
    let scalar = |value, bounds| property(ValueKind::Scalar, &[value], bounds);
    let mut entries = Vec::new();
    if !marker {
        entries.push((
            "ADBE Anchor Point",
            property(ValueKind::Spatial, &center, None)?,
        ));
    }
    if let Some(position) = position {
        entries.push((
            "ADBE Position",
            property(ValueKind::Spatial, &position, None)?,
        ));
    }
    entries.push(("ADBE Position_0", scalar(0.0, Some((0.0, 0.0)))?));
    entries.push(("ADBE Position_1", scalar(0.0, Some((0.0, 0.0)))?));
    if marker {
        let orientation = list(
            *b"otst",
            vec![
                property(ValueKind::Orientation, &[0.0; 3], None)?,
                list(*b"otky", vec![raw(*b"otda", [0; 24])]),
            ],
        );
        entries.push(("ADBE Orientation", orientation));
        entries.push(("ADBE Rotate X", scalar(0.0, None)?));
        entries.push(("ADBE Rotate Y", scalar(0.0, None)?));
    } else {
        entries.push(("ADBE Position_2", scalar(0.0, Some((0.0, 0.0)))?));
        if !side {
            entries.push((
                "ADBE Scale",
                property(ValueKind::Scale, &[1.0; 3], Some((0.0, 0.0)))?,
            ));
        }
        entries.push(("ADBE Rotate Z", property(ValueKind::Angle, &[0.0], None)?));
        entries.push(("ADBE Opacity", scalar(1.0, Some((0.0, 100.0)))?));
    }
    entries.push((
        "ADBE Envir Appear in Reflect",
        property(ValueKind::Toggle, &[1.0], None)?,
    ));
    group(1, "-_0_/-", entries)
}

fn camera_options(clock: super::keyframes::PropertyClock) -> Result<Chunk, RifxError> {
    let property =
        |kind, values: &[f64], bounds| property_with_clock(kind, values, bounds, None, clock);
    let scalar = |value, bounds| property(ValueKind::Scalar, &[value], bounds);
    group(
        1,
        "-_0_/-",
        vec![
            (
                "ADBE Camera Focus Area Width",
                scalar(0.0, Some((0.0, 0.0)))?,
            ),
            (
                "ADBE Camera Split Blur Level",
                property(ValueKind::Pair, &[100.0, 100.0], Some((-32000.0, 32000.0)))?,
            ),
        ],
    )
}

fn marker_properties(
    center: [f64; 3],
    clock: super::keyframes::PropertyClock,
) -> Result<Chunk, RifxError> {
    let property =
        |kind, values: &[f64], bounds| property_with_clock(kind, values, bounds, None, clock);
    let scalar = |value, bounds| property(ValueKind::Scalar, &[value], bounds);
    let mut styles = vec![(
        "ADBE Blend Options Group",
        group(
            3,
            "-_0_/-",
            vec![("ADBE Adv Blend Group", group(1, "-_0_/-", vec![])?)],
        )?,
    )];
    for name in [
        "dropShadow/enabled",
        "innerShadow/enabled",
        "outerGlow/enabled",
        "innerGlow/enabled",
        "bevelEmboss/enabled",
        "chromeFX/enabled",
        "solidFill/enabled",
        "gradientFill/enabled",
        "patternFill/enabled",
        "frameFX/enabled",
    ] {
        styles.push((name, group(2, "-_0_/-", vec![])?));
    }
    let mut material = vec![
        (
            "ADBE Casts Shadows",
            property(ValueKind::Toggle, &[0.0], None)?,
        ),
        ("ADBE Light Transmission", scalar(0.0, Some((0.0, 100.0)))?),
        (
            "ADBE Accepts Shadows",
            property(ValueKind::Toggle, &[1.0], None)?,
        ),
        (
            "ADBE Accepts Lights",
            property(ValueKind::Toggle, &[1.0], None)?,
        ),
        (
            "ADBE Shadow Color",
            property(ValueKind::Color, &[255.0, 0.0, 0.0, 0.0], None)?,
        ),
        (
            "ADBE Appears in Reflections",
            property(ValueKind::Toggle, &[1.0], None)?,
        ),
    ];
    for (name, value) in [
        ("ADBE Ambient Coefficient", 100.0),
        ("ADBE Diffuse Coefficient", 50.0),
        ("ADBE Specular Coefficient", 50.0),
        ("ADBE Shininess Coefficient", 5.0),
        ("ADBE Metal Coefficient", 100.0),
        ("ADBE Reflection Coefficient", 0.0),
        ("ADBE Glossiness Coefficient", 100.0),
        ("ADBE Fresnel Coefficient", 0.0),
        ("ADBE Transparency Coefficient", 0.0),
        ("ADBE Transp Rolloff", 0.0),
    ] {
        material.push((name, scalar(value, Some((0.0, 100.0)))?));
    }
    material.push((
        "ADBE Index of Refraction",
        property(ValueKind::Refraction, &[1.0], Some((1.0, 2.0)))?,
    ));
    group(
        1,
        "",
        vec![
            (
                "ADBE Transform Group",
                transform(center, None, false, true, clock)?,
            ),
            ("ADBE Layer Styles", group(3, "-_0_/-", styles)?),
            (
                "ADBE Extrsn Options Group",
                group(
                    3,
                    "-_0_/-",
                    vec![(
                        "ADBE Bevel Direction",
                        property(ValueKind::Bevel, &[1.0], None)?,
                    )],
                )?,
            ),
            ("ADBE Material Options Group", group(3, "-_0_/-", material)?),
            ("ADBE Audio Group", group(3, "-_0_/-", vec![])?),
            ("ADBE Layer Sets", group(3, "-_0_/-", vec![])?),
        ],
    )
}

fn guides() -> Chunk {
    // Empty AE26 guide list header: signature, version, element stride and
    // empty allocation/count fields. No guide objects or fixture bytes.
    let header = EmptyListHeader::guides().encode();
    list(
        *b"Gide",
        vec![
            raw(*b"gdta", [1, 0, 1, 0, 0, 0, 0, 0]),
            list(*b"list", vec![raw(*b"lhd3", header)]),
        ],
    )
}

fn panel_settings(kind: [u8; 4]) -> Chunk {
    list(
        kind,
        vec![
            list(
                *b"CpS2",
                vec![
                    raw(*b"CsCt", 0x01000000_u32.to_be_bytes()),
                    string("Untitled"),
                    string("en_US"),
                ],
            ),
            list(
                *b"CapS",
                vec![
                    raw(*b"CsCt", 0x01000000_u32.to_be_bytes()),
                    raw(*b"CapL", 0_u32.to_be_bytes()),
                    string("Untitled"),
                ],
            ),
            raw(*b"CPTm", 1_u64.to_be_bytes()),
            raw(*b"CROI", 0_u64.to_be_bytes()),
            raw(*b"CcCt", 0_u32.to_be_bytes()),
        ],
    )
}

/// Canonical working views, not timeline cameras. IDs 2..12 are reserved by
/// the root constructor; all view durations use the composition timebase.
pub(super) fn build_views(
    width: u16,
    height: u16,
    duration: crate::timing::Duration24,
) -> Result<Vec<Chunk>, super::AepWriteError> {
    build_views_with_clock(
        width,
        height,
        duration,
        super::keyframes::PropertyClock::DEFAULT,
    )
}

pub(super) fn build_views_with_clock(
    width: u16,
    height: u16,
    duration: crate::timing::Duration24,
    clock: super::keyframes::PropertyClock,
) -> Result<Vec<Chunk>, super::AepWriteError> {
    let center = [f64::from(width) / 2.0, f64::from(height) / 2.0, 0.0];
    // Canonical custom perspective offsets. These need not retain the exact
    // UI rounding of AE's saved defaults; they must describe valid finite views.
    let custom_distance = f64::from(width) * 25.0 / 18.0;
    let positions = [
        None,
        Some([center[0], center[1], -5000.0]),
        Some([center[0] - 5000.0, center[1], 0.0]),
        Some([center[0], center[1] - 5000.0, 0.0]),
        Some([center[0], center[1], 5000.0]),
        Some([center[0] + 5000.0, center[1], 0.0]),
        Some([center[0], center[1] + 5000.0, 0.0]),
        Some([
            center[0] - custom_distance,
            center[1] - custom_distance,
            -custom_distance,
        ]),
        Some([center[0], center[1] - custom_distance, -custom_distance]),
        Some([
            center[0] + custom_distance,
            center[1] - custom_distance,
            -custom_distance,
        ]),
        None,
    ];
    let names = [
        "Default",
        "Front",
        "Left",
        "Top",
        "Back",
        "Right",
        "Bottom",
        "Custom View 1",
        "Custom View 2",
        "Custom View 3",
        "Markers",
    ];
    let mut result = Vec::new();
    for (index, (name, position)) in names.into_iter().zip(positions).enumerate() {
        let marker = index == 10;
        let side = (1..=6).contains(&index);
        let kind = if marker {
            *b"SecL"
        } else if side {
            *b"SLay"
        } else if index == 0 {
            *b"DLay"
        } else {
            *b"CLay"
        };
        let properties = if marker {
            marker_properties(center, clock)?
        } else {
            group(
                1,
                "",
                vec![
                    (
                        "ADBE Transform Group",
                        transform(center, position, side, false, clock)?,
                    ),
                    ("ADBE Camera Options Group", camera_options(clock)?),
                ],
            )?
        };
        let id = u32::try_from(index).map_err(|_| RifxError::Limit("view ID"))? + 2;
        let metadata = ViewLayerRecord::new(id, name, duration, marker, side)?;
        result.push(list(
            kind,
            vec![
                raw(*b"ldta", metadata.encode()),
                string(name),
                properties,
                list(
                    *b"GdV2",
                    vec![string(""), raw(*b"GdCt", 0_u32.to_be_bytes())],
                ),
                guides(),
            ],
        ));
        result.push(list(*b"Ewst", vec![]));
        for _ in 0..2 {
            result.extend([
                raw(*b"fvdv", 3_u32.to_be_bytes()),
                raw(*b"fiop", vec![0]),
                raw(*b"ftts", 0_u32.to_be_bytes()),
                raw(*b"foac", vec![0]),
                raw(*b"fiac", vec![0]),
                raw(*b"fipc", 0_u16.to_be_bytes()),
                raw(*b"fifl", 0_u32.to_be_bytes()),
            ]);
        }
    }
    result.extend([
        panel_settings(*b"CIFO"),
        panel_settings(*b"CIF2"),
        panel_settings(*b"CIF3"),
        list(
            *b"GdV2",
            vec![string(""), raw(*b"GdCt", 0_u32.to_be_bytes())],
        ),
        guides(),
    ]);
    Ok(result)
}

#[cfg(test)]
mod effect_color_tests {
    #[test]
    fn effect_color_final_clock_overflow_retains_static_control() {
        use super::super::keyframes::{Easing, Keyframe, PropertyClock, Track};
        let values = [0.0, 25.5, 51.0, 76.5];
        let track = Track {
            keys: [0, 50_000_000]
                .into_iter()
                .map(|time_millis| Keyframe {
                    time_millis,
                    values: values.to_vec(),
                    easing: vec![Easing::Linear; 4],
                    spatial_in: Vec::new(),
                    spatial_out: Vec::new(),
                })
                .collect(),
        };
        assert!(
            super::super::keyframes::effect_color_list_with_clock(&track, PropertyClock::DEFAULT,)
                .is_ok()
        );
        let clock = PropertyClock::for_rate(crate::timing::FrameRate::new(60.0).unwrap()).unwrap();
        assert!(super::super::keyframes::effect_color_list_with_clock(&track, clock).is_err());
        let actual = super::property_with_clock(
            super::ValueKind::EffectColor,
            &values,
            None,
            Some(&track),
            clock,
        )
        .unwrap();
        let expected =
            super::property_with_clock(super::ValueKind::EffectColor, &values, None, None, clock)
                .unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn thirty_fps_working_views_have_no_legacy_property_descriptors() {
        fn clocks(chunks: &[crate::rifx::Chunk], output: &mut Vec<u32>) {
            for chunk in chunks {
                if chunk.id() == *b"tdb4" {
                    let bytes = chunk.data_payload().unwrap();
                    output.push(u32::from_be_bytes(bytes[12..16].try_into().unwrap()));
                }
                if let Some(children) = chunk.children() {
                    clocks(children, output);
                }
            }
        }
        let views = super::build_views_with_clock(
            640,
            480,
            crate::timing::Duration24::from_frames(60).unwrap(),
            super::super::keyframes::PropertyClock::for_rate(
                crate::timing::FrameRate::new(30.0).unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        let mut descriptors = Vec::new();
        clocks(&views, &mut descriptors);
        assert!(descriptors.len() > 50, "inspect working view descriptors");
        assert!(descriptors.iter().all(|clock| *clock == 30_720));
    }

    #[test]
    fn authored_c17_hold_times_use_owning_thirty_fps_property_clock() {
        // Independently authored comp 17 has Hold Scale at 250, 1100 and
        // 1650 ms (export_repairs/native-controls.aep). Compare raw wire
        // units: reader millisecond rounding hides the frame-33 regression.
        use super::super::keyframes::{Easing, Keyframe, PropertyClock, Track};
        let rate = crate::timing::FrameRate::new(30.0).unwrap();
        let track = Track {
            keys: [250, 1100, 1650]
                .into_iter()
                .map(|time_millis| Keyframe {
                    time_millis,
                    values: vec![100.0, 100.0, 100.0],
                    easing: vec![Easing::Hold; 3],
                    spatial_in: vec![],
                    spatial_out: vec![],
                })
                .collect(),
        };
        let property = super::property_with_clock(
            super::ValueKind::Scale,
            &[100.0, 100.0, 100.0],
            None,
            Some(&track),
            PropertyClock::for_rate(rate).unwrap(),
        )
        .unwrap();
        let children = property.children().unwrap();
        let descriptor = crate::properties::data(children, *b"tdb4").unwrap();
        assert_eq!(
            u32::from_be_bytes(descriptor[12..16].try_into().unwrap()),
            30_720
        );
        let key_list = crate::properties::unique_list(children, *b"list").unwrap();
        let data = crate::properties::data(key_list, *b"ldat").unwrap();
        let stride = data.len() / 3;
        assert_eq!(
            (0..3)
                .map(|i| i32::from_be_bytes(data[i * stride..i * stride + 4].try_into().unwrap()))
                .collect::<Vec<_>>(),
            [7680, 33792, 50688]
        );
    }
    use super::*;
    use crate::writer::keyframes::{Easing, Keyframe, Track};

    #[test]
    fn animated_color_descriptor_and_flags_match_independent_adobe_ramp() {
        fn parameter<'a>(chunks: &'a [Chunk], name: &[u8]) -> Option<&'a Chunk> {
            for pair in chunks.windows(2) {
                if pair[0].id() == *b"tdmn"
                    && pair[1].list_kind() == Some(*b"tdbs")
                    && pair[0].data_payload()?.split(|byte| *byte == 0).next() == Some(name)
                {
                    return Some(&pair[1]);
                }
            }
            chunks
                .iter()
                .find_map(|chunk| parameter(chunk.children()?, name))
        }
        let project = crate::structure::read_project(include_bytes!(
            "../../tests/fixtures/effects/animated_catalog.aep"
        ))
        .unwrap();
        let crate::structure::ItemKind::Composition(comp) = &project.item(422).unwrap().kind else {
            panic!("pinned native Ramp composition");
        };
        let native = parameter(&comp.layers[0].content, b"ADBE Ramp-0002").unwrap();
        let track = Track {
            keys: [0, 1000]
                .into_iter()
                .map(|time_millis| Keyframe {
                    time_millis,
                    values: vec![255.0, 0.0, 0.0, 0.0],
                    easing: vec![Easing::Linear; 4],
                    spatial_in: Vec::new(),
                    spatial_out: Vec::new(),
                })
                .collect(),
        };
        let generated = property_with_animation(
            ValueKind::Color,
            &[255.0, 0.0, 0.0, 0.0],
            None,
            Some(&track),
        )
        .unwrap();
        let native = native.children().unwrap();
        let fresh = generated.children().unwrap();
        assert_eq!(
            crate::properties::data(fresh, *b"tdb4").unwrap(),
            crate::properties::data(native, *b"tdb4").unwrap()
        );
        let native_list = crate::properties::unique_list(native, *b"list").unwrap();
        let fresh_list = crate::properties::unique_list(fresh, *b"list").unwrap();
        assert_eq!(
            &crate::properties::data(fresh_list, *b"ldat").unwrap()[..8],
            &crate::properties::data(native_list, *b"ldat").unwrap()[..8]
        );
    }
}
