//! Legacy numeric sampler cases retained as test-only conversion references.

#[cfg(test)]
mod tests;

use fx_schema::animator::PropertyKeyframeEasing;
use serde::Serialize;

use super::animation::easing_for_key;
use crate::properties::NumericProperty;

#[derive(Serialize)]
pub(super) struct ScalarChannel {
    base: f64,
    keys: Vec<ScalarKey>,
}

#[derive(Serialize)]
struct ScalarKey {
    t: f64,
    v: f64,
    e: PropertyKeyframeEasing,
    si: Option<f64>,
    so: Option<f64>,
}

impl ScalarChannel {
    pub(super) fn constant(base: f64) -> Self {
        Self {
            base,
            keys: Vec::new(),
        }
    }

    pub(super) fn animated(&self) -> bool {
        !self.keys.is_empty()
    }

    pub(super) fn decode(
        name: &str,
        numeric: &NumericProperty,
        component: usize,
        base: f64,
        warnings: &mut Vec<String>,
    ) -> Self {
        let mut result = Self::constant(base);
        if numeric.expression_enabled {
            warnings.push(format!(
                "{name}: enabled AE expression is not executed; base value retained"
            ));
            return result;
        }
        if numeric.expression_present {
            warnings.push(format!(
                "{name}: disabled AE expression omitted; native keys retained"
            ));
        }
        if numeric
            .keyframes
            .windows(2)
            .any(|pair| pair[0].time_secs >= pair[1].time_secs)
        {
            warnings.push(format!(
                "{name}: nonascending key sequence; base value retained"
            ));
            return result;
        }
        for (index, key) in numeric.keyframes.iter().enumerate() {
            let Some(value) = key.values.get(component).copied() else {
                warnings.push(format!(
                    "{name}: missing key component {component}; base value retained"
                ));
                return Self::constant(base);
            };
            let si = key.spatial_in.get(component).copied();
            let so = key.spatial_out.get(component).copied();
            if !value.is_finite()
                || !key.time_secs.is_finite()
                || si.into_iter().chain(so).any(|value| !value.is_finite())
            {
                warnings.push(format!("{name}: non-finite key data; base value retained"));
                return Self::constant(base);
            }
            result.keys.push(ScalarKey {
                t: key.time_secs,
                v: value,
                e: easing_for_key(&numeric.keyframes, index, component, 1.0, warnings, name),
                si,
                so,
            });
        }
        result
    }
}
