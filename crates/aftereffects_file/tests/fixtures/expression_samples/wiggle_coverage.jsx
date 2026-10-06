/* Managed run_jsx body (managed-jsx/v1). Independent Adobe-native authoring of
 * the wiggle-coverage fixture: eased/spatial keyed Position, separated Position,
 * 3D X Rotation/Orientation and a Bezier-keyed wiggle control. Pre-expression and
 * evaluated values are read back from Adobe before the single save.
 */
function plain(value) {
    if (value instanceof Array) {
        var copy = [];
        for (var i = 0; i < value.length; i++) copy.push(value[i]);
        return copy;
    }
    return value;
}
function easeAll(property, dimensions) {
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
function keyed(property, times, values) {
    for (var i = 0; i < times.length; i++) property.setValueAtTime(times[i], values[i]);
}
function expression(property, source) {
    property.expression = source;
    property.expressionEnabled = true;
    if (!property.expressionEnabled || property.expression !== source)
        throw new Error("expression did not persist: " + property.name);
    if (property.expressionError) throw new Error("expression error: " + property.expressionError);
}
function sample(comp, layer, property, matchName) {
    var rows = [];
    var frames = Math.round(comp.duration * comp.frameRate);
    for (var f = 0; f <= frames; f++) {
        var t = f / comp.frameRate;
        rows.push({time: t, pre: plain(property.valueAtTime(t, true)), evaluated: plain(property.valueAtTime(t, false))});
    }
    return {composition_id: comp.id, composition: comp.name, layer_id: layer.id, layer: layer.name,
            match_name: matchName, expression: property.expression, samples: rows};
}

var project = app.newProject();
if (!project) throw new Error("After Effects did not create a fresh project");
var times = [0, 1, 2];
var path = [[60, 90], [160, 40], [260, 90]];
var cases = [];
var targets = [];

function newComp(name) {
    var comp = project.items.addComp(name, 320, 180, 1, 2, 30);
    comp.displayStartTime = 0;
    comp.bgColor = [0, 0, 0];
    targets.push({id: name, name: name, native_id: String(comp.id)});
    return comp;
}
function subject(comp) {
    return comp.layers.addSolid([0.95, 0.35, 0.08], "Subject", 24, 24, 1, 2);
}
function easedPosition(layer) {
    var position = layer.property("ADBE Transform Group").property("ADBE Position");
    keyed(position, times, path);
    for (var k = 1; k <= position.numKeys; k++) position.setSpatialAutoBezierAtKey(k, true);
    easeAll(position, 1);
    return position;
}

// 1. Base: eased spatial Position read through `value`.
var base = newComp("WiggleBase");
var baseLayer = subject(base);
var basePosition = easedPosition(baseLayer);
expression(basePosition, "value");
cases.push(sample(base, baseLayer, basePosition, "ADBE Position"));

// 2. The same keys with wiggle.
var wigglePositionComp = newComp("WigglePosition");
var wiggleLayer = subject(wigglePositionComp);
var wigglePosition = easedPosition(wiggleLayer);
expression(wigglePosition, "wiggle(3,20)");
cases.push(sample(wigglePositionComp, wiggleLayer, wigglePosition, "ADBE Position"));

// 3. Separated Position: eased X with wiggle, static Y.
var separatedComp = newComp("WiggleSeparated");
var separatedLayer = subject(separatedComp);
var separatedPosition = separatedLayer.property("ADBE Transform Group").property("ADBE Position");
separatedPosition.dimensionsSeparated = true;
var xPosition = separatedLayer.property("ADBE Transform Group").property("ADBE Position_0");
var yPosition = separatedLayer.property("ADBE Transform Group").property("ADBE Position_1");
keyed(xPosition, [0, 2], [60, 260]);
easeAll(xPosition, 1);
yPosition.setValue(90);
expression(xPosition, "wiggle(2,15)");
cases.push(sample(separatedComp, separatedLayer, xPosition, "ADBE Position_0"));

// 4. 3D layer: eased X Rotation and static Orientation, both wiggled.
var threeComp = newComp("Wiggle3D");
var threeLayer = subject(threeComp);
threeLayer.threeDLayer = true;
var xRotation = threeLayer.property("ADBE Transform Group").property("ADBE Rotate X");
keyed(xRotation, [0, 2], [0, 60]);
easeAll(xRotation, 1);
expression(xRotation, "wiggle(2,10)");
var orientation = threeLayer.property("ADBE Transform Group").property("ADBE Orientation");
expression(orientation, "wiggle(1,15)");
cases.push(sample(threeComp, threeLayer, xRotation, "ADBE Rotate X"));
cases.push(sample(threeComp, threeLayer, orientation, "ADBE Orientation"));

// 5. wiggle frequency read from a Bezier-keyed Slider.
var controlComp = newComp("WiggleBezierControl");
var controller = controlComp.layers.addNull(2);
controller.name = "Controller";
var sliderEffect = controller.property("ADBE Effect Parade").addProperty("ADBE Slider Control");
sliderEffect.name = "Frequency";
var slider = sliderEffect.property("ADBE Slider Control-0001");
keyed(slider, [0, 2], [1, 5]);
easeAll(slider, 1);
controller.enabled = false;
var controlLayer = subject(controlComp);
var rotation = controlLayer.property("ADBE Transform Group").property("ADBE Rotate Z");
expression(rotation, 'wiggle(thisComp.layer("Controller").effect("Frequency")("Slider"), 20)');
cases.push(sample(controlComp, controlLayer, rotation, "ADBE Rotate Z"));
cases.push(sample(controlComp, controller, slider, "ADBE Slider Control-0001"));

var destination = new File(context.output_path);
project.save(destination);
if (!project.file || project.file.fsName !== destination.fsName)
    throw new Error("After Effects did not save the fixture to the managed output path");
return {result: {app_version: app.version, fps: 30, cases: cases}, targets: targets};
