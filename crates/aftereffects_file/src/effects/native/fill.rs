//! Sparse Fill defaults established by fill_isolated.aep's native declarations.

use crate::properties::{NumericProperty, NumericValueKind};

pub(super) fn parameter_names(effect: &str) -> &'static [&'static str] {
    if effect == "ADBE Fill" {
        &["ADBE Fill-0002", "ADBE Fill-0005"]
    } else {
        &[]
    }
}

pub(super) fn default_parameter(effect: &str, name: &str) -> Option<NumericProperty> {
    if effect != "ADBE Fill" {
        return None;
    }
    let (values, value_kind) = match name {
        "ADBE Fill-0002" => (vec![1.0, 0.0, 0.0, 1.0], NumericValueKind::Color),
        "ADBE Fill-0005" => (vec![1.0], NumericValueKind::Continuous),
        _ => return None,
    };
    Some(NumericProperty {
        values,
        value_kind,
        animated: false,
        expression_enabled: false,
        expression_present: false,
        dimensions_separated: false,
        keyframes: Vec::new(),
    })
}
