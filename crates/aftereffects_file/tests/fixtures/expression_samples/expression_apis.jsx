/* Managed run_jsx body (managed-jsx/v1). Independent Adobe-native authoring of
 * the expression-API fixture: deterministic expressions using ease/easeIn/easeOut,
 * velocity/speed, parented toComp/fromComp, loopOutDuration, smooth, loops and
 * operators, Contents and Mask lookups, layer attributes, color helpers and
 * lookAt. Pre-expression and evaluated values are read back before the save.
 */
function plain(value) {
    if (value instanceof Array) {
        var copy = [];
        for (var i = 0; i < value.length; i++) copy.push(value[i]);
        return copy;
    }
    return value;
}
function easeAll(property) {
    var type = property.propertyValueType;
    var value = property.keyValue(1);
    var dimensions = (type === PropertyValueType.TwoD_SPATIAL || type === PropertyValueType.ThreeD_SPATIAL ||
        !(value instanceof Array)) ? 1 : value.length;
    for (var k = 1; k <= property.numKeys; k++) {
        var easeIn = [], easeOut = [];
        for (var d = 0; d < dimensions; d++) {
            easeIn.push(new KeyframeEase(0, 33.333333));
            easeOut.push(new KeyframeEase(0, 33.333333));
        }
        property.setInterpolationTypeAtKey(k, KeyframeInterpolationType.BEZIER, KeyframeInterpolationType.BEZIER);
        property.setTemporalEaseAtKey(k, easeIn, easeOut);
    }
}
function keyed(property, times, values, eased) {
    for (var i = 0; i < times.length; i++) property.setValueAtTime(times[i], values[i]);
    if (eased) easeAll(property);
    else for (var k = 1; k <= property.numKeys; k++)
        property.setInterpolationTypeAtKey(k, KeyframeInterpolationType.LINEAR, KeyframeInterpolationType.LINEAR);
}
function expression(property, source) {
    property.expression = source;
    property.expressionEnabled = true;
    if (!property.expressionEnabled || property.expression !== source)
        throw new Error("expression did not persist: " + property.name);
    if (property.expressionError) throw new Error("expression error " + property.name + ": " + property.expressionError);
}
var cases = [];
var pending = [];
// Sample only after every layer exists: layer indices and parenting are final.
function sample(comp, layer, property) { pending.push([comp, layer, property]); }
function sampleNow(comp, layer, property) {
    var rows = [];
    var frames = Math.round(comp.duration * comp.frameRate);
    for (var f = 0; f <= frames; f++) {
        var t = f / comp.frameRate;
        rows.push({time: t, pre: plain(property.valueAtTime(t, true)), evaluated: plain(property.valueAtTime(t, false))});
    }
    cases.push({composition_id: comp.id, composition: comp.name, layer_id: layer.id, layer: layer.name,
                match_name: property.matchName, expression: property.expression, samples: rows});
}
function transform(layer, name) {
    return layer.property("ADBE Transform Group").property(name);
}

var project = app.newProject();
if (!project) throw new Error("After Effects did not create a fresh project");
var comp = project.items.addComp("ExpressionApis", 320, 180, 1, 2, 30);
comp.displayStartTime = 0;
comp.bgColor = [0, 0, 0];

// Parent chain for toComp/fromComp.
var parentLayer = comp.layers.addNull(2);
parentLayer.name = "Parent";
keyed(transform(parentLayer, "ADBE Position"), [0, 2], [[60, 90], [200, 90]], true);
transform(parentLayer, "ADBE Rotate Z").setValue(30);
transform(parentLayer, "ADBE Scale").setValue([80, 120]);
transform(parentLayer, "ADBE Anchor Point").setValue([10, 5]);
parentLayer.enabled = false;
var child = comp.layers.addSolid([0.2, 0.6, 1], "Child", 20, 20, 1, 2);
child.parent = parentLayer;
transform(child, "ADBE Position").setValue([40, 10]);

// Contents and Mask sources.
var shapes = comp.layers.addShape();
shapes.name = "Shapes";
shapes.property("ADBE Root Vectors Group").addProperty("ADBE Vector Group");
shapes.property("ADBE Root Vectors Group").property(1).property("ADBE Vectors Group").addProperty("ADBE Vector Shape - Ellipse");
shapes.property("ADBE Root Vectors Group").property(1).property("ADBE Vectors Group").addProperty("ADBE Vector Graphic - Fill");
var groupPosition = shapes.property("ADBE Root Vectors Group").property(1)
    .property("ADBE Vector Transform Group").property("ADBE Vector Position");
keyed(groupPosition, [0, 2], [[-40, 0], [40, 20]], true);
var masked = comp.layers.addSolid([0.95, 0.35, 0.08], "Masked", 320, 180, 1, 2);
masked.property("ADBE Mask Parade").addProperty("ADBE Mask Atom");
var outline = new Shape();
outline.vertices = [[20, 20], [100, 20], [100, 80], [20, 80]];
outline.closed = true;
masked.property("ADBE Mask Parade").property(1).property("ADBE Mask Shape").setValue(outline);
keyed(masked.property("ADBE Mask Parade").property(1).property("ADBE Mask Opacity"), [0, 2], [100, 20], true);

function probe(name) {
    var layer = comp.layers.addSolid([1, 1, 1], name, 10, 10, 1, 2);
    return layer;
}
var p1 = probe("Probe ToComp");
expression(transform(p1, "ADBE Position"), 'thisComp.layer("Child").toComp([0,0])');
sample(comp, p1, transform(p1, "ADBE Position"));
expression(transform(p1, "ADBE Rotate Z"), 'var p=thisComp.layer("Child").fromComp([160,90]); p[0]+p[1]');
sample(comp, p1, transform(p1, "ADBE Rotate Z"));

var p2 = probe("Probe Ease");
expression(transform(p2, "ADBE Rotate Z"), "ease(time,0,2,0,180)");
sample(comp, p2, transform(p2, "ADBE Rotate Z"));
expression(transform(p2, "ADBE Opacity"), "easeIn(time,0.5,1.5,20,100)");
sample(comp, p2, transform(p2, "ADBE Opacity"));
expression(transform(p2, "ADBE Scale"), "easeOut(time,0,2,[50,50],[100,150])");
sample(comp, p2, transform(p2, "ADBE Scale"));

var p3 = probe("Probe Velocity");
keyed(transform(p3, "ADBE Position"), [0, 2], [[60, 140], [260, 140]], true);
expression(transform(p3, "ADBE Rotate Z"), "transform.position.speed/10");
sample(comp, p3, transform(p3, "ADBE Rotate Z"));
expression(transform(p3, "ADBE Opacity"), "50 + transform.position.velocity[0]/10");
sample(comp, p3, transform(p3, "ADBE Opacity"));

var p4 = probe("Probe Loop");
keyed(transform(p4, "ADBE Position"), [0, 1], [[60, 40], [120, 40]], false);
expression(transform(p4, "ADBE Position"), 'loopOutDuration("pingpong",1)');
sample(comp, p4, transform(p4, "ADBE Position"));
keyed(transform(p4, "ADBE Rotate Z"), [0, 1], [0, 90], false);
expression(transform(p4, "ADBE Rotate Z"), "smooth(0.3,7)");
sample(comp, p4, transform(p4, "ADBE Rotate Z"));

var p5 = probe("Probe Loops");
expression(transform(p5, "ADBE Rotate Z"), "var s=0; for (var i=0;i<5;i++){ s+=i*time; } s % 7 + radiansToDegrees(Math.PI/4)");
sample(comp, p5, transform(p5, "ADBE Rotate Z"));
expression(transform(p5, "ADBE Opacity"), "var c = hslToRgb(rgbToHsl([0.2,0.4,0.6,1])); c[1]*100");
sample(comp, p5, transform(p5, "ADBE Opacity"));

var p6 = probe("Probe Lookup");
expression(transform(p6, "ADBE Position"), 'thisComp.layer("Shapes").content("Group 1").transform.position + [100,50]');
sample(comp, p6, transform(p6, "ADBE Position"));
expression(transform(p6, "ADBE Opacity"), 'thisComp.layer("Masked").mask("Mask 1").maskOpacity');
sample(comp, p6, transform(p6, "ADBE Opacity"));
expression(transform(p6, "ADBE Rotate Z"), 'thisComp.layer("Child").width + (thisComp.layer("Child").active ? 1 : 0) + thisLayer.index');
sample(comp, p6, transform(p6, "ADBE Rotate Z"));

var p7 = probe("Probe LookAt");
p7.threeDLayer = true;
transform(p7, "ADBE Position").setValue([160, 90, 0]);
expression(transform(p7, "ADBE Orientation"), "lookAt(position, [260,40,-300])");
sample(comp, p7, transform(p7, "ADBE Orientation"));

for (var q = 0; q < pending.length; q++) sampleNow(pending[q][0], pending[q][1], pending[q][2]);
var destination = new File(context.output_path);
project.save(destination);
if (!project.file || project.file.fsName !== destination.fsName)
    throw new Error("After Effects did not save the fixture to the managed output path");
return {result: {app_version: app.version, fps: 30, cases: cases},
        targets: [{id: "ExpressionApis", name: "ExpressionApis", native_id: String(comp.id)}]};
