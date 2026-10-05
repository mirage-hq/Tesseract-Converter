/* Managed run_jsx body (managed-jsx/v1). Independent Adobe-native authoring of
 * the wiggle-targets fixture: Shape contents on a start-offset layer, Mask
 * Feather/Opacity/Expansion and Text Animator properties, each mixing
 * deterministic eased `value` expressions with wiggle. Pre-expression and
 * evaluated values plus native property paths are read back before the save.
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
function keyed(property, times, values) {
    for (var i = 0; i < times.length; i++) property.setValueAtTime(times[i], values[i]);
    easeAll(property);
}
function expression(property, source) {
    property.expression = source;
    property.expressionEnabled = true;
    if (!property.expressionEnabled || property.expression !== source)
        throw new Error("expression did not persist: " + property.name);
    if (property.expressionError) throw new Error("expression error " + property.name + ": " + property.expressionError);
}
function nativePath(property) {
    var path = [];
    var current = property;
    while (current && current.propertyDepth > 0) {
        path.unshift({index: current.propertyIndex, match_name: current.matchName});
        current = current.parentProperty;
    }
    return path;
}
function sample(comp, layer, property) {
    var rows = [];
    var frames = Math.round(comp.duration * comp.frameRate);
    for (var f = 0; f <= frames; f++) {
        var t = f / comp.frameRate;
        rows.push({time: t, pre: plain(property.valueAtTime(t, true)), evaluated: plain(property.valueAtTime(t, false))});
    }
    return {composition_id: comp.id, composition: comp.name, layer_id: layer.id, layer: layer.name,
            layer_start: layer.startTime, match_name: property.matchName, path: nativePath(property),
            expression: property.expression, samples: rows};
}

var project = app.newProject();
if (!project) throw new Error("After Effects did not create a fresh project");
var cases = [];
var targets = [];
function newComp(name) {
    var comp = project.items.addComp(name, 320, 180, 1, 2, 30);
    comp.displayStartTime = 0;
    comp.bgColor = [0, 0, 0];
    targets.push({id: name, name: name, native_id: String(comp.id)});
    return comp;
}

// 1. Shape contents on a layer starting at 0.5s (layer-local clock).
var shapeComp = newComp("WiggleShape");
var shapeLayer = shapeComp.layers.addShape();
shapeLayer.name = "Shapes";
shapeLayer.startTime = 0.5;
var root = shapeLayer.property("ADBE Root Vectors Group");
root.addProperty("ADBE Vector Group");
var groupContents = function () {
    return shapeLayer.property("ADBE Root Vectors Group").property(1).property("ADBE Vectors Group");
};
groupContents().addProperty("ADBE Vector Shape - Ellipse");
groupContents().addProperty("ADBE Vector Graphic - Stroke");
groupContents().addProperty("ADBE Vector Graphic - Fill");
groupContents().addProperty("ADBE Vector Filter - Trim");
var group = shapeLayer.property("ADBE Root Vectors Group").property(1);
var ellipse = groupContents().property("ADBE Vector Shape - Ellipse");
var stroke = groupContents().property("ADBE Vector Graphic - Stroke");
var fill = groupContents().property("ADBE Vector Graphic - Fill");
var trim = groupContents().property("ADBE Vector Filter - Trim");
var ellipseSize = ellipse.property("ADBE Vector Ellipse Size");
keyed(ellipseSize, [0, 1.5], [[40, 40], [90, 60]]);
expression(ellipseSize, "value");
fill.property("ADBE Vector Fill Color").setValue([0.95, 0.35, 0.08, 1]);
var fillOpacity = fill.property("ADBE Vector Fill Opacity");
fillOpacity.setValue(70);
expression(fillOpacity, "wiggle(2,30)");
stroke.property("ADBE Vector Stroke Color").setValue([0.2, 0.6, 1, 1]);
var strokeWidth = stroke.property("ADBE Vector Stroke Width");
strokeWidth.setValue(6);
expression(strokeWidth, "wiggle(3,4)");
var trimEnd = trim.property("ADBE Vector Trim End");
keyed(trimEnd, [0, 1.5], [100, 40]);
expression(trimEnd, "value");
var groupPosition = group.property("ADBE Vector Transform Group").property("ADBE Vector Position");
keyed(groupPosition, [0, 1.5], [[-60, 0], [60, 0]]);
expression(groupPosition, "wiggle(2,15)");
shapeLayer.property("ADBE Transform Group").property("ADBE Position").setValue([160, 90]);
cases.push(sample(shapeComp, shapeLayer, ellipseSize));
cases.push(sample(shapeComp, shapeLayer, fillOpacity));
cases.push(sample(shapeComp, shapeLayer, strokeWidth));
cases.push(sample(shapeComp, shapeLayer, trimEnd));
cases.push(sample(shapeComp, shapeLayer, groupPosition));

// 2. Mask Feather (eased value), Opacity (wiggle) and Expansion (eased value).
var maskComp = newComp("WiggleMask");
var solid = maskComp.layers.addSolid([0.95, 0.35, 0.08], "Masked", 320, 180, 1, 2);
solid.property("ADBE Mask Parade").addProperty("ADBE Mask Atom");
var mask = solid.property("ADBE Mask Parade").property(1);
var outline = new Shape();
outline.vertices = [[100, 50], [220, 50], [220, 130], [100, 130]];
outline.closed = true;
mask.property("ADBE Mask Shape").setValue(outline);
var feather = mask.property("ADBE Mask Feather");
keyed(feather, [0, 2], [[0, 0], [30, 30]]);
expression(feather, "value");
var maskOpacity = mask.property("ADBE Mask Opacity");
maskOpacity.setValue(80);
expression(maskOpacity, "wiggle(2,30)");
var expansion = mask.property("ADBE Mask Offset");
keyed(expansion, [0, 2], [0, 20]);
expression(expansion, "value");
cases.push(sample(maskComp, solid, feather));
cases.push(sample(maskComp, solid, maskOpacity));
cases.push(sample(maskComp, solid, expansion));

// 3. Text Animator properties (not selectors).
var textComp = newComp("WiggleText");
var textLayer = textComp.layers.addText("Wiggle");
textLayer.name = "Title";
var sourceText = textLayer.property("ADBE Text Properties").property("ADBE Text Document");
var textDoc = sourceText.value;
textDoc.font = "ArialMT";
textDoc.fontSize = 48;
textDoc.applyFill = true;
textDoc.fillColor = [0.95, 0.35, 0.08];
textDoc.justification = ParagraphJustification.CENTER_JUSTIFY;
sourceText.setValue(textDoc);
textLayer.property("ADBE Transform Group").property("ADBE Position").setValue([160, 106]);
textLayer.property("ADBE Text Properties").property("ADBE Text Animators").addProperty("ADBE Text Animator");
var animator = function () {
    return textLayer.property("ADBE Text Properties").property("ADBE Text Animators").property(1);
};
animator().property("ADBE Text Animator Properties").addProperty("ADBE Text Position 3D");
animator().property("ADBE Text Animator Properties").addProperty("ADBE Text Opacity");
animator().property("ADBE Text Selectors").addProperty("ADBE Text Selector");
var textPosition = animator().property("ADBE Text Animator Properties").property("ADBE Text Position 3D");
expression(textPosition, "wiggle(2,20)");
var textOpacity = animator().property("ADBE Text Animator Properties").property("ADBE Text Opacity");
keyed(textOpacity, [0, 2], [100, 30]);
expression(textOpacity, "value");
cases.push(sample(textComp, textLayer, textPosition));
cases.push(sample(textComp, textLayer, textOpacity));

var destination = new File(context.output_path);
project.save(destination);
if (!project.file || project.file.fsName !== destination.fsName)
    throw new Error("After Effects did not save the fixture to the managed output path");
return {result: {app_version: app.version, fps: 30, cases: cases}, targets: targets};
