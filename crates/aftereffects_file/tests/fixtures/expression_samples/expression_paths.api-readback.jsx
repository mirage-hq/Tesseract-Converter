/* Trusted managed-jsx/v1: read the pinned private native source; scratch
 * expressions are never saved back to either input or original artifact. */
function plainShape(value) {
    return {vertices:value.vertices,in_tangents:value.inTangents,
        out_tangents:value.outTangents,closed:value.closed};
}
function path(layer) {
    return layer.property("ADBE Root Vectors Group").property("Geometry")
        .property("ADBE Vectors Group").property("Outline").property("ADBE Vector Shape");
}
var project=app.open(new File(context.assets.project));
if (!project) throw new Error("Private pinned source did not open");
var comp=project.itemByID(context.data.composition_id);
if (!comp || !(comp instanceof CompItem) || comp.name!=="ExpressionPaths")
    throw new Error("Pinned target identity mismatch");
var scratch=comp.layers.addNull(2); scratch.name="NativePathReadback";
var definitions=[
    ["UnusedHandles","var p=thisComp.layer('Guide').content('Geometry').content('Outline').path; [p.inTangents(time)[0][0],p.outTangents(time)[2][0]]"],
    ["Points","thisComp.layer('Guide').content('Geometry').content('Outline').path.points(time)[0]"],
    ["ExplicitPoints","thisComp.layer('Guide').content('Geometry').content('Outline').path.points(0.5)[0]"]
];
for(var i=0;i<definitions.length;i++) {
    var effect=scratch.property("ADBE Effect Parade").addProperty("ADBE Point Control");
    effect.name=definitions[i][0];
    effect.property("ADBE Point Control-0001").expression=definitions[i][1];
}
var times=[0,1/60,1/30,2/30,0.1,0.5,1,1.5,2],cases=[],raw=[];
for(var i=0;i<definitions.length;i++) {
    var property=scratch.property("ADBE Effect Parade").property(definitions[i][0]).property("ADBE Point Control-0001");
    var rows=[];
    for(var j=0;j<times.length;j++) {
        var value=property.valueAtTime(times[j],false);
        if(property.expressionError) throw new Error(definitions[i][0]+": "+property.expressionError);
        rows.push({time:times[j],value:value});
    }
    cases.push({name:definitions[i][0],expression:definitions[i][1],samples:rows});
}
for(var i=0;i<times.length;i++) raw.push({time:times[i],value:plainShape(path(comp.layer("Guide")).valueAtTime(times[i],true))});
var defaults=path(comp.layer("Defaults"));
defaults.expression="createPath([[0,0],[10,20],[30,0]],[[12,8],[0,0],[0,0]],[[0,0],[0,0],[18,5]],false)";
var direct=defaults.valueAtTime(0,false);
if(defaults.expressionError) throw new Error("Direct open handles: "+defaults.expressionError);
return {result:{application_version:app.version,composition_id:comp.id,
    guide_layer_id:comp.layer("Guide").id,cases:cases,raw_guide:raw,
    direct_open_create_path:plainShape(direct)},targets:[{id:"paths",name:comp.name,native_id:String(comp.id)}]};
