//! Late image transport for composition-plane Twirl on a static planar owner.

use fx_schema::{
    EffectData, EffectId, EffectPayload, EffectRecord, GroupLayer, LayerEffect, Position,
};

use crate::structure::{Composition, Layer};

pub(super) fn stage(
    layer: &Layer,
    composition: &Composition,
    owner: &mut GroupLayer,
    next_id: &mut u64,
    root_plane: bool,
    linked: bool,
) -> Result<bool, String> {
    if !owner.effects.iter().any(|record| {
        let payload = match record.data() {
            EffectData::Identified { effect, .. } | EffectData::Legacy(effect) => effect,
        };
        matches!(payload, EffectPayload::Known(LayerEffect::Twirl { .. }))
    }) {
        return Ok(false);
    }
    if linked {
        return Err("Twirl late image transport not staged for linked picture: Groups use the destination composition frame, not the native source canvas; source-local controls and owner Transform remain editable without invented point/radius gains".into());
    }
    if owner.effects.iter().any(|record| {
        let effect = match record.data() {
            EffectData::Identified { effect, .. } | EffectData::Legacy(effect) => effect,
        };
        matches!(
            effect,
            EffectPayload::Known(
                LayerEffect::DropShadow(_)
                    | LayerEffect::OuterGlow(_)
                    | LayerEffect::Stroke(_)
                    | LayerEffect::GradientOverlay(_)
                    | LayerEffect::InnerShadow(_)
                    | LayerEffect::InnerGlow(_)
                    | LayerEffect::Satin(_)
                    | LayerEffect::BevelEmboss(_)
            )
        )
    }) {
        return Err("Twirl late image transport not staged for Layer Style owner (including bypassed styles): transport after screen-space styles would rotate offsets and scale widths. Original editable Transform and style phase retained; Twirl source-local/frame-plane deviation remains".into());
    }
    let unsupported = "Twirl late image transport not staged: requires a root composition-plane Shape/Text, static planar unparented Transform and no owner masks/matte; retained editable controls use post-transform frame UV instead of native source-local coordinates";
    let transform = &owner.transform;
    if !root_plane
        || !matches!(layer.record.layer_type(), 3 | 4)
        || layer.record.parent_id() != 0
        || layer.record.flags().three_d_layer
        || !owner.masks.is_empty()
        || owner.track_matte.is_some()
        || transform.skew != 0.0
        || transform.skew_axis != 0.0
        || transform.rotation_x != 0.0
        || transform.rotation_y != 0.0
        || transform.orientation != [0.0; 3]
        || transform.scale.contains(&0.0)
    {
        return Err(unsupported.into());
    }
    let (properties, _) = super::control_links::read_layer_transform(layer, composition)
        .map_err(|error| error.to_string())?;
    if properties
        .iter()
        .filter(|property| property.match_name != "ADBE Opacity")
        .any(|property| {
            !property
                .numeric
                .as_ref()
                .is_ok_and(|value| !value.animated && !value.expression_enabled)
        })
    {
        return Err(unsupported.into());
    }
    let Position::TwoD(position) = transform.position else {
        return Err(unsupported.into());
    };
    let size = [f64::from(composition.width), f64::from(composition.height)];
    let (sin, cos) = transform.rotation.to_radians().sin_cos();
    let project = |point: [f64; 2]| {
        let x = (point[0] - transform.anchor_point[0]) * transform.scale[0] / 100.0;
        let y = (point[1] - transform.anchor_point[1]) * transform.scale[1] / 100.0;
        [
            (position[0] + cos * x - sin * y) / size[0],
            (position[1] + sin * x + cos * y) / size[1],
        ]
    };
    let [ul, ur, ll, lr] = [[0.0, 0.0], [size[0], 0.0], [0.0, size[1]], size].map(project);
    let id = super::reserve_ids(next_id, 1).ok_or("Twirl staging identity exhausted")?;
    let effect = EffectRecord::from_data(&EffectData::Identified {
        id: EffectId::new(id),
        enabled: true,
        effect: EffectPayload::Known(LayerEffect::CornerPin {
            upper_left_x: ul[0],
            upper_left_y: ul[1],
            upper_right_x: ur[0],
            upper_right_y: ur[1],
            lower_left_x: ll[0],
            lower_left_y: ll[1],
            lower_right_x: lr[0],
            lower_right_y: lr[1],
        }),
    })
    .map_err(|error| error.to_string())?;
    // Opacity and playback stay on the occurrence; only image geometry moves
    // after the source effect stack. CornerPin samples the full composition.
    owner.transform.anchor_point = [0.0; 2];
    owner.transform.position = Position::TwoD([0.0; 2]);
    owner.transform.scale = [100.0; 2];
    owner.transform.rotation = 0.0;
    owner.effects.push(effect);
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn twirl_plane_rotated_nonuniform_style_owner_keeps_screen_style_phase_and_bypass() {
        let native = crate::structure::read_project(include_bytes!(
            "../../tests/fixtures/effects_coverage/native_static_controls.aep"
        ))
        .unwrap();
        let crate::structure::ItemKind::Composition(comp) = &native.item(625).unwrap().kind else {
            panic!("Twirl composition");
        };
        let document = crate::structure_document::to_structural_fx_document(&native, Some(625))
            .unwrap()
            .document;
        let fx_schema::LayerData::Group(root) = document.composition().layers()[0].data() else {
            panic!("root");
        };
        let fx_schema::LayerData::Group(owner) = root.layers[0].data() else {
            panic!("owner");
        };
        let styles = crate::structure::read_project(include_bytes!(
            "../../tests/fixtures/layer_styles/styles_static_adobe.aep"
        ))
        .unwrap();
        let crate::structure::ItemKind::Composition(style_comp) = &styles.item(1).unwrap().kind
        else {
            panic!("style composition");
        };
        let mut next_id = 10000;
        let style = super::super::layer_styles::import(
            style_comp
                .layers
                .iter()
                .find(|layer| layer.record.id() == 15)
                .unwrap(),
            [f64::from(style_comp.width), f64::from(style_comp.height)],
            &mut next_id,
            &mut super::super::animation_budget::AnimationBudget::default(),
        )
        .effects
        .remove(0);
        // Supplementary composition of two pinned native controls, not a claim
        // that Adobe authored this combined transformed/style/bypass case.
        for twirl_enabled in [false, true] {
            for style_enabled in [false, true] {
                let mut owner = owner.clone();
                owner.effects.truncate(1);
                let mut twirl = owner.effects[0].data().clone();
                let EffectData::Identified { enabled, .. } = &mut twirl else {
                    panic!("identified Twirl");
                };
                *enabled = twirl_enabled;
                owner.effects[0] = EffectRecord::from_data(&twirl).unwrap();
                let mut style = style.data().clone();
                let EffectData::Identified { enabled, .. } = &mut style else {
                    panic!("identified style");
                };
                *enabled = style_enabled;
                owner.effects.push(EffectRecord::from_data(&style).unwrap());
                owner.transform.position = Position::TwoD([160.0, 90.0]);
                owner.transform.anchor_point = [60.0, 40.0];
                owner.transform.rotation = 37.0;
                owner.transform.scale = [200.0, 50.0];
                let before = owner.clone();
                let first_unused = next_id;
                let error = stage(&comp.layers[0], comp, &mut owner, &mut next_id, true, false)
                    .unwrap_err();
                assert!(error.contains("rotate offsets and scale widths"));
                assert_eq!(next_id, first_unused);
                assert_eq!(owner.transform.rotation, 37.0);
                assert_eq!(owner.transform.scale, [200.0, 50.0]);
                assert_eq!(
                    owner.effects.len(),
                    2,
                    "no late transport may move the screen-space style"
                );
                assert_eq!(
                    owner, before,
                    "bypass and owner geometry stay editable on the original phase"
                );
            }
        }
    }
}
