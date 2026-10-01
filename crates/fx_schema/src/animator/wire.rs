//! Structural tagged animator fields.
use super::PropertyKeyframe;
use crate::PropertyValue;
use serde::{de::Error as _, Deserialize, Deserializer};
use ts_rs::TS;
crate::define_wire_property_animator_schema!();

fn reject_other_animator_variant_field<'de, D>(_: D) -> Result<(), D::Error>
where
    D: Deserializer<'de>,
{
    Err(D::Error::custom(
        "field belongs to another PropertyAnimator variant",
    ))
}
