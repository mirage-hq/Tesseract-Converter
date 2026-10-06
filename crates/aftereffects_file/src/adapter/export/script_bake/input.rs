//! Construct the playback-shaped JS input without an intermediate JSON tree.

use std::sync::OnceLock;

use boa_engine::{
    Context, JsValue, js_string,
    object::{JsObject, ObjectInitializer, builtins::JsArray},
    property::Attribute,
};

// serde_json's Map iteration order changes under preserve_order feature
// unification. The previous JSON builder followed that order, which scripts
// can observe through Object.keys; probe the actual map implementation once.
fn sorted_json_keys() -> bool {
    static SORTED: OnceLock<bool> = OnceLock::new();
    *SORTED.get_or_init(|| {
        let mut map = serde_json::Map::new();
        map.insert("z".into(), serde_json::Value::Null);
        map.insert("a".into(), serde_json::Value::Null);
        map.keys().next().is_some_and(|key| key == "a")
    })
}

pub(super) fn empty_object(context: &mut Context) -> JsValue {
    JsObject::with_object_proto(context.intrinsics()).into()
}

pub(super) fn build(context: &mut Context, milliseconds: u64, random_seed: u32) -> JsValue {
    // The caller bounds milliseconds to the largest exactly representable JS integer.
    let seconds = milliseconds as f64 / 1000.0;
    let time = if sorted_json_keys() {
        ObjectInitializer::new(context)
            .property(
                js_string!("milliseconds"),
                numeric(milliseconds),
                Attribute::all(),
            )
            .property(js_string!("seconds"), seconds, Attribute::all())
            .build()
    } else {
        ObjectInitializer::new(context)
            .property(js_string!("seconds"), seconds, Attribute::all())
            .property(
                js_string!("milliseconds"),
                numeric(milliseconds),
                Attribute::all(),
            )
            .build()
    };
    let deps = JsArray::new(context);
    let seed = numeric(u64::from(random_seed));
    let input = if sorted_json_keys() {
        ObjectInitializer::new(context)
            .property(js_string!("deps"), deps, Attribute::all())
            .property(js_string!("randomSeed"), seed, Attribute::all())
            .property(js_string!("time"), time, Attribute::all())
            .build()
    } else {
        ObjectInitializer::new(context)
            .property(js_string!("time"), time, Attribute::all())
            .property(js_string!("randomSeed"), seed, Attribute::all())
            .property(js_string!("deps"), deps, Attribute::all())
            .build()
    };
    input.into()
}

pub(super) fn set_dependencies(
    context: &mut Context,
    input: &JsValue,
    values: &[f64],
) -> Result<(), super::BakeError> {
    let mut dependencies = Vec::with_capacity(values.len());
    for &value in values {
        dependencies.push(JsValue::from(
            ObjectInitializer::new(context)
                .property(js_string!("type"), js_string!("float"), Attribute::all())
                .property(js_string!("value"), value, Attribute::all())
                .build(),
        ));
    }
    let dependencies = JsArray::from_iter(dependencies, context);
    input
        .as_object()
        .expect("input builder returns an object")
        .set(js_string!("deps"), dependencies, true, context)
        .map_err(fx_keyframe_bake::script::ScriptError::from)?;
    Ok(())
}

fn numeric(value: u64) -> JsValue {
    match i32::try_from(value) {
        Ok(value) => JsValue::new(value),
        Err(_) => JsValue::new(value as f64),
    }
}
