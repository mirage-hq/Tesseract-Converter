/* Trusted managed-jsx/v1 body. Independent Adobe-native controls; no transport,
 * foreign paths, render queue or receipt writes. */
function transform(layer, name) { return layer.property("ADBE Transform Group").property(name); }
function expression(property, source) {
    property.expression = source;
    property.expressionEnabled = true;
    if (!property.expressionEnabled || property.expression !== source)
        throw new Error("Expression was not retained: " + property.name);
}
function solid(comp, name, y) {
    var layer = comp.layers.addSolid([0.2, 0.65, 1], name, 16, 16, 1, 1);
    transform(layer, "ADBE Position").setValue([40, y]);
    return layer;
}
var project = app.newProject();
if (!project) throw new Error("No fresh project");
var comp = project.items.addComp("ExpressionCompatibility", 320, 180, 1, 1, 30);
comp.bgColor = [0,0,0];
var subject = comp.layers.addSolid([1, 0.4, 0.2], "Subject", 32, 24, 1, 1);
subject.threeDLayer = true;
transform(subject, "ADBE Position").setValue([160,90,80]);
transform(subject, "ADBE Orientation").setValue([10,15,20]);
transform(subject, "ADBE Rotate X").setValue(25);
transform(subject, "ADBE Rotate Y").setValue(-35);
transform(subject, "ADBE Rotate Z").setValue(15);
transform(subject, "ADBE Scale").setValue([120,80,110]);
var title = comp.layers.addText("Lottie"); title.name = "Title";
var titleText = title.property("ADBE Text Properties").property("ADBE Text Document");
var document = titleText.value;
document.font = "ArialMT"; document.fontSize = 24; document.fillColor = [0.2,0.8,1];
titleText.setValue(document);
transform(title, "ADBE Position").setValue([150,160]);
var loop = solid(comp,"Loop",25);
transform(loop,"ADBE Rotate Z").setValueAtTime(0,0);
transform(loop,"ADBE Rotate Z").setValueAtTime(0.4,120);
for(var k=1;k<=2;k++) transform(loop,"ADBE Rotate Z").setInterpolationTypeAtKey(k,KeyframeInterpolationType.LINEAR,KeyframeInterpolationType.LINEAR);
var probeDefinitions = [
 ["World", "thisComp.layer('Subject').toWorld([10,-5,3])", "ADBE Position", true],
 ["WorldRoundtrip", "var l=thisComp.layer('Subject'); var p=l.fromWorld(l.toWorld([10,-5,3])); p[0]*10+p[1]+p[2]", "ADBE Rotate Z", false],
 ["WorldVector", "var p=thisComp.layer('Subject').toWorldVec([1,2,3]); p[0]*10+p[1]*2+p[2]", "ADBE Rotate Z", false],
 ["VectorRoundtrip", "var l=thisComp.layer('Subject'); var p=l.fromWorldVec(l.toWorldVec([1,2,3])); p[0]*10+p[1]*2+p[2]", "ADBE Rotate Z", false],
 ["PropertyLoop", "thisComp.layer('Loop').transform.rotation.loopOut('pingpong')", "ADBE Rotate Z", false],
 ["ScalarLength", "length(-7,3)*5", "ADBE Rotate Z", false],
 ["ZeroPosterize", "posterizeTime(0); value + time*100", "ADBE Rotate Z", false],
 ["SourceStyle", "thisComp.layer('Title').text.sourceText.style.fillColor[0]*100", "ADBE Rotate Z", false],
 ["TextBounds", "thisComp.layer('Title').sourceRectAtTime(time,false).width", "ADBE Rotate Z", false]
];
var probes=[];
for(var i=0;i<probeDefinitions.length;i++) {
 var definition=probeDefinitions[i], layer=solid(comp,definition[0],15+i*17);
 if(definition[3]) layer.threeDLayer=true;
 var property=transform(layer,definition[2]);
 expression(property,definition[1]);
 probes.push({layer:layer,property:property});
}
var echo=comp.layers.addText("Echo"); echo.name="Echo";
var echoText=echo.property("ADBE Text Properties").property("ADBE Text Document");
var echoDocument=echoText.value; echoDocument.font="ArialMT"; echoDocument.fontSize=24;
echoText.setValue(echoDocument);
transform(echo,"ADBE Position").setValue([150,130]);
expression(echoText,"thisComp.layer('Title').text.sourceText + '!'");
probes.push({layer:echo,property:echoText});
var cases=[];
for(var i=0;i<probes.length;i++) {
 var probe=probes[i], rows=[];
 for(var frame=0;frame<=30;frame++) {
  var time=frame/30, value=probe.property.valueAtTime(time,false);
  if(probe.property.expressionError) throw new Error(probe.layer.name+": "+probe.property.expressionError);
  if(value instanceof TextDocument) value=value.text;
  rows.push({time:time,value:value});
 }
 cases.push({layer_id:probe.layer.id,layer:probe.layer.name,match_name:probe.property.matchName,expression:probe.property.expression,samples:rows});
}
project.save(new File(context.output_path));
if(!project.file || project.file.fsName !== new File(context.output_path).fsName) throw new Error("Native save failed");
return {result:{application_version:app.version,composition_id:comp.id,fps:comp.frameRate,duration:comp.duration,width:comp.width,height:comp.height,cases:cases},targets:[{id:"compatibility",name:comp.name,native_id:String(comp.id)}]};
