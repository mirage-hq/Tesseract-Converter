//! Successful expression samples shared by identical occurrences in one import.

use std::{collections::HashMap, rc::Rc};

use crate::{
    essential::Override,
    expression_eval::Approximation,
    expression_samples::ExpressionSamples,
    structure::{Composition, ProjectItem},
};

#[derive(Default)]
pub(super) struct ExpressionEvaluations {
    successful: HashMap<u32, CachedOccurrence>,
    #[cfg(test)]
    evaluations: usize,
    #[cfg(test)]
    reused_properties: usize,
}

struct CachedOccurrence {
    overrides: Vec<Override>,
    evaluation: Rc<EvaluatedOccurrence>,
    complete: bool,
}

pub(super) struct EvaluatedOccurrence {
    pub(super) samples: ExpressionSamples,
    pub(super) approximations: Vec<Rc<Approximation>>,
}

impl ExpressionEvaluations {
    // Only Converter::composition_layers calls this with its immutable source
    // graph/capture and the composition after applying these ordered overrides.
    // Native identity plus exact overrides therefore fixes every evaluator input,
    // including dependency layers, source-frame grid and seeded random streams.
    // Placement clocks/FX IDs are not evaluator inputs and remain occurrence-local.
    pub(super) fn evaluate(
        &mut self,
        items: &HashMap<u32, &ProjectItem>,
        comp_id: u32,
        comp: &Composition,
        captured: &ExpressionSamples,
        overrides: &[Override],
    ) -> Rc<EvaluatedOccurrence> {
        let cached = self
            .successful
            .get(&comp_id)
            .filter(|cached| cached.overrides == overrides);
        if let Some(cached) = cached
            && cached.complete
        {
            return Rc::clone(&cached.evaluation);
        }
        // A failed sibling must not force complete numeric tracks to resample.
        // Use the evaluator's existing supplied-record path without changing its
        // override flag. Overridden batches deliberately reject source-ID-only
        // records, so partial reuse is restricted to unoverridden occurrences.
        let reusable = cached.filter(|_| overrides.is_empty());
        let supplied = reusable.map(|cached| {
            #[cfg(test)]
            {
                self.reused_properties += cached.evaluation.samples.properties.len();
            }
            let mut supplied = ExpressionSamples::default();
            supplied.properties = cached.evaluation.samples.properties.clone();
            supplied.errors.extend(
                captured
                    .errors()
                    .iter()
                    .filter(|error| error.composition_id() == comp_id)
                    .cloned(),
            );
            supplied
        });
        #[cfg(test)]
        {
            self.evaluations += 1;
        }
        let mut approximations = Vec::new();
        let samples = crate::expression_eval::evaluate_occurrence_with_diagnostics(
            items,
            comp_id,
            comp,
            supplied.as_ref().unwrap_or(captured),
            !overrides.is_empty(),
            &mut approximations,
        );
        drop(supplied);
        let mut approximations: Vec<_> = approximations.into_iter().map(Rc::new).collect();
        if let Some(cached) = reusable {
            let mut ordered = Vec::new();
            for prior in &cached.evaluation.approximations {
                // Prefer freshly evaluated notes (e.g. Source Text); replay a
                // prior note only for a numeric record actually supplied above.
                if let Some(index) = approximations.iter().position(|note| {
                    note.layer_id == prior.layer_id && note.property == prior.property
                }) {
                    ordered.push(approximations.remove(index));
                } else if cached
                    .evaluation
                    .samples
                    .lookup(comp_id, prior.layer_id, &prior.property)
                    .is_some()
                {
                    ordered.push(Rc::clone(prior));
                }
            }
            ordered.extend(approximations);
            approximations = ordered;
        }
        let evaluation = Rc::new(EvaluatedOccurrence {
            samples,
            approximations,
        });
        // Keep at most one variant per native composition, never per occurrence.
        // A failed batch stores only complete successful numeric records, not
        // errors or partial grids. Its failed properties are evaluated afresh.
        let complete = evaluation.samples.errors().is_empty();
        // Matching partial entries need no replacement: immutable inputs make
        // their complete numeric records stable. Do not clone them into a new
        // retained entry after every freshly evaluated failure.
        if complete
            || (overrides.is_empty()
                && cached.is_none()
                && !evaluation.samples.properties.is_empty())
        {
            let retained = if complete {
                Rc::clone(&evaluation)
            } else {
                let mut samples = ExpressionSamples::default();
                samples.properties = evaluation.samples.properties.clone();
                Rc::new(EvaluatedOccurrence {
                    samples,
                    approximations: evaluation.approximations.clone(),
                })
            };
            self.successful.insert(
                comp_id,
                CachedOccurrence {
                    overrides: overrides.to_vec(),
                    evaluation: retained,
                    complete,
                },
            );
        }
        evaluation
    }
}

#[cfg(test)]
mod tests;
