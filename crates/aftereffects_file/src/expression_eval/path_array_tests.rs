//! Copied Path array admission and defensive runtime checks, not Adobe proof.
use super::compile;
use crate::expression_eval::ScriptRuntime;
use boa_engine::JsValue;

fn run(source: &str) -> serde_json::Value {
    let mut runtime = ScriptRuntime::new().unwrap();
    let script = format!(
        "{}\nlet failure=null;const path=__aePathRuntime(message=>{{failure=message;throw Error(message);}},()=>{{}},x=>x);\n{}",
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
fn native_copied_open_source_compiles() {
    let compiled = compile("var p=thisLayer.content('Shape').content('Path').path; var a=p.points(); for(var i=0;i<a.length;i++){a[i]=[a[i][0],a[i][1]+25];} createPath(a,p.inTangents(),p.outTangents(),false);").unwrap();
    assert!(compiled.contains("__aePathSet"));
    assert!(compiled.contains("__aePathGet"));
    assert!(compiled.contains("__aeAdd"));
}

#[test]
fn copied_array_aliases_and_row_compounds_compile() {
    compile("var a=points();var b=a;var row=b[0];var i=1;row[i]+=25;createPath(b);").unwrap();
}

#[test]
fn unrelated_objects_and_escape_indices_are_not_admitted() {
    for source in [
        "var a=[[0,0]];var i=0;a[i]=[1,2];",
        "var a=points();a.length=0;",
        "var a=points();a.__proto__=[1,2];",
        "var a=points();a['constructor']=[1,2];",
        "var a=points();a[0.5]=[1,2];",
        "var a=points();var i=0;a[i++]=[1,2];",
        "var a=points();a=[[1,2]];",
        "var a=points();var row=a[0];row=[1,2];",
        "thisProperty.value[0]=1;",
        "__aePathSet(points(),0);",
    ] {
        assert!(compile(source).is_err(), "admitted {source}");
    }
}

#[test]
fn editable_copies_do_not_mutate_native_snapshots() {
    let value = run(
        "const original=path.createPath([[0,0],[10,20]],[],[],false);const api=path.methods(()=>original,()=>0,x=>x);const a=api.points();for(let i=0;i<a.length;i++){path.set(path.slot(a,i),[path.get(path.get(a,i),0),path.get(path.get(a,i),1)+25]);}const result=path.createPath(a,api.inTangents(),api.outTangents(),false);return [result.vertices,original.vertices,api.points()];",
    );
    assert_eq!(
        value,
        serde_json::json!([[[0, 25], [10, 45]], [[0, 0], [10, 20]], [[0, 0], [10, 20]]])
    );
}

#[test]
fn handles_rows_and_compound_rhs_order_are_defensive() {
    let value = run(
        "const original=path.native({vertices:[[0,0]],in_tangents:[[2,3]],out_tangents:[[4,5]],closed:true});const api=path.methods(()=>original,()=>0,x=>x);const a=api.inTangents();const row=path.get(a,0);const reference=path.slot(row,0);path.set(path.slot(row,0),100);path.set(reference,5,(a,b)=>a+b);const out=api.outTangents();path.set(path.slot(path.get(out,0),1),9);return [a,out,original.in_tangents,original.out_tangents];",
    );
    assert_eq!(
        value,
        serde_json::json!([[[7, 3]], [[4, 9]], [[2, 3]], [[4, 5]]])
    );
}

#[test]
fn runtime_rejects_invalid_indices_missing_brands_and_native_writes_stickily() {
    let value = run(
        "const original=path.createPath([[0,0]],[],[],false);const a=path.methods(()=>original,()=>0,x=>x).points();let rejected=0;for(const action of [()=>path.get(a,'__proto__'),()=>path.get(a,0.5),()=>path.get(a,1),()=>path.get(a,-1),()=>path.get([[0,0]],0),()=>path.slot(original.vertices,0),()=>path.set(path.slot(a,0),[1,Infinity]),()=>path.set(path.slot(path.get(a,0),0),NaN),()=>path.set(path.slot(a,0),[,1])]){failure=null;try{action();}catch(error){if(failure!==null)rejected++;}}return [rejected,original.vertices];",
    );
    assert_eq!(value, serde_json::json!([9, [[0, 0]]]));
}
