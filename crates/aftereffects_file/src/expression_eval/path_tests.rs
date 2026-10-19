//! Path helper coverage, not independent Adobe fidelity proof.
use super::*;

fn run(source: &str) -> serde_json::Value {
    let mut runtime = ScriptRuntime::new().unwrap();
    let script = format!(
        "{}\nconst notes=[];const path=__aePathRuntime(message=>{{throw Error(message);}},note=>notes.push(note),value=>value);\n{}",
        include_str!("path.js"),
        source
    );
    let input = JsValue::from_json(
        &serde_json::json!({"time":{"seconds":0,"milliseconds":0},"deps":[],"randomSeed":1}),
        runtime.context_mut(),
    )
    .unwrap();
    runtime
        .call(&script, input)
        .unwrap()
        .to_json(runtime.context_mut())
        .unwrap()
        .unwrap()
}

#[test]
fn path_creation_defaults_and_native_unused_handles_are_distinct() {
    let value = run(
        "const original=path.native({vertices:[[0,0],[10,0]],in_tangents:[[12,8],[0,0]],out_tangents:[[0,0],[18,5]],closed:false});
         const api=path.methods(()=>original,()=>0,x=>x);
         const created=path.createPath(original.vertices,original.in_tangents,original.out_tangents,false);
         return [path.createPath([[0,0],[1,0]]).closed,api.inTangents(),api.outTangents(),created.in_tangents,created.out_tangents];",
    );
    assert_eq!(
        value,
        serde_json::json!([
            true,
            [[12, 8], [0, 0]],
            [[0, 0], [18, 5]],
            [[0, 0], [0, 0]],
            [[0, 0], [0, 0]]
        ])
    );
}

#[test]
fn path_api_quantization_does_not_modify_raw_values() {
    let value = run(
        "const original=path.createPath([[-104.66667175,0],[10,0]],undefined,undefined,false);
         const api=path.methods(()=>original,()=>0,x=>x);
         const points=api.points();points[0][0]=999;
         return [original.vertices[0][0],api.points()[0][0],notes];",
    );
    assert_eq!(
        value,
        serde_json::json!([
            -104.66667175,
            -104.6875,
            ["path:points-1/16-pixel", "path:points-1/16-pixel"]
        ])
    );
}

#[test]
fn path_traversal_uses_native_sampled_inversion_and_left_normal() {
    let value = run(
        "const original=path.createPath([[0,0],[100,0]],[[0,0],[-90,0]],[[0,0],[0,0]],false);
         const api=path.methods(()=>original,()=>0,x=>x);
         return [api.pointOnPath(.5),api.tangentOnPath(0),api.normalOnPath(1)];",
    );
    // Native NonuniformLine at .5 is slightly below the ideal arc midpoint
    // (50): chord-table inversion interpolates the cubic parameter.
    assert!((value[0][0].as_f64().unwrap() - 49.9954023707497).abs() < 1e-6);
    assert_eq!(value[0][1], 0.0);
    for (actual, expected) in [(&value[1], [1.0, 0.0]), (&value[2], [0.0, 1.0])] {
        for (actual, expected) in actual.as_array().unwrap().iter().zip(expected) {
            assert_eq!(actual.as_f64().unwrap(), expected);
        }
    }
}

#[test]
fn path_traversal_preserves_clamped_endpoints_and_degenerate_geometry() {
    let value = run(
        "const original=path.createPath([[0,0],[0,0],[100,0]],undefined,undefined,false);
         const api=path.methods(()=>original,()=>0,x=>x);
         const degenerate=path.methods(()=>path.createPath([[3,4],[3,4]],undefined,undefined,false),()=>0,x=>x);
         return [api.pointOnPath(-1),api.pointOnPath(0),api.pointOnPath(1),api.pointOnPath(2),api.tangentOnPath(0),degenerate.pointOnPath(.5),degenerate.tangentOnPath(.5),degenerate.normalOnPath(.5)];",
    );
    assert_eq!(
        value,
        serde_json::json!([
            [0, 0],
            [0, 0],
            [100, 0],
            [100, 0],
            [1, 0],
            [3, 4],
            [0, 0],
            [0, 0]
        ])
    );
}

#[test]
fn path_methods_read_requested_geometry_and_reject_wrong_values() {
    let value = run(
        "const api=path.methods(t=>path.createPath([[t,0],[t+10,0]],undefined,undefined,false),()=>2,x=>x);
         let rejected=0;
         for(const action of [()=>path.requirePath({vertices:[[0,0]]}),()=>path.createPath([[Infinity,0]]),()=>path.createPath([[0,0],[1,0]],[[0,0]],[]),()=>path.interpolate(path.createPath([[0,0]]),path.createPath([[0,0],[1,0]]),.5)]) {
             try {action();} catch(error) {++rejected;}
         }
         return [api.pointOnPath(.5),api.pointOnPath(.5,7),rejected];",
    );
    assert!((value[0][0].as_f64().unwrap() - 7.0).abs() < 1e-6);
    assert!((value[1][0].as_f64().unwrap() - 12.0).abs() < 1e-6);
    assert_eq!(value[2], 4);
}
