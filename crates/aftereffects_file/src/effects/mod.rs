//! Converter-owned effect catalog and native-format boundaries.
//! No renderer, persisted FX schema, or expression runtime is extended here.

pub(crate) mod box_blur;
pub(crate) mod catalog;
pub(crate) mod definitions;
pub(crate) mod fractal_noise;
pub(crate) mod keylight;
pub(crate) mod mapping;
pub(crate) mod native;
pub(crate) mod special;
pub(crate) mod toner;

#[cfg(test)]
mod tests;
