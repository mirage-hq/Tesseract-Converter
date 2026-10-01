//! Bounded Fractal Noise approximations through existing editable FX effects.
//! Fractal's ABI differs from Turbulent Noise; popup defaults are not cache values.
use std::collections::{HashMap, HashSet};

use fx_schema::{
    LayerEffect,
    effect::{FractalType, NoiseType},
};

use super::native::DecodedEffect;
use crate::{
    properties,
    rifx::Chunk,
    structure::{Layer, ProjectItem},
};

pub(crate) const MATCH_NAME: &str = "ADBE Fractal Noise";

struct Controls<'a> {
    rows: Vec<(&'a str, &'a [Chunk])>,
}

// (native kind, default scalar/first point coordinate, popup choice count).
// The pinned complete parT contains a cached Blend value 5, but default 2.
const DEFAULTS: [(u32, u32, u16); 32] = [
    (0, 0, 0),
    (7, 1, 20),
    (7, 3, 4),
    (4, 0, 0),
    (2, 100 << 16, 0),
    (2, 0, 0),
    (7, 4, 4),
    (13, 0, 0),
    (3, 0, 0),
    (4, 1, 0),
    (2, 100 << 16, 0),
    (2, 100 << 16, 0),
    (2, 100 << 16, 0),
    (6, 50 << 16, 0),
    (14, 0, 0),
    (2, 6 << 16, 0),
    (13, 0, 0),
    (2, 70 << 16, 0),
    (2, 56 << 16, 0),
    (3, 0, 0),
    (6, 0, 0),
    (4, 0, 0),
    (14, 0, 0),
    (3, 0, 0),
    (13, 0, 0),
    (4, 0, 0),
    (1, 1, 0),
    (1, 0, 0),
    (14, 0, 0),
    (2, 100 << 16, 0),
    (7, 2, 21),
    (4, 0, 0),
];

fn suffix(name: &str) -> Option<usize> {
    let number = name.strip_prefix("ADBE Fractal Noise-")?;
    (number.len() == 4 && number.bytes().all(|b| b.is_ascii_digit()))
        .then(|| number.parse().ok())
        .flatten()
        .filter(|n| *n < DEFAULTS.len())
}

impl<'a> Controls<'a> {
    fn read(layer: &'a Layer, source: &DecodedEffect) -> Result<Self, String> {
        let roots = properties::root_runs(&layer.content).map_err(|e| e.to_string())?;
        let parade = roots
            .iter()
            .filter(|(name, _)| *name == "ADBE Effect Parade")
            .collect::<Vec<_>>();
        let [(_, parade)] = parade.as_slice() else {
            return Err("requires one Effect Parade".into());
        };
        let effects =
            properties::runs(properties::unique_list(parade, *b"tdgp").map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        let (name, run) = effects
            .get(
                source
                    .index
                    .checked_sub(1)
                    .ok_or("invalid effect ordinal")?,
            )
            .ok_or("effect absent")?;
        if *name != MATCH_NAME {
            return Err("effect ordinal/name mismatch".into());
        }
        let descriptor = properties::unique_list(run, *b"sspc").map_err(|e| e.to_string())?;
        let mut warnings = Vec::new();
        properties::group_enabled_or_warn(descriptor, MATCH_NAME, &mut warnings);
        if !warnings.is_empty() {
            return Err(warnings.join("; "));
        }
        let table = properties::unique_list(descriptor, *b"parT").map_err(|e| e.to_string())?;
        if !table.is_empty() {
            let rows = properties::runs(table).map_err(|e| e.to_string())?;
            if rows.len() != DEFAULTS.len() + 1 {
                return Err(
                    "requires complete native Fractal declarations or sparse instance".into(),
                );
            }
            let mut seen = HashSet::new();
            for (name, run) in rows {
                if !seen.insert(name) {
                    return Err("duplicate Fractal declaration".into());
                }
                let (kind, default, choices) = if name == "ADBE Effect Built In Params" {
                    (9, 0, 0)
                } else {
                    DEFAULTS[suffix(name).ok_or("unknown Fractal declaration")?]
                };
                let bytes = properties::data(run, *b"pard").map_err(|e| e.to_string())?;
                if bytes.len() != 148
                    || bytes[12..16] != kind.to_be_bytes()
                    || (kind == 7
                        && (bytes[60..62] != choices.to_be_bytes()
                            || bytes[62..64] != (default as u16).to_be_bytes()))
                    || (kind != 7 && bytes[56..60] != default.to_be_bytes())
                    || (kind == 6 && bytes[60..64] != default.to_be_bytes())
                {
                    return Err(
                        "Fractal declarations conflict with validated native defaults".into(),
                    );
                }
            }
        }
        let rows = properties::runs(
            properties::unique_list(descriptor, *b"tdgp").map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let mut seen = HashSet::new();
        for (name, run) in &rows {
            if !seen.insert(*name) {
                return Err("duplicate Fractal control".into());
            }
            if *name == "ADBE Effect Built In Params" {
                let options = properties::runs(
                    properties::unique_list(run, *b"tdgp").map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?;
                if options.iter().any(|(name, _)| *name != "ADBE Group End") {
                    return Err("nonempty Fractal compositing options".into());
                }
            } else if *name != "ADBE Group End" && suffix(name).is_none() {
                return Err(format!("unknown Fractal control {name}"));
            }
        }
        let result = Self { rows };
        if result.scalar(0, 0.)? != 0. {
            return Err("nondefault Fractal dummy control".into());
        }
        Ok(result)
    }

    fn leaf(&self, number: usize) -> Result<Option<&[Chunk]>, String> {
        let Some((_, run)) = self
            .rows
            .iter()
            .find(|(name, _)| suffix(name) == Some(number))
        else {
            return Ok(None);
        };
        properties::unique_list(run, *b"tdbs")
            .map(Some)
            .map_err(|e| e.to_string())
    }

    fn numeric(&self, number: usize) -> Result<Option<properties::NumericProperty>, String> {
        let Some(leaf) = self.leaf(number)? else {
            return Ok(None);
        };
        let numeric = if DEFAULTS[number].0 == 6 {
            properties::read_effect_point(leaf)
        } else {
            properties::read_numeric(leaf)
        }
        .map_err(|e| e.to_string())?;
        if numeric.animated
            || !numeric.keyframes.is_empty()
            || numeric.expression_present
            || numeric.expression_enabled
            || numeric.dimensions_separated
            || numeric.values.iter().any(|v| !v.is_finite())
        {
            return Err(format!(
                "Fractal control {number:04}: requires finite static value without expression"
            ));
        }
        Ok(Some(numeric))
    }

    fn scalar(&self, number: usize, default: f64) -> Result<f64, String> {
        let Some(numeric) = self.numeric(number)? else {
            return Ok(default);
        };
        let [value] = numeric.values.as_slice() else {
            return Err(format!("Fractal control {number:04}: requires scalar"));
        };
        Ok(*value)
    }

    fn point(&self, number: usize, default: [f64; 2], size: [u16; 2]) -> Result<[f64; 2], String> {
        let Some(numeric) = self.numeric(number)? else {
            return Ok(default);
        };
        let [x, y] = numeric.values.as_slice() else {
            return Err("Fractal point requires two coordinates".into());
        };
        let leaf = self.leaf(number)?.ok_or("Fractal point leaf missing")?;
        let relative =
            properties::data(leaf, *b"tdb4").is_ok_and(|meta| meta.len() == 124 && meta[59] == 4);
        Ok(if relative {
            [x * f64::from(size[0]), y * f64::from(size[1])]
        } else {
            [*x, *y]
        })
    }
}

fn opaque_source(layer: &Layer, items: Option<&HashMap<u32, &ProjectItem>>) -> bool {
    let Some(Ok(solid)) = items
        .and_then(|items| items.get(&layer.record.source_id()))
        .and_then(|item| item.solid.as_ref())
    else {
        return false;
    };
    if solid.width == 0
        || solid.height == 0
        || solid.pixel_aspect != (1, 1)
        || solid
            .color
            .iter()
            .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
        || layer.record.layer_type() != 0
        || layer.record.flags().null_layer
        || layer.record.flags().three_d_layer
        || layer.record.flags().adjustment_layer
    {
        return false;
    }
    let Ok(roots) = properties::root_runs(&layer.content) else {
        return false;
    };
    for (name, run) in roots {
        if name == "ADBE Mask Parade" {
            let Ok(group) = properties::unique_list(run, *b"tdgp") else {
                return false;
            };
            let Ok(rows) = properties::runs(group) else {
                return false;
            };
            if rows.iter().any(|(name, _)| *name != "ADBE Group End") {
                return false;
            }
        }
    }
    true
}

/// An independent generator needs a finite native canvas, not opaque input pixels.
/// Keep this source-plane proof separate from the in-place prefix alpha proof.
pub(crate) fn blend_canvas(layer: &Layer, items: Option<&HashMap<u32, &ProjectItem>>) -> bool {
    opaque_source(layer, items)
}

pub(crate) fn opaque_ordinals(
    layer: &Layer,
    sources: &[DecodedEffect],
    items: Option<&HashMap<u32, &ProjectItem>>,
) -> HashSet<usize> {
    let mut opaque = opaque_source(layer, items);
    let mut admitted = HashSet::new();
    for source in sources {
        if opaque && source.match_name == MATCH_NAME {
            admitted.insert(source.index);
        }
        if !source.enabled {
            continue;
        }
        opaque &= matches!(
            source.match_name.as_str(),
            "CC Toner" | "ADBE Tint" | "ADBE Exposure2"
        ) || (source.match_name == MATCH_NAME
            && Controls::read(layer, source).is_ok_and(|controls| {
                controls.scalar(29, 100.) == Ok(100.)
                    && controls
                        .scalar(30, 2.)
                        .is_ok_and(|blend| [2., 5., 6.].contains(&blend))
            }));
    }
    admitted
}

pub(crate) fn lower(
    source: &DecodedEffect,
    layer: &Layer,
    size: [u16; 2],
    opaque: bool,
) -> Result<(LayerEffect, &'static str), String> {
    if !opaque || size.contains(&0) {
        return Err(
            "requires proven opaque solid plane and alpha-preserving prefix with no native masks"
                .into(),
        );
    }
    let controls = Controls::read(layer, source)?;
    let noise_type = controls.scalar(2, 3.)?;
    if controls.scalar(1, 1.)? != 1.
        || ![1., 2., 3., 4.].contains(&noise_type)
        || controls.scalar(6, 4.)? != 4.
    {
        return Err("outside admitted Basic, valid Noise Type, Allow HDR profile".into());
    }
    let invert = controls.scalar(3, 0.)?;
    if ![0., 1.].contains(&invert) {
        return Err("invalid Fractal Invert switch".into());
    }
    let contrast = controls.scalar(4, 100.)?;
    let brightness = controls.scalar(5, 0.)?;
    let opacity = controls.scalar(29, 100.)?;
    let blend = controls.scalar(30, 2.)?;
    if !(0. ..=100.).contains(&opacity) {
        return Err("invalid Fractal opacity".into());
    }
    if contrast == 0. && brightness == 0. && blend == 5. {
        // Zero contrast implies the midgray constant in the related noise model.
        // Coordinates/evolution cannot change this constant; native formula is
        // undocumented, so this source-derived simplification remains approximate.
        let multiplier = 1. - opacity / 100. + opacity / 100. * 0.5;
        return Ok((
            LayerEffect::Exposure {
                exposure: Some(multiplier.log2()),
                offset: Some(0.),
                gamma_correction: Some(1.),
            },
            "zero-contrast Multiply approximated by editable Exposure using the opacity-weighted midgray constant; native contrast formula is unverified",
        ));
    }
    if noise_type != 4. || blend != 2. || controls.scalar(9, 1.)? != 1. {
        return Err("requires Normal blending and Uniform Scaling; other modes/anisotropy remain outside admitted profile".into());
    }
    Ok((
        generator(&controls, size, controls.scalar(10, 100.)?, opacity / 100.)?,
        "static uniform Basic/Spline Normal stage approximated by editable TurbulentNoise; native kernel, frame-relative feature scale, HDR overflow and evolution differ",
    ))
}

/// An independent opaque generator for bounded layer-level blend staging.
/// The existing constant Multiply simplification remains in `lower`.
pub(crate) fn blend_stage(
    layer: &Layer,
    source: &DecodedEffect,
    size: [u16; 2],
    canvas: bool,
) -> Result<(LayerEffect, fx_schema::BlendMode, f64), String> {
    if !canvas || size.contains(&0) {
        return Err(
            "requires finite 2D square-pixel solid source canvas with no native masks".into(),
        );
    }
    let controls = Controls::read(layer, source)?;
    let blend_mode = match controls.scalar(30, 2.)? {
        5. => fx_schema::BlendMode::Multiply,
        6. => fx_schema::BlendMode::Screen,
        _ => return Err("requires Multiply or Screen blend staging".into()),
    };
    let opacity = controls.scalar(29, 100.)?;
    if !(0. ..=100.).contains(&opacity) {
        return Err("invalid Fractal opacity".into());
    }
    let scale = match controls.scalar(9, 1.)? {
        1. => controls.scalar(10, 100.)?,
        0. => {
            let width = controls.scalar(11, 100.)?;
            let height = controls.scalar(12, 100.)?;
            if width <= 0. || width != height {
                return Err(
                    "nonuniform Fractal width/height remain outside admitted profile".into(),
                );
            }
            width
        }
        _ => return Err("invalid Fractal Uniform Scaling switch".into()),
    };
    Ok((generator(&controls, size, scale, 1.)?, blend_mode, opacity))
}

fn generator(
    controls: &Controls<'_>,
    size: [u16; 2],
    scale: f64,
    blend: f64,
) -> Result<LayerEffect, String> {
    if controls.scalar(1, 1.)? != 1.
        || controls.scalar(2, 3.)? != 4.
        || controls.scalar(6, 4.)? != 4.
    {
        return Err("outside admitted Basic/Spline, Allow HDR generator profile".into());
    }
    let invert = controls.scalar(3, 0.)?;
    if ![0., 1.].contains(&invert) {
        return Err("invalid Fractal Invert switch".into());
    }
    let contrast = controls.scalar(4, 100.)?;
    let brightness = controls.scalar(5, 0.)?;
    for (number, default) in [
        (18, 56.),
        (19, 0.),
        (21, 0.),
        (25, 0.),
        (26, 1.),
        (27, 0.),
        (31, 0.),
    ] {
        if controls.scalar(number, default)? != default {
            return Err(format!(
                "nondefault Fractal control {number:04} is outside admitted profile"
            ));
        }
    }
    if controls.point(20, [0.; 2], size)? != [0.; 2] {
        return Err("nonzero sub-offset is outside admitted profile".into());
    }
    let complexity = controls.scalar(15, 6.)?;
    let sub_influence = controls.scalar(17, 70.)?;
    if scale <= 0.
        || !(1. ..=10.).contains(&complexity)
        || !(0. ..=100.).contains(&sub_influence)
        || contrast < 0.
    {
        return Err(
            "Fractal scale/complexity/sub-influence/contrast outside represented range".into(),
        );
    }
    let offset = controls.point(13, size.map(|s| f64::from(s) * 0.5), size)?;
    Ok(LayerEffect::TurbulentNoise {
        brightness: Some(brightness),
        contrast: Some(contrast),
        scale: Some(scale),
        complexity: Some(complexity),
        sub_influence: Some(sub_influence),
        evolution: Some(controls.scalar(23, 0.)?),
        rotation: Some(controls.scalar(8, 0.)?),
        offset_x: Some(100. * offset[0] / f64::from(size[0]) - 50.),
        offset_y: Some((100. * offset[1] - 50. * f64::from(size[1])) / f64::from(size[0])),
        invert: Some(invert),
        blend: Some(blend),
        noise_type: Some(NoiseType::Spline),
        fractal_type: Some(FractalType::Basic),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::structure::{ItemKind, read_project};

    fn native() -> (Layer, DecodedEffect, Chunk) {
        let project = read_project(include_bytes!(
            "../../tests/fixtures/effects/shape_owner_gaussian.aep"
        ))
        .unwrap();
        let ItemKind::Composition(comp) = &project.item(1).unwrap().kind else {
            panic!()
        };
        let fixture = crate::rifx::Rifx::parse_with(
            include_bytes!("../../tests/fixtures/effects/native-fractal-noise-controls.rifx"),
            |_| false,
        )
        .unwrap();
        let mut layer = comp.layers[0].clone();
        layer.content = fixture.chunks()[0].children().unwrap().to_vec();
        layer.record = crate::schema::layer_records::LayerRecord::decode(
            properties::data(&layer.content, *b"ldta").unwrap(),
        )
        .unwrap();
        layer.name = "Renamed native source".into();
        let effect = super::super::native::read_effects(&layer.content, [1920., 1080.])
            .0
            .remove(0);
        (layer, effect, fixture.chunks()[1].clone())
    }
    fn descriptor(chunks: &mut [Chunk]) -> &mut Vec<Chunk> {
        for chunk in chunks {
            if chunk.list_kind() == Some(*b"sspc") {
                return chunk.children_mut().unwrap();
            }
            if let Some(children) = chunk.children_mut()
                && contains_descriptor(children)
            {
                return descriptor(children);
            }
        }
        panic!("descriptor missing")
    }
    fn opaque_prefix(
        layer: &Layer,
        index: usize,
        sources: &[DecodedEffect],
        items: Option<&HashMap<u32, &ProjectItem>>,
    ) -> bool {
        opaque_ordinals(layer, sources, items).contains(&index)
    }
    fn contains_descriptor(chunks: &[Chunk]) -> bool {
        chunks.iter().any(|chunk| {
            chunk.list_kind() == Some(*b"sspc") || chunk.children().is_some_and(contains_descriptor)
        })
    }
    fn scalar(layer: &mut Layer, number: usize, value: f64) {
        let body = descriptor(&mut layer.content)
            .iter_mut()
            .find(|c| c.list_kind() == Some(*b"tdgp"))
            .unwrap()
            .children_mut()
            .unwrap();
        let name = format!("ADBE Fractal Noise-{number:04}");
        let start = body.iter().position(|c| {
            c.id() == *b"tdmn"
                && c.data_payload()
                    .is_some_and(|data| data.starts_with(name.as_bytes()))
        });
        if let Some(start) = start {
            let leaf = body[start + 1..]
                .iter_mut()
                .find(|c| c.list_kind() == Some(*b"tdbs"))
                .unwrap()
                .children_mut()
                .unwrap();
            *leaf.iter_mut().find(|c| c.id() == *b"cdat").unwrap() =
                Chunk::data(*b"cdat", value.to_be_bytes()).unwrap();
        } else {
            let roots = properties::runs(body).unwrap();
            let template = roots
                .iter()
                .find(|(name, _)| *name == "ADBE Fractal Noise-0004")
                .unwrap()
                .1
                .to_vec();
            let mut leaf = template
                .into_iter()
                .find(|c| c.list_kind() == Some(*b"tdbs"))
                .unwrap();
            let children = leaf.children_mut().unwrap();
            *children.iter_mut().find(|c| c.id() == *b"cdat").unwrap() =
                Chunk::data(*b"cdat", value.to_be_bytes()).unwrap();
            let mut name_bytes = vec![0; 40];
            name_bytes[..name.len()].copy_from_slice(name.as_bytes());
            body.push(Chunk::data(*b"tdmn", name_bytes).unwrap());
            body.push(leaf);
        }
    }
    fn flag(layer: &mut Layer, number: usize, animated: bool) {
        let body = descriptor(&mut layer.content)
            .iter_mut()
            .find(|c| c.list_kind() == Some(*b"tdgp"))
            .unwrap()
            .children_mut()
            .unwrap();
        let name = format!("ADBE Fractal Noise-{number:04}");
        let start = body
            .iter()
            .position(|c| {
                c.id() == *b"tdmn"
                    && c.data_payload()
                        .is_some_and(|data| data.starts_with(name.as_bytes()))
            })
            .unwrap();
        let leaf = body[start + 1..]
            .iter_mut()
            .find(|c| c.list_kind() == Some(*b"tdbs"))
            .unwrap()
            .children_mut()
            .unwrap();
        let meta = leaf.iter_mut().find(|c| c.id() == *b"tdb4").unwrap();
        let mut bytes = meta.data_payload().unwrap().to_vec();
        if animated {
            bytes[68] = 1
        } else {
            bytes[120] |= 1;
            bytes[119] &= !1
        }
        *meta = Chunk::data(*b"tdb4", bytes).unwrap();
    }

    #[test]
    fn full_native_fractal_defaults_use_popup_default_not_cached_blend() {
        let (mut layer, source, table) = native();
        let d = descriptor(&mut layer.content);
        *d.iter_mut()
            .find(|c| c.list_kind() == Some(*b"parT"))
            .unwrap() = table;
        assert_eq!(
            Controls::read(&layer, &source)
                .unwrap()
                .scalar(30, 2.)
                .unwrap(),
            2.
        );
        assert!(matches!(
            lower(&source, &layer, [1280, 720], true).unwrap().0,
            LayerEffect::TurbulentNoise { .. }
        ));
        let d = descriptor(&mut layer.content);
        let table = d
            .iter_mut()
            .find(|c| c.list_kind() == Some(*b"parT"))
            .unwrap()
            .children_mut()
            .unwrap();
        let start = table
            .iter()
            .position(|c| {
                c.id() == *b"tdmn"
                    && c.data_payload()
                        .unwrap()
                        .starts_with(b"ADBE Fractal Noise-0030")
            })
            .unwrap();
        let pard = table[start + 1..]
            .iter_mut()
            .find(|c| c.id() == *b"pard")
            .unwrap();
        let mut bytes = pard.data_payload().unwrap().to_vec();
        bytes[62..64].copy_from_slice(&5_u16.to_be_bytes());
        *pard = Chunk::data(*b"pard", bytes).unwrap();
        assert!(lower(&source, &layer, [1280, 720], true).is_err());
    }

    #[test]
    fn fractal_profile_rejects_unsupported_controls_and_preserves_source_values() {
        let (layer, source, _) = native();
        for (number, value) in [
            (1, 15.),
            (2, 3.),
            (3, 2.),
            (4, -1.),
            (6, 1.),
            (9, 0.),
            (10, 0.),
            (15, 11.),
            (18, 40.),
            (19, 10.),
            (25, 1.),
            (27, 2.),
            (29, 101.),
            (30, 6.),
            (23, f64::NAN),
        ] {
            let mut changed = layer.clone();
            scalar(&mut changed, number, value);
            assert!(
                lower(&source, &changed, [1280, 720], true).is_err(),
                "{number}={value}"
            );
        }
        assert!(lower(&source, &layer, [1280, 720], false).is_err());
        assert!(lower(&source, &layer, [0, 720], true).is_err());
        let mut changed = layer.clone();
        scalar(&mut changed, 10, 350.);
        scalar(&mut changed, 4, 71.);
        scalar(&mut changed, 29, 25.);
        let LayerEffect::TurbulentNoise {
            scale,
            contrast,
            blend,
            ..
        } = lower(&source, &changed, [1280, 720], true).unwrap().0
        else {
            panic!()
        };
        assert_eq!(
            (scale, contrast, blend),
            (Some(350.), Some(71.), Some(0.25))
        );
        scalar(&mut changed, 4, 0.);
        scalar(&mut changed, 5, 0.);
        scalar(&mut changed, 30, 5.);
        let LayerEffect::Exposure { exposure, .. } =
            lower(&source, &changed, [1280, 720], true).unwrap().0
        else {
            panic!()
        };
        assert_eq!(exposure, Some(0.875_f64.log2()));
        scalar(&mut changed, 5, 1.);
        assert!(lower(&source, &changed, [1280, 720], true).is_err());
        for number in [4, 5, 23, 29, 30] {
            for animated in [false, true] {
                let mut changed = layer.clone();
                scalar(&mut changed, number, if number == 30 { 2. } else { 50. });
                flag(&mut changed, number, animated);
                assert!(
                    lower(&source, &changed, [1280, 720], true).is_err(),
                    "dynamic {number}"
                );
            }
        }
        let mut constant = layer.clone();
        scalar(&mut constant, 4, 0.);
        scalar(&mut constant, 5, 0.);
        scalar(&mut constant, 30, 5.);
        flag(&mut constant, 23, false);
        assert!(matches!(
            lower(&source, &constant, [1280, 720], true).unwrap().0,
            LayerEffect::Exposure { .. }
        ));
        let d = descriptor(&mut constant.content);
        let body = d
            .iter_mut()
            .find(|c| c.list_kind() == Some(*b"tdgp"))
            .unwrap()
            .children_mut()
            .unwrap();
        let mut name = vec![0; 40];
        name[..27].copy_from_slice(b"ADBE Effect Built In Params");
        let mut unknown = vec![0; 40];
        unknown[..13].copy_from_slice(b"Unsupported X");
        body.push(Chunk::data(*b"tdmn", name).unwrap());
        body.push(Chunk::list(
            *b"tdgp",
            vec![Chunk::data(*b"tdmn", unknown).unwrap()],
        ));
        assert!(lower(&source, &constant, [1280, 720], true).is_err());
    }

    #[test]
    fn native_equal_width_height_multiply_stage_retains_authored_controls() {
        let (layer, _, _) = native();
        let sources = super::super::native::read_effects(&layer.content, [1920., 1080.]).0;
        let sources = sources
            .iter()
            .filter(|source| source.match_name == MATCH_NAME)
            .collect::<Vec<_>>();
        let source = sources[2];
        assert!(lower(source, &layer, [1280, 720], true).is_err());
        let (effect, mode, opacity) = blend_stage(&layer, source, [1280, 720], true).unwrap();
        assert_eq!((mode, opacity), (fx_schema::BlendMode::Multiply, 100.));
        let LayerEffect::TurbulentNoise {
            contrast,
            brightness,
            scale,
            complexity,
            evolution,
            blend,
            ..
        } = effect
        else {
            panic!()
        };
        assert_eq!(
            (contrast, brightness, scale, complexity, evolution, blend),
            (
                Some(122.),
                Some(-10.),
                Some(666.),
                Some(2.),
                Some(161.),
                Some(1.)
            ),
        );
        // The adjacent anisotropic and Cloudy native stages remain omissions.
        for source in [sources[1], sources[3]] {
            assert!(blend_stage(&layer, source, [1280, 720], true).is_err());
        }
    }

    #[test]
    fn static_screen_generator_keeps_opacity_on_blend_stage_and_rejects_dynamic_controls() {
        let (mut layer, source, _) = native();
        scalar(&mut layer, 30, 6.);
        scalar(&mut layer, 29, 25.);
        scalar(&mut layer, 10, 350.);
        let (effect, mode, opacity) = blend_stage(&layer, &source, [1280, 720], true).unwrap();
        assert_eq!((mode, opacity), (fx_schema::BlendMode::Screen, 25.));
        assert!(matches!(
            effect,
            LayerEffect::TurbulentNoise {
                scale: Some(350.),
                blend: Some(1.),
                ..
            }
        ));
        for (number, value) in [
            (1, 15.),
            (2, 3.),
            (3, 2.),
            (4, -1.),
            (6, 1.),
            (9, 2.),
            (10, 0.),
            (15, 11.),
            (18, 40.),
            (19, 10.),
            (25, 1.),
            (27, 2.),
            (29, 101.),
            (30, 7.),
        ] {
            let mut rejected = layer.clone();
            scalar(&mut rejected, number, value);
            assert!(
                blend_stage(&rejected, &source, [1280, 720], true).is_err(),
                "{number}={value}"
            );
        }
        for number in [4, 5, 10, 23, 29, 30] {
            for animated in [false, true] {
                let mut rejected = layer.clone();
                scalar(&mut rejected, number, if number == 30 { 6. } else { 50. });
                flag(&mut rejected, number, animated);
                assert!(
                    blend_stage(&rejected, &source, [1280, 720], true).is_err(),
                    "dynamic {number}"
                );
            }
        }
        assert!(blend_stage(&layer, &source, [1280, 720], false).is_err());
        assert!(blend_stage(&layer, &source, [0, 720], true).is_err());
        scalar(&mut layer, 9, 0.);
        scalar(&mut layer, 11, 666.);
        scalar(&mut layer, 12, 666.);
        assert!(blend_stage(&layer, &source, [1280, 720], true).is_ok());
        for height in [0., 667., f64::INFINITY] {
            let mut rejected = layer.clone();
            scalar(&mut rejected, 12, height);
            assert!(blend_stage(&rejected, &source, [1280, 720], true).is_err());
        }
    }

    #[test]
    fn opaque_prefix_rejects_masks_and_alpha_changing_effects() {
        let (mut layer, source, _) = native();
        let project = read_project(include_bytes!(
            "../../tests/fixtures/effects/shape_owner_gaussian.aep"
        ))
        .unwrap();
        let mut item = project.items[0].clone();
        item.solid = Some(Ok(crate::structure::SolidSource {
            width: 1280,
            height: 720,
            pixel_aspect: (1, 1),
            color: [0.; 3],
        }));
        let items = HashMap::from([(layer.record.source_id(), &item)]);
        assert!(opaque_prefix(
            &layer,
            1,
            std::slice::from_ref(&source),
            Some(&items)
        ));
        for (offset, byte) in [(38, 0x80), (131, 1)] {
            let mut rejected = layer.clone();
            let mut record = properties::data(&rejected.content, *b"ldta")
                .unwrap()
                .to_vec();
            record[offset] = byte;
            rejected.record = crate::schema::layer_records::LayerRecord::decode(&record).unwrap();
            assert!(
                !opaque_prefix(&rejected, 1, std::slice::from_ref(&source), Some(&items)),
                "null and non-AV carriers do not draw an opaque source plane"
            );
        }
        let mut prefix = source.clone();
        prefix.index = 0;
        prefix.match_name = "ADBE Drop Shadow".into();
        assert!(!opaque_prefix(
            &layer,
            1,
            &[prefix, source.clone()],
            Some(&items)
        ));
        assert!(!opaque_prefix(
            &layer,
            1,
            std::slice::from_ref(&source),
            None
        ));
        let root = layer
            .content
            .iter_mut()
            .find(|c| c.list_kind() == Some(*b"tdgp"))
            .unwrap()
            .children_mut()
            .unwrap();
        let mut name = vec![0; 40];
        name[..16].copy_from_slice(b"ADBE Mask Parade");
        let mut mask = vec![0; 40];
        mask[..10].copy_from_slice(b"ADBE Mask1");
        root.push(Chunk::data(*b"tdmn", name).unwrap());
        root.push(Chunk::list(
            *b"tdgp",
            vec![Chunk::data(*b"tdmn", mask).unwrap()],
        ));
        assert!(!opaque_prefix(
            &layer,
            1,
            std::slice::from_ref(&source),
            Some(&items)
        ));
    }
}
