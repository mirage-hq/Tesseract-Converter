/* Managed headless-adobe JSX body: read back a saved AEP that carries
 * ADBE Apply Color LUT2 (Effect > Utility > Apply Color LUT). Opens the pinned
 * asset "project" in a fresh host, never saves, and returns:
 * - project colour settings (bit depth, working space, linear blending),
 * - every composition (id, name, size, fps, duration) and footage item,
 * - every effect on every layer with each control's ExtendScript readout,
 *   in the receipt shape used by the effect-definition generator
 *   ({matchName, name, properties:[{matchName, name, type, value, ...}]}).
 * Values the scripting API cannot expose keep their error text and a null value.
 */
function jsonValue(value) {
    if (value === undefined || value === null) return null;
    if (value instanceof Array) {
        var list = [];
        for (var i = 0; i < value.length; i++) list.push(jsonValue(value[i]));
        return list;
    }
    if (typeof value === 'number') return isFinite(value) ? value : String(value);
    if (typeof value === 'string' || typeof value === 'boolean') return value;
    return '[' + typeof value + ']';
}

function readProperty(prop) {
    var row = {index: prop.propertyIndex, name: prop.name, matchName: prop.matchName};
    if (prop.propertyType === PropertyType.PROPERTY) {
        row.type = String(prop.propertyValueType);
        row.canVaryOverTime = prop.canVaryOverTime;
        try { row.value = jsonValue(prop.value); } catch (error) { row.value = null; row.valueError = String(error); }
        try { if (prop.hasMin) row.min = jsonValue(prop.minValue); } catch (error) { row.minError = String(error); }
        try { if (prop.hasMax) row.max = jsonValue(prop.maxValue); } catch (error) { row.maxError = String(error); }
        try { row.isDropdownEffect = prop.isDropdownEffect === true; } catch (error) {}
        try { row.numKeys = prop.numKeys; } catch (error) {}
    } else {
        row.type = 'undefined';
        row.canVaryOverTime = null;
        row.properties = [];
        for (var i = 1; i <= prop.numProperties; i++) row.properties.push(readProperty(prop.property(i)));
    }
    return row;
}

function readEffect(comp, layer, effect) {
    var properties = [];
    for (var i = 1; i <= effect.numProperties; i++) properties.push(readProperty(effect.property(i)));
    var row = {matchName: effect.matchName, compositionId: comp.id, composition: comp.name,
               layerId: layer.id, layerName: layer.name, name: effect.name,
               enabled: effect.enabled, properties: properties};
    return row;
}

function plain(value) {
    try { return jsonValue(value); } catch (error) { return 'error: ' + String(error); }
}

var sourcePath = context.assets.project;
if (typeof sourcePath !== 'string') sourcePath = sourcePath.path;
var opened = app.open(new File(sourcePath));
if (!opened) throw new Error('Pinned AEP did not open');
var project = app.project;
var colour = {};
colour.bitsPerChannel = plain(project.bitsPerChannel);
try { colour.workingSpace = plain(project.workingSpace); } catch (error) { colour.workingSpaceError = String(error); }
try { colour.linearBlending = plain(project.linearBlending); } catch (error) { colour.linearBlendingError = String(error); }
try { colour.linearizeWorkingSpace = plain(project.linearizeWorkingSpace); } catch (error) {}
try { colour.colorManagementSystem = plain(project.colorManagementSystem); } catch (error) {}
try { colour.lutInterpolationMethod = plain(project.lutInterpolationMethod); } catch (error) {}
try { colour.ocioConfigurationFile = plain(project.ocioConfigurationFile); } catch (error) {}

var compositions = [];
var footage = [];
var effects = [];
for (var i = 1; i <= project.numItems; i++) {
    var item = project.item(i);
    if (item instanceof CompItem) {
        compositions.push({id: item.id, name: item.name, width: item.width, height: item.height,
                           frameRate: item.frameRate, duration: item.duration, numLayers: item.numLayers});
        for (var l = 1; l <= item.numLayers; l++) {
            var layer = item.layer(l);
            var parade = layer.property('ADBE Effect Parade');
            if (!parade) continue;
            for (var e = 1; e <= parade.numProperties; e++) effects.push(readEffect(item, layer, parade.property(e)));
        }
    } else if (item instanceof FootageItem) {
        var row = {id: item.id, name: item.name};
        try { row.file = item.file ? item.file.fsName : null; } catch (error) { row.fileError = String(error); }
        try { row.missing = item.footageMissing; } catch (error) {}
        footage.push(row);
    }
}
return {result: {version: app.version, build: app.buildNumber, colour: colour,
                 compositions: compositions, footage: footage, effects: effects}, targets: []};
