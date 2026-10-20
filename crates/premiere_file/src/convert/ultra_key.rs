//! Deliberate static RGB-distance keyer, not Adobe's proprietary Ultra Key.
use crate::schema::ultra_key::{PrUltraKey, APPROXIMATION};
use fx_schema::{effect::EffectParam, LayerEffect};

/// Shared identity used to recognize the coverage-preserving approximation.
pub(super) const SHADER_NAME: &str = "Premiere Ultra Key approximation";

pub(super) fn effect(key: PrUltraKey) -> LayerEffect {
    let color = key.color.rgb.map(|v| f64::from(v) / 255.0);
    let params = [
        (
            "key.colorR",
            "Saved Key Color red (display RGB)",
            1.0,
            color[0],
        ),
        (
            "key.colorG",
            "Saved Key Color green (display RGB)",
            1.0,
            color[1],
        ),
        (
            "key.colorB",
            "Saved Key Color blue (display RGB)",
            1.0,
            color[2],
        ),
        (
            "tolerance",
            "Saved Tolerance percent; surrogate RGB radius contribution",
            100.0,
            key.tolerance,
        ),
        (
            "pedestal",
            "Saved Pedestal percent; surrogate RGB radius contribution",
            100.0,
            key.pedestal,
        ),
        (
            "transparency",
            "Saved Transparency percent; surrogate edge softness",
            100.0,
            key.transparency,
        ),
    ]
    .into_iter()
    .map(|(name, description, max, default)| EffectParam {
        name: name.into(),
        description: description.into(),
        min: 0.0,
        max,
        default,
    })
    .collect();
    LayerEffect::CustomShader {
        name: SHADER_NAME.into(),
        description: APPROXIMATION.into(),
        wgsl: include_str!("ultra_key.wgsl").into(),
        params,
        texture_inputs: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emitted_shader_uses_the_shared_coverage_key_identity() {
        let key = PrUltraKey {
            color: crate::schema::PrColour { rgb: [0, 255, 0] },
            tolerance: 50.0,
            pedestal: 10.0,
            transparency: 0.0,
        };
        let LayerEffect::CustomShader { name, .. } = effect(key) else {
            panic!("Ultra Key must retain its approximation shader");
        };
        assert_eq!(name, SHADER_NAME);
        assert_eq!(name, "Premiere Ultra Key approximation");
    }
}
