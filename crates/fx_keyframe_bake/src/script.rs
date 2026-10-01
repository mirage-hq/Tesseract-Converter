//! Shared Boa execution core for editable FX scripts and keyframe baking.
//!
//! The host builds the input in this runtime's realm before calling `call`.
//! VM iteration, recursion and stack limits are not wall-clock or heap limits.

use std::collections::HashMap;

use boa_engine::{
    Context, JsError, JsNativeError, JsSymbol, JsValue, NativeFunction, Source, js_string,
    object::{FunctionObjectBuilder, builtins::JsFunction},
    property::{Attribute, PropertyDescriptor},
};
use boa_runtime::Console;

const JS_LOOP_ITERATION_LIMIT: u64 = 100_000;
const JS_RECURSION_LIMIT: usize = 128;
// Boa counts VM stack slots, not native stack bytes.
const JS_STACK_SLOT_LIMIT: usize = 10 * 1024;
const TIME_OBJECT_COERCION_ERROR: &str =
    "input.time is an object; use input.time.seconds or input.time.milliseconds";

/// An error from preparing, compiling or executing a script.
#[derive(Debug, thiserror::Error)]
pub enum ScriptError {
    /// A native Boa error or an invalid compiled function.
    #[error("{message}")]
    Runtime { message: String },
    /// An exception thrown by JavaScript.
    #[error("{message}")]
    Exception { message: String },
    /// The input did not have the expected object/time shape.
    #[error("{message}")]
    SerializeInput { message: String },
}

impl From<JsError> for ScriptError {
    fn from(error: JsError) -> Self {
        if error.as_native().is_some() {
            Self::Runtime {
                message: error.to_string(),
            }
        } else {
            Self::Exception {
                message: error.to_string(),
            }
        }
    }
}

/// One persistent Boa realm and a cache of compiled JS functions.
#[derive(Debug)]
pub struct ScriptRuntime {
    context: Context,
    time_object_coercion_guard: JsFunction,
    // Cache compilation only; every call still executes in the shared realm.
    functions: HashMap<String, JsFunction>,
    compile_count: usize,
}

impl ScriptRuntime {
    /// Creates the same bounded VM and console used during FX playback.
    pub fn new() -> Result<Self, ScriptError> {
        let mut context = Context::default();
        // Boa exposes no wall-clock interrupt hook. These limits are weaker
        // than a sandbox for straight-line expensive scripts.
        let limits = context.runtime_limits_mut();
        limits.set_loop_iteration_limit(JS_LOOP_ITERATION_LIMIT);
        limits.set_recursion_limit(JS_RECURSION_LIMIT);
        limits.set_stack_size_limit(JS_STACK_SLOT_LIMIT);
        let console = Console::init(&mut context);
        context
            .register_global_property(Console::NAME, console, Attribute::all())
            .map_err(ScriptError::from)?;
        let time_object_coercion_guard = FunctionObjectBuilder::new(
            context.realm(),
            NativeFunction::from_fn_ptr(reject_time_object_coercion),
        )
        .name(JsSymbol::to_primitive().fn_name())
        .length(1)
        .build();
        Ok(Self {
            context,
            time_object_coercion_guard,
            functions: HashMap::new(),
            compile_count: 0,
        })
    }

    /// Exposes the realm for constructing inputs and converting outputs.
    pub fn context_mut(&mut self) -> &mut Context {
        &mut self.context
    }

    /// Installs the time-object coercion guard, then invokes the cached function.
    pub fn call(&mut self, code: &str, input: JsValue) -> Result<JsValue, ScriptError> {
        install_time_object_coercion_guard(
            &input,
            &self.time_object_coercion_guard,
            &mut self.context,
        )?;
        let function = self.function_for(code)?;
        function
            .call(&JsValue::undefined(), &[input], &mut self.context)
            .map_err(ScriptError::from)
    }

    /// Compiles using the same wrapper as `call` without running user code.
    /// Unknown globals are allowed because scripts can define them at runtime.
    pub fn validate(&mut self, code: &str) -> Result<(), ScriptError> {
        self.function_for(code).map(|_| ())
    }

    /// Number of distinct sources successfully compiled by this runtime.
    pub fn compile_count(&self) -> usize {
        self.compile_count
    }

    fn function_for(&mut self, code: &str) -> Result<JsFunction, ScriptError> {
        // An owned GC handle avoids borrowing the cache while calling Boa.
        if let Some(function) = self.functions.get(code) {
            return Ok(function.clone());
        }
        let value = self
            .context
            .eval(Source::from_bytes(wrap_script(code).as_str()))
            .map_err(ScriptError::from)?;
        let function = value
            .as_object()
            .and_then(JsFunction::from_object)
            .ok_or_else(|| ScriptError::Runtime {
                message: "script expression did not evaluate to a function".into(),
            })?;
        self.functions.insert(code.to_owned(), function.clone());
        self.compile_count += 1;
        Ok(function)
    }
}

fn install_time_object_coercion_guard(
    input: &JsValue,
    guard: &JsFunction,
    context: &mut Context,
) -> Result<(), ScriptError> {
    let input = input
        .as_object()
        .ok_or_else(|| ScriptError::SerializeInput {
            message: "JavaScript animator input was not an object".into(),
        })?;
    let time = input
        .get(js_string!("time"), context)
        .map_err(ScriptError::from)?;
    let time = time
        .as_object()
        .ok_or_else(|| ScriptError::SerializeInput {
            message: "JavaScript animator time input was not an object".into(),
        })?;
    time.define_property_or_throw(
        JsSymbol::to_primitive(),
        PropertyDescriptor::builder()
            .value(guard.clone())
            .writable(false)
            .enumerable(false)
            .configurable(false),
        context,
    )
    .map_err(ScriptError::from)?;
    Ok(())
}

fn reject_time_object_coercion(
    _this: &JsValue,
    _args: &[JsValue],
    _context: &mut Context,
) -> boa_engine::JsResult<JsValue> {
    Err(JsNativeError::typ()
        .with_message(TIME_OBJECT_COERCION_ERROR)
        .into())
}

/// Installs the same readonly reference and hidden metadata tables as playback.
/// Hosts prepare their contents; an empty-reference bake uses empty objects.
pub fn install_reference_tables(
    input: &JsValue,
    refs: JsValue,
    metadata: JsValue,
    context: &mut Context,
) -> Result<(), ScriptError> {
    let input = input
        .as_object()
        .ok_or_else(|| ScriptError::SerializeInput {
            message: "JavaScript animator input was not an object".into(),
        })?;
    for (name, value, enumerable) in [
        ("refs", refs, true),
        ("__jerboaAssetMetadata", metadata, false),
    ] {
        input
            .define_property_or_throw(
                boa_engine::JsString::from(name),
                PropertyDescriptor::builder()
                    .value(value)
                    .writable(false)
                    .enumerable(enumerable)
                    .configurable(false),
                context,
            )
            .map_err(ScriptError::from)?;
    }
    Ok(())
}

fn wrap_script(code: &str) -> String {
    format!(
        "(function() {{\nconst __userAnimator = function(input) {{\n\"use strict\";\nconst getMetadata = (ref, name) => {{\n  if (ref === null || typeof ref !== 'object' || !Number.isInteger(ref.layerId) || typeof name !== 'string') return null;\n  const metadata = input.__jerboaAssetMetadata[String(ref.layerId)];\n  return metadata && Object.prototype.hasOwnProperty.call(metadata, name) ? metadata[name] : null;\n}};\n{code}\n}};\nreturn function(input) {{ return __userAnimator(input); }};\n}})()"
    )
}

#[cfg(test)]
mod tests {
    use boa_engine::JsValue;
    use serde_json::json;

    use super::{ScriptError, ScriptRuntime, TIME_OBJECT_COERCION_ERROR};

    fn input(runtime: &mut ScriptRuntime) -> JsValue {
        JsValue::from_json(
            &json!({"time": {"seconds": 1.5, "milliseconds": 1500}, "deps": [], "randomSeed": 1}),
            runtime.context_mut(),
        )
        .expect("valid test input")
    }

    #[test]
    fn scalar_and_global_state_are_preserved_between_calls() {
        let mut runtime = ScriptRuntime::new().unwrap();
        let code = "globalThis.counter = (globalThis.counter ?? 0) + 1; return input.time.seconds + globalThis.counter;";
        let first_input = input(&mut runtime);
        assert_eq!(
            runtime.call(code, first_input).unwrap().as_number(),
            Some(2.5)
        );
        let second_input = input(&mut runtime);
        assert_eq!(
            runtime.call(code, second_input).unwrap().as_number(),
            Some(3.5)
        );
        assert_eq!(runtime.compile_count(), 1);
    }

    #[test]
    fn validation_uses_cache_without_executing_script() {
        let mut runtime = ScriptRuntime::new().unwrap();
        let code = "globalThis.count = (globalThis.count ?? 0) + 1; return globalThis.count;";
        runtime.validate(code).unwrap();
        runtime.validate(code).unwrap();
        assert_eq!(runtime.compile_count(), 1);
        let input = input(&mut runtime);
        assert_eq!(runtime.call(code, input).unwrap().as_i32(), Some(1));
    }

    #[test]
    fn time_object_rejects_numeric_coercion() {
        let mut runtime = ScriptRuntime::new().unwrap();
        let input = input(&mut runtime);
        let error = runtime.call("return input.time + 1;", input).unwrap_err();
        assert!(
            matches!(error, ScriptError::Runtime { message } if message.contains(TIME_OBJECT_COERCION_ERROR))
        );
    }

    #[test]
    fn loop_budget_is_enforced() {
        let mut runtime = ScriptRuntime::new().unwrap();
        let input = input(&mut runtime);
        let error = runtime.call("while (true) {}", input).unwrap_err();
        assert!(matches!(error, ScriptError::Runtime { .. }));
    }

    #[test]
    fn native_and_thrown_errors_have_distinct_types() {
        let mut runtime = ScriptRuntime::new().unwrap();
        let native_input = input(&mut runtime);
        let native = runtime
            .call("return missingName;", native_input)
            .unwrap_err();
        assert!(matches!(native, ScriptError::Runtime { .. }));
        let thrown_input = input(&mut runtime);
        let thrown = runtime
            .call("throw 'user failure';", thrown_input)
            .unwrap_err();
        assert!(matches!(thrown, ScriptError::Exception { .. }));
        let syntax = runtime.validate("return (;").unwrap_err();
        assert!(matches!(syntax, ScriptError::Runtime { .. }));
    }
}
