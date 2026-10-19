//! Normalize native tiling inputs to FX Group effects' root-sized plane.

use fx_schema::{EffectData, EffectId, GroupLayer, LayerData, LayerId, Position};

use super::{MAX_GROUP_DEPTH, group, reserve_ids};

#[derive(Clone, Copy)]
enum InputPlane {
    Shape(Option<LayerId>),
    Raster,
}

pub(super) fn apply(
    owner: &mut GroupLayer,
    effect: EffectId,
    generated_text: Option<LayerId>,
    source: [u16; 2],
    root: [u16; 2],
    depth: usize,
    next: &mut u64,
) -> Result<(), String> {
    normalize(
        owner,
        effect,
        InputPlane::Shape(generated_text),
        source,
        root,
        depth,
        next,
    )
}

pub(super) fn apply_raster(
    owner: &mut GroupLayer,
    effect: EffectId,
    source: [u16; 2],
    root: [u16; 2],
    depth: usize,
    next: &mut u64,
) -> Result<(), String> {
    normalize(owner, effect, InputPlane::Raster, source, root, depth, next)
}

fn normalize(
    owner: &mut GroupLayer,
    effect: EffectId,
    plane: InputPlane,
    source: [u16; 2],
    root: [u16; 2],
    depth: usize,
    next: &mut u64,
) -> Result<(), String> {
    if source.contains(&0)
        || root.contains(&0)
        || depth + 3 >= MAX_GROUP_DEPTH
        || !owner.masks.is_empty()
        || owner.track_matte.is_some()
        || owner.layers.len() != 1
    {
        return Err("requires one finite source Group without owner masks/matte and room for two helper Groups".into());
    }
    let transform = &owner.transform;
    if matches!(plane, InputPlane::Shape(_))
        && (transform.rotation != 0.
            || transform.rotation_x != 0.
            || transform.rotation_y != 0.
            || transform.orientation != [0.; 3]
            || transform.skew != 0.
            || transform.position.z_or_zero() != 0.
            || !transform.scale.iter().all(|v| v.is_finite() && *v > 0.)
            || !transform.anchor_point.iter().all(|v| v.is_finite())
            || !transform.position.xy_array().iter().all(|v| v.is_finite()))
    {
        return Err("requires a finite positive axis-aligned planar owner Transform".into());
    }
    let mut candidate = owner.clone();
    let mut content = candidate.layers.remove(0).data().clone();
    let LayerData::Group(content_group) = &mut content else {
        return Err("source content is not an editable Group".into());
    };
    let mut cursor = *next;
    let first = reserve_ids(&mut cursor, 2).ok_or("Tiler helper identity allocation exhausted")?;
    let mut output = group(
        LayerId::new(first),
        "Tiler source plane".into(),
        Some(owner.id),
        owner.playback.input_range(),
    );
    let mut input = group(
        LayerId::new(first + 1),
        "Tiler normalized input".into(),
        Some(output.id),
        owner.playback.input_range(),
    );
    let [sw, sh] = source.map(f64::from);
    let [rw, rh] = root.map(f64::from);
    let [sx, sy] = transform.scale.map(|v| v / 100.);
    let [ax, ay] = transform.anchor_point;
    let [px, py] = transform.position.xy_array();
    if let InputPlane::Shape(Some(id)) = plane {
        let layer = content_group
            .layers
            .iter_mut()
            .find(|layer| layer.data().id() == id)
            .ok_or("generated Text is not in the source content Group")?;
        let mut data = layer.data().clone();
        let LayerData::Text(text) = &mut data else {
            return Err("generated paint is not editable Text".into());
        };
        let [x, y] = text.transform.position.xy_array();
        // Effect-generated paint follows Shape geometry, so cancel only its owner geometry.
        text.transform.position =
            Position::xy((x + sw / 2. - px) / sx + ax, (y + sh / 2. - py) / sy + ay);
        text.transform.scale[0] /= sx;
        text.transform.scale[1] /= sy;
        *layer = fx_schema::Layer::from_data(&data).map_err(|e| e.to_string())?;
    }
    match plane {
        InputPlane::Shape(_) => {
            // Shape effects sample the composition plane after the owner's geometry.
            input.transform.position =
                Position::xy((px - sx * ax) * rw / sw, (py - sy * ay) * rh / sh);
            input.transform.scale = [100. * sx * rw / sw, 100. * sy * rh / sh];
            output.transform.position = Position::xy(ax - px / sx, ay - py / sy);
            output.transform.scale = [100. * sw / rw / sx, 100. * sh / rh / sy];
        }
        InputPlane::Raster => {
            // Raster effects precede the occurrence transform and use a top-left source origin.
            input.transform.scale = [100. * rw / sw, 100. * rh / sh];
            output.transform.scale = [100. * sw / rw, 100. * sh / rh];
        }
    }
    content_group.parent = Some(input.id);
    input
        .layers
        .push(fx_schema::Layer::from_data(&content).map_err(|e| e.to_string())?);
    let record = candidate.effects.remove(0);
    if !matches!(record.data(), EffectData::Identified { id, .. } if *id == effect) {
        return Err("Tiler is not the first mapped effect".into());
    }
    output.effects.push(record);
    output
        .layers
        .push(fx_schema::Layer::from_data(&LayerData::Group(input)).map_err(|e| e.to_string())?);
    candidate
        .layers
        .push(fx_schema::Layer::from_data(&LayerData::Group(output)).map_err(|e| e.to_string())?);
    *owner = candidate;
    *next = cursor;
    Ok(())
}
