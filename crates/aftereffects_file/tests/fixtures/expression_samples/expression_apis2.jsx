/* Managed run_jsx body (managed-jsx/v1). Independent Adobe-native authoring of
 * the second expression-API fixture: Source Text string expressions, 3D layer
 * space (toComp/fromComp with X/Y rotation and Orientation) and Shape-layer
 * sourceRectAtTime. Evaluated values are read back after all layers exist.
 */
function plain(value) {
    if (value instanceof Array) {
        var copy = [];
        for (var i = 0; i < value.length; i++) copy.push(value[i]);
        return copy;
    }
    return value;
}
function expression(property, source) {
    property.expression = source;
    property.expressionEnabled = true;
    if (!property.expressionEnabled || property.expression !== source)
        throw new Error("expression did not persist: " + property.name);
    if (property.expressionError) throw new Error("expression error " + property.name + ": " + property.expressionError);
}
function transform(layer, name) { return layer.property("ADBE Transform Group").property(name); }
var pending = [];
function sample(comp, layer, property) { pending.push([comp, layer, property]); }
var cases = [];
function sampleNow(comp, layer, property) {
    var rows = [];
    var frames = Math.round(comp.duration * comp.frameRate);
    for (var f = 0; f <= frames; f++) {
        var t = f / comp.frameRate;
        var evaluated = property.valueAtTime(t, false);
        if (evaluated instanceof TextDocument) evaluated = evaluated.text;
        rows.push({time: t, evaluated: plain(evaluated)});
    }
    cases.push({composition_id: comp.id, composition: comp.name, layer_id: layer.id, layer: layer.name,
                match_name: property.matchName, expression: property.expression, samples: rows});
}

var project = app.newProject();
if (!project) throw new Error("After Effects did not create a fresh project");
var comp = project.items.addComp("ExpressionApis2", 320, 180, 1, 2, 30);
comp.displayStartTime = 0;
comp.bgColor = [0, 0, 0];

// Source Text strings.
var counter = comp.layers.addText("0");
counter.name = "Counter";
var counterText = counter.property("ADBE Text Properties").property("ADBE Text Document");
var counterDoc = counterText.value;
counterDoc.font = "ArialMT";
counterDoc.fontSize = 40;
counterText.setValue(counterDoc);
expression(counterText, 'Math.round(linear(time,0,2,0,100)) + "%"');
sample(comp, counter, counterText);
var timer = comp.layers.addText("0");
timer.name = "Timer";
var timerText = timer.property("ADBE Text Properties").property("ADBE Text Document");
expression(timerText, 'time.toFixed(2) + "s"');
sample(comp, timer, timerText);

// 3D layer space.
var spinner = comp.layers.addSolid([0.2, 0.6, 1], "Spinner", 40, 40, 1, 2);
spinner.threeDLayer = true;
transform(spinner, "ADBE Position").setValue([160, 90, 50]);
transform(spinner, "ADBE Rotate X").setValue(30);
transform(spinner, "ADBE Rotate Y").setValue(45);
transform(spinner, "ADBE Rotate Z").setValue(20);
transform(spinner, "ADBE Orientation").setValue([10, 0, 15]);
transform(spinner, "ADBE Scale").setValue([120, 80, 100]);
var probe3d = comp.layers.addSolid([1, 1, 1], "Probe 3D", 10, 10, 1, 2);
probe3d.threeDLayer = true;
expression(transform(probe3d, "ADBE Position"), 'thisComp.layer("Spinner").toComp([0,0,0])');
sample(comp, probe3d, transform(probe3d, "ADBE Position"));
expression(transform(probe3d, "ADBE Rotate Z"), 'var p=thisComp.layer("Spinner").fromComp([100,60,0]); p[0]+p[1]+p[2]');
sample(comp, probe3d, transform(probe3d, "ADBE Rotate Z"));

// Shape sourceRectAtTime.
var box = comp.layers.addShape();
box.name = "Box";
box.property("ADBE Root Vectors Group").addProperty("ADBE Vector Group");
var contents = function () { return box.property("ADBE Root Vectors Group").property(1).property("ADBE Vectors Group"); };
contents().addProperty("ADBE Vector Shape - Rect");
contents().addProperty("ADBE Vector Graphic - Stroke");
contents().addProperty("ADBE Vector Graphic - Fill");
contents().property("ADBE Vector Shape - Rect").property("ADBE Vector Rect Size").setValue([80, 40]);
contents().property("ADBE Vector Shape - Rect").property("ADBE Vector Rect Position").setValue([10, -5]);
contents().property("ADBE Vector Graphic - Stroke").property("ADBE Vector Stroke Width").setValue(6);
var groupTransform = box.property("ADBE Root Vectors Group").property(1).property("ADBE Vector Transform Group");
groupTransform.property("ADBE Vector Position").setValue([20, 10]);
groupTransform.property("ADBE Vector Scale").setValue([150, 100]);
var rectProbe = comp.layers.addSolid([1, 1, 1], "Probe Rect", 10, 10, 1, 2);
expression(transform(rectProbe, "ADBE Position"), 'var r=thisComp.layer("Box").sourceRectAtTime(time,false); [r.left + r.width, r.top + r.height]');
sample(comp, rectProbe, transform(rectProbe, "ADBE Position"));
expression(transform(rectProbe, "ADBE Rotate Z"), 'var r=thisComp.layer("Box").sourceRectAtTime(time,true); r.width + r.height/1000');
sample(comp, rectProbe, transform(rectProbe, "ADBE Rotate Z"));

for (var q = 0; q < pending.length; q++) sampleNow(pending[q][0], pending[q][1], pending[q][2]);
var destination = new File(context.output_path);
project.save(destination);
if (!project.file || project.file.fsName !== destination.fsName)
    throw new Error("After Effects did not save the fixture to the managed output path");
return {result: {app_version: app.version, fps: 30, cases: cases},
        targets: [{id: "ExpressionApis2", name: "ExpressionApis2", native_id: String(comp.id)}]};
