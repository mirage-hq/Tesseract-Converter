//! Ordered native vector program, separate from editable FX allocation.
//!
//! A paint owns a prefix of geometry, not a list of descendant FX layers.
//! Geometry references remain live across later modifiers. Groups export
//! geometry independently from their own paints and composite opacity.

use super::{is_shape_source, property_group};
use crate::rifx::Chunk;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ScopeId(pub usize);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct GeometryId(pub usize);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PaintId(pub usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Draw {
    Paint(PaintId),
    Group(ScopeId),
}

/// Semantic order, deliberately NOT an assertion about AE's numeric enum.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PaintOrder {
    AbovePrevious,
    BelowPrevious,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct NativeRun<'a> {
    pub name: &'a str,
    pub chunks: &'a [Chunk],
}

#[derive(Debug)]
pub(super) enum GeometryKind<'a> {
    Source(NativeRun<'a>),
    /// Keep Merge distinct from Union: append preserves contour winding and
    /// interior stroke boundaries whereas a union removes them.
    Merge {
        operation: NativeRun<'a>,
        operands: Vec<GeometryId>,
    },
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Modifier<'a> {
    pub owner: ScopeId,
    pub operation: NativeRun<'a>,
}

#[derive(Debug)]
pub(super) struct Geometry<'a> {
    pub owner: ScopeId,
    pub kind: GeometryKind<'a>,
    pub modifiers: Vec<Modifier<'a>>,
}

#[derive(Debug)]
pub(super) struct Paint<'a> {
    pub owner: ScopeId,
    pub enabled: bool,
    pub operation: NativeRun<'a>,
    pub geometry: Vec<GeometryId>,
}

#[derive(Debug)]
pub(super) struct Scope<'a> {
    pub parent: Option<ScopeId>,
    pub enabled: bool,
    pub operation: Option<NativeRun<'a>>,
    pub transform: Option<&'a [Chunk]>,
    /// Native operation encounter order. Order resolution is a lowering step.
    pub draws: Vec<Draw>,
    pub geometry: Vec<GeometryId>,
}

impl Scope<'_> {
    /// FX layer arrays are topmost-first. A group is below earlier elements;
    /// each paint's composite choice can put it above or below that prefix.
    pub(super) fn paint_order(&self, mut order: impl FnMut(PaintId) -> PaintOrder) -> Vec<Draw> {
        let mut result = std::collections::VecDeque::with_capacity(self.draws.len());
        for &draw in &self.draws {
            if matches!(draw, Draw::Paint(id) if order(id) == PaintOrder::AbovePrevious) {
                result.push_front(draw);
            } else {
                result.push_back(draw);
            }
        }
        result.into_iter().collect()
    }
}

#[derive(Debug)]
pub(super) struct Program<'a> {
    pub scopes: Vec<Scope<'a>>,
    pub geometry: Vec<Geometry<'a>>,
    pub paints: Vec<Paint<'a>>,
    pub warnings: Vec<String>,
}

impl<'a> Program<'a> {
    pub(super) fn parse(children: &'a [Chunk], remaining_depth: usize) -> Self {
        let mut result = Self {
            scopes: vec![Scope {
                parent: None,
                enabled: true,
                operation: None,
                transform: None,
                draws: Vec::new(),
                geometry: Vec::new(),
            }],
            geometry: Vec::new(),
            paints: Vec::new(),
            warnings: Vec::new(),
        };
        result.parse_scope(ScopeId(0), children, remaining_depth);
        result
    }

    fn warn_once(&mut self, message: &str) {
        if !self.warnings.iter().any(|warning| warning == message) {
            self.warnings.push(message.into());
        }
    }

    fn parse_scope(&mut self, owner: ScopeId, children: &'a [Chunk], depth: usize) {
        let entries = match crate::properties::runs(children) {
            Ok(entries) => entries,
            Err(error) => {
                self.warnings
                    .push(format!("vector scope {} is malformed: {error}", owner.0));
                return;
            }
        };
        for (name, chunks) in entries {
            let operation = NativeRun { name, chunks };
            let toggleable = is_shape_source(name)
                || is_paint(name)
                || name == "ADBE Vector Group"
                || name.starts_with("ADBE Vector Filter -");
            let enabled = !toggleable
                || crate::properties::group_enabled_or_warn(chunks, name, &mut self.warnings);
            if !enabled && !is_paint(name) && name != "ADBE Vector Group" {
                self.warnings.push(format!("disabled vector operation {name} omitted; inactive operation controls have no enable-switch equivalent"));
                continue;
            }
            if is_shape_source(name) {
                let id = GeometryId(self.geometry.len());
                self.geometry.push(Geometry {
                    owner,
                    kind: GeometryKind::Source(operation),
                    modifiers: Vec::new(),
                });
                self.scopes[owner.0].geometry.push(id);
            } else if is_paint(name) {
                let id = PaintId(self.paints.len());
                self.paints.push(Paint {
                    owner,
                    enabled,
                    operation,
                    geometry: self.scopes[owner.0].geometry.clone(),
                });
                self.scopes[owner.0].draws.push(Draw::Paint(id));
            } else if matches!(name, "ADBE Vector Group" | "ADBE Vectors Group") {
                self.parse_group(owner, operation, depth, enabled);
            } else if name == "ADBE Vector Filter - Merge" {
                let count = self.scopes[owner.0].geometry.len();
                if count == 0 {
                    continue;
                }
                let operands = std::mem::take(&mut self.scopes[owner.0].geometry);
                let id = GeometryId(self.geometry.len());
                self.geometry.push(Geometry {
                    owner,
                    kind: GeometryKind::Merge {
                        operation,
                        operands,
                    },
                    modifiers: Vec::new(),
                });
                self.scopes[owner.0].geometry.push(id);
                // Merge replaces the entire preceding element collection, not
                // just its outlines. Old paint records stay inert in the arena.
                self.scopes[owner.0].draws.clear();
            } else if matches!(
                name,
                "ADBE Vector Filter - RC"
                    | "ADBE Vector Filter - Offset"
                    | "ADBE Vector Filter - Trim"
            ) {
                for &id in &self.scopes[owner.0].geometry {
                    self.geometry[id.0]
                        .modifiers
                        .push(Modifier { owner, operation });
                }
            } else if name != "ADBE Vector Transform Group" {
                self.warnings.push(format!(
                    "vector operation {name} retained in source but has no program lowering"
                ));
            }
        }
    }

    fn parse_group(
        &mut self,
        parent: ScopeId,
        operation: NativeRun<'a>,
        depth: usize,
        enabled: bool,
    ) {
        if depth == 0 {
            self.warn_once("native vector scope depth budget reached; nested group omitted");
            return;
        }
        let wrapper = match crate::properties::unique_list(operation.chunks, *b"tdgp") {
            Ok(wrapper) => wrapper,
            Err(error) => {
                self.warnings
                    .push(format!("vector group malformed: {error}"));
                return;
            }
        };
        let entries = match property_group(operation.chunks, operation.name) {
            Ok(entries) => entries,
            Err(error) => {
                self.warnings.push(error);
                return;
            }
        };
        let transform = entries
            .iter()
            .find(|(name, _)| *name == "ADBE Vector Transform Group")
            .map(|(_, run)| *run);
        let contents = match entries
            .iter()
            .find(|(name, _)| *name == "ADBE Vectors Group")
        {
            Some((_, run)) => match crate::properties::unique_list(run, *b"tdgp") {
                Ok(contents) => contents,
                Err(error) => {
                    self.warnings
                        .push(format!("vector contents malformed: {error}"));
                    return;
                }
            },
            None => wrapper,
        };
        let id = ScopeId(self.scopes.len());
        self.scopes.push(Scope {
            parent: Some(parent),
            enabled,
            operation: Some(operation),
            transform,
            draws: Vec::new(),
            geometry: Vec::new(),
        });
        self.parse_scope(id, contents, depth - 1);
        if enabled {
            let geometry = self.scopes[id.0].geometry.clone();
            self.scopes[parent.0].geometry.extend(geometry);
        }
        if !enabled {
            self.warnings.push("disabled vector group retained hidden; re-enabling it does not rebuild parent paint geometry bindings".into());
        }
        self.scopes[parent.0].draws.push(Draw::Group(id));
    }
}

fn is_paint(name: &str) -> bool {
    matches!(
        name,
        "ADBE Vector Graphic - Fill"
            | "ADBE Vector Graphic - Stroke"
            | "ADBE Vector Graphic - G-Fill"
            | "ADBE Vector Graphic - G-Stroke"
    )
}

#[cfg(test)]
mod tests;
