//! Converter-owned effect catalog and native-format boundaries.
//! No renderer, persisted FX schema, or expression runtime is extended here.

pub(crate) mod apply_color_lut;
pub(crate) mod box_blur;
pub(crate) mod catalog;
pub(crate) mod color_grades;
pub(crate) mod compositing;
pub(crate) mod curves;
pub(crate) mod definitions;
pub(crate) mod extract;
pub(crate) mod fractal_noise;
pub(crate) mod glasses;
pub(crate) mod keylight;
pub(crate) mod mapping;
pub(crate) mod native;
pub(crate) mod roughen;
pub(crate) mod special;
pub(crate) mod tiler;
pub(crate) mod toner;
pub(crate) mod venetian_blinds;

#[cfg(test)]
mod tests;
