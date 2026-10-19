/* Trusted managed-jsx/v1 body: independent native Path API authoring/readback.
 * No transport, foreign paths, render queue, preferences or receipt writes. */
function transform(layer, name) { return layer.property("ADBE Transform Group").property(name); }
function shapeValue(vertices, incoming, outgoing, closed) {
    var value = new Shape();
    value.vertices = vertices; value.inTangents = incoming;
    value.outTangents = outgoing; value.closed = closed;
    return value;
}
function pathLayer(comp, name, value, color, position) {
    var layer = comp.layers.addShape(); layer.name = name;
    transform(layer, "ADBE Position").setValue(position || [160,90]);
    var group = layer.property("ADBE Root Vectors Group").addProperty("ADBE Vector Group");
    group.name = "Geometry";
    var contents = group.property("ADBE Vectors Group");
    var shape = contents.addProperty("ADBE Vector Shape - Group"); shape.name = "Outline";
    shape.property("ADBE Vector Shape").setValue(value);
    var stroke = contents.addProperty("ADBE Vector Graphic - Stroke");
    stroke.property("ADBE Vector Stroke Color").setValue(color);
    stroke.property("ADBE Vector Stroke Width").setValue(3);
    // Reacquire after indexed Contents mutations invalidate old handles.
    return layer;
}
function path(layer) {
    return layer.property("ADBE Root Vectors Group").property("Geometry")
        .property("ADBE Vectors Group").property("Outline").property("ADBE Vector Shape");
}
function setExpression(property, source) {
    property.expression = source; property.expressionEnabled = true;
    if (!property.expressionEnabled || property.expression !== source)
        throw new Error("Expression was not retained: " + property.name);
}
function linear(property) {
    for (var i=1;i<=property.numKeys;i++)
        property.setInterpolationTypeAtKey(i,KeyframeInterpolationType.LINEAR,KeyframeInterpolationType.LINEAR);
}
function plainShape(value) {
    return {vertices:value.vertices,in_tangents:value.inTangents,
        out_tangents:value.outTangents,closed:value.closed};
}
function marker(comp,name,color,source) {
    var layer=comp.layers.addSolid(color,name,8,8,1,2);
    setExpression(transform(layer,"ADBE Position"),source);
    return layer;
}
var project=app.newProject();
if (!project) throw new Error("No fresh project");
var comp=project.items.addComp("ExpressionPaths",320,180,1,2,30);
comp.bgColor=[0,0,0];
var incoming=[[12,8],[-26,-2],[-8,32]], outgoing=[[60,-4],[20,-40],[18,5]];
var initial=shapeValue([[-105,-25],[-55,40],[5,-5]],incoming,outgoing,false);
var guide=pathLayer(comp,"Guide",initial,[1,0.45,0.1]);
path(guide).setValueAtTime(0,initial);
path(guide).setValueAtTime(2,shapeValue([[-85,-25],[-35,40],[25,-5]],incoming,outgoing,false));
linear(path(guide));
var guideSelector="thisComp.layer('Guide').content('Geometry').content('Outline').path";
var copied=pathLayer(comp,"CopiedOpen",initial,[0.15,0.8,1]);
setExpression(path(copied),"var p="+guideSelector+"; var a=p.points(); for(var i=0;i<a.length;i++){a[i]=[a[i][0],a[i][1]+25];} createPath(a,p.inTangents(),p.outTangents(),p.isClosed())");
var created=pathLayer(comp,"CreatedClosed",shapeValue([[-105,-35],[-60,-70],[-5,-30]],[[0,0],[0,0],[0,0]],[[0,0],[0,0],[0,0]],true),[0.7,1,0.1]);
setExpression(path(created),"var x=15*time; createPath([[-105+x,-35],[-60+x,-70],[-5+x,-30]],[[-10,-15],[-20,0],[0,15]],[[0,-15],[20,0],[10,15]],true)");
var timed=pathLayer(comp,"TimedCopy",initial,[0.9,0.2,0.7],[160,70]);
setExpression(path(timed),guideSelector+".valueAtTime(0.5)");
var maskSource=comp.layers.addSolid([0.8,0.15,0.2],"MaskSource",160,100,1,2);
transform(maskSource,"ADBE Position").setValue([80,50]);
var mask=maskSource.property("ADBE Mask Parade").addProperty("ADBE Mask Atom"); mask.name="GuideMask";
var maskInitial=shapeValue([[20,10],[110,15],[100,80],[15,75]],[[-8,3],[-12,0],[0,-18],[10,2]],[[32,-2],[14,25],[-10,7],[-3,-20]],true);
var maskPath=mask.property("ADBE Mask Shape");
maskPath.setValueAtTime(0,maskInitial);
maskPath.setValueAtTime(2,shapeValue([[30,10],[120,15],[110,80],[25,75]],maskInitial.inTangents,maskInitial.outTangents,true));
linear(maskPath);
var maskCopy=comp.layers.addSolid([0.2,0.35,0.85],"CopiedMask",160,100,1,2);
transform(maskCopy,"ADBE Position").setValue([240,130]);
var targetMask=maskCopy.property("ADBE Mask Parade").addProperty("ADBE Mask Atom"); targetMask.name="OutputMask";
targetMask.property("ADBE Mask Shape").setValue(maskInitial);
setExpression(targetMask.property("ADBE Mask Shape"),"var p=thisComp.layer('MaskSource').mask('GuideMask').maskPath; createPath(p.points(),p.inTangents(),p.outTangents(),p.isClosed())");
var point=marker(comp,"Point",[1,1,1],"var p="+guideSelector+".pointOnPath(time/2); [160+p[0],90+p[1]]");
var tangent=marker(comp,"Tangent",[1,0.8,0],"var p="+guideSelector+".tangentOnPath(time/2); [270+25*p[0],30+25*p[1]]");
var normal=marker(comp,"Normal",[0,1,0.7],"var p="+guideSelector+".normalOnPath(time/2); [270+25*p[0],80+25*p[1]]");
var dependent=marker(comp,"Dependent",[1,0.2,1],"var p=thisComp.layer('CopiedOpen').content('Geometry').content('Outline').path.pointOnPath(0.5); [160+p[0],90+p[1]]");
var probes=[
    {layer:copied,property:path(copied),kind:"shape"},
    {layer:created,property:path(created),kind:"shape"},
    {layer:timed,property:path(timed),kind:"shape"},
    {layer:maskCopy,property:maskCopy.property("ADBE Mask Parade").property("OutputMask").property("ADBE Mask Shape"),kind:"mask"},
    {layer:point,property:transform(point,"ADBE Position"),kind:"numeric"},
    {layer:tangent,property:transform(tangent,"ADBE Position"),kind:"numeric"},
    {layer:normal,property:transform(normal,"ADBE Position"),kind:"numeric"},
    {layer:dependent,property:transform(dependent,"ADBE Position"),kind:"numeric"}
];
var cases=[];
for(var i=0;i<probes.length;i++) {
    var probe=probes[i],rows=[];
    for(var frame=0;frame<=60;frame++) {
        var t=frame/30,value=probe.property.valueAtTime(t,false);
        if(probe.property.expressionError) throw new Error(probe.layer.name+": "+probe.property.expressionError);
        rows.push({time:t,value:probe.kind==="numeric"?value:plainShape(value)});
    }
    cases.push({layer_id:probe.layer.id,layer:probe.layer.name,match_name:probe.property.matchName,
        kind:probe.kind,expression:probe.property.expression,samples:rows});
}
// A separate native expression probe resolves omitted createPath closure/defaults.
var defaults=pathLayer(comp,"Defaults",initial,[0.4,0.4,0.4]); defaults.enabled=false;
setExpression(path(defaults),"createPath([[0,0],[10,20],[30,0]])");
var defaultValue=path(defaults).valueAtTime(0,false);
if(path(defaults).expressionError) throw new Error("Defaults: "+path(defaults).expressionError);
var sources=[{layer_id:guide.id,layer:guide.name,kind:"shape",initial:plainShape(path(guide).valueAtTime(0,true)),
    timed:plainShape(path(guide).valueAtTime(0.5,true))},
    {layer_id:maskSource.id,layer:maskSource.name,kind:"mask",width:160,height:100,
    initial:plainShape(maskSource.property("ADBE Mask Parade").property("GuideMask").property("ADBE Mask Shape").valueAtTime(0,true))}];
project.save(new File(context.output_path));
if(!project.file || project.file.fsName!==new File(context.output_path).fsName) throw new Error("Native save failed");
return {result:{application_version:app.version,composition_id:comp.id,fps:comp.frameRate,
    duration:comp.duration,width:comp.width,height:comp.height,cases:cases,sources:sources,
    create_path_defaults:plainShape(defaultValue)},
    targets:[{id:"paths",name:comp.name,native_id:String(comp.id)}]};
