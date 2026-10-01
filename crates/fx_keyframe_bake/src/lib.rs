//! Shared primitives for baking evaluated FX animation into editable keyframes.
//!
//! The owning adapter supplies property types, owner-local clocks and graph inputs.
//! This crate has no document, renderer or Adobe dependency.

pub mod curve_fit;
pub mod identity;
pub mod script;
pub mod value_curve;
