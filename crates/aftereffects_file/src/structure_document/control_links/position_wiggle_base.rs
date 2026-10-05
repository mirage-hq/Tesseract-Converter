//! Retain native base motion for a bounded additive-jitter expression.
//!
//! This does not implement wiggle or posterizeTime. Both are omitted; only the
//! already-authored Position keys are kept rather than substituting comp center.

use super::{expression, finished, finite_number, token, unique_run};
use crate::{
    properties::{self, PropertyError},
    structure::Layer,
};

pub(super) fn source(layer: &Layer) -> Result<&str, PropertyError> {
    let root = properties::root_runs(&layer.content)?;
    let transform = unique_run(&root, "ADBE Transform Group")?;
    let leaves = properties::runs(properties::unique_list(transform, *b"tdgp")?)?;
    let body = properties::unique_list(unique_run(&leaves, "ADBE Position")?, *b"tdbs")?;
    expression(body)
}

pub(super) fn recognized(text: &str) -> bool {
    parse(text).is_some()
}

fn parse(mut text: &str) -> Option<()> {
    token(&mut text, "posterizeTime")?;
    token(&mut text, "(")?;
    positive(&mut text)?;
    token(&mut text, ")")?;
    token(&mut text, ";")?;
    token(&mut text, "wiggle")?;
    token(&mut text, "(")?;
    positive(&mut text)?;
    token(&mut text, ",")?;
    (finite_number(&mut text)? >= 0.0).then_some(())?;
    token(&mut text, ")")?;
    finished(text).then_some(())
}

fn positive(text: &mut &str) -> Option<()> {
    (finite_number(text)? > 0.0).then_some(())
}

#[cfg(test)]
mod tests {
    use super::recognized;

    #[test]
    fn complete_positive_constant_profile_only() {
        for expression in [
            "posterizeTime(5);\rwiggle(3, 5);",
            "posterizeTime(7); wiggle(1, 15)",
            "posterizeTime(5);wiggle(3,0);",
        ] {
            assert!(recognized(expression), "{expression}");
        }
        for expression in [
            "posterizeTime(0);wiggle(3,5)",
            "posterizeTime(5);wiggle(-3,5)",
            "posterizeTime(5);wiggle(3,-5)",
            "posterizeTime(5);wiggle(3,5,2)",
            "posterizeTime(5);wiggle(3,5);value+[100,0]",
            "posterizeTime(5);wiggle(3,5)*2",
            "posterizeTime(time);wiggle(3,5)",
            "posterizeTime(5);wiggle(3,effect('Amount')(1))",
            "wiggle(3,5)",
        ] {
            assert!(!recognized(expression), "{expression}");
        }
    }
}
