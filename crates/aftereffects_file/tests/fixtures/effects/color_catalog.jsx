/* Managed headless-adobe JSX body for the color-effect catalog.
 * mode "author": one 320x180 30fps 2 s composition per requested effect, one
 * solid, the effect applied with Adobe defaults (no setter is called), save to
 * context.output_path, and read back every control.
 * mode "readback": open the pinned saved catalog (asset "catalog") in a fresh
 * host and read back every effect control without saving.
 * mode "installed": list app.effects (display name, match name, category).
 * Readback is a raw ExtendScript observation: groups are nested, values that
 * the scripting API cannot expose keep their error text and a null value.
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
    return {matchName: effect.matchName, compositionId: comp.id, composition: comp.name,
            layerId: layer.id, name: effect.name, properties: properties};
}

function readProject(project) {
    var effects = [];
    for (var i = 1; i <= project.numItems; i++) {
        var comp = project.item(i);
        if (!(comp instanceof CompItem)) continue;
        for (var l = 1; l <= comp.numLayers; l++) {
            var parade = comp.layer(l).property('ADBE Effect Parade');
            if (!parade) continue;
            for (var e = 1; e <= parade.numProperties; e++) effects.push(readEffect(comp, comp.layer(l), parade.property(e)));
        }
    }
    return effects;
}

var project = app.project;
var mode = context.data.mode;
var effects;
var targets = [];
if (mode === 'author') {
    if (!project || project.numItems !== 0) throw new Error('author mode requires the owned empty project');
    var names = context.data.effects;
    var authored = [];
    for (var n = 0; n < names.length; n++) {
        var comp = project.items.addComp('Effect_' + n, 320, 180, 1, 2, 30);
        var solid = comp.layers.addSolid([0.5, 0.5, 0.5], 'Solid_' + n, 320, 180, 1, 2);
        var effect = solid.property('ADBE Effect Parade').addProperty(names[n]);
        if (!effect || effect.matchName !== names[n]) throw new Error('Effect unavailable: ' + names[n]);
        authored.push(comp);
    }
    effects = readProject(project);
    var output = new File(context.output_path);
    var saved = project.save(output);
    if (!output.exists) throw new Error('Catalog AEP was not saved; save returned ' + String(saved));
    for (var t = 0; t < authored.length; t++)
        targets.push({id: 'effect_' + t, name: authored[t].name, native_id: String(authored[t].id)});
} else if (mode === 'readback') {
    var catalogPath = context.assets.catalog;
    if (typeof catalogPath !== 'string') catalogPath = catalogPath.path;
    var opened = app.open(new File(catalogPath));
    if (!opened) throw new Error('Pinned catalog AEP did not open');
    effects = readProject(app.project);
} else if (mode === 'installed') {
    // Installed effect inventory (app.effects), used to resolve native match names.
    effects = [];
    for (var k = 0; k < app.effects.length; k++) {
        var installed = app.effects[k];
        effects.push({displayName: installed.displayName, matchName: installed.matchName,
                      category: installed.category});
    }
} else {
    throw new Error('Unknown mode ' + mode);
}
return {result: {mode: mode, version: app.version, build: app.buildNumber,
                 bitsPerChannel: app.project.bitsPerChannel, effects: effects}, targets: targets};
