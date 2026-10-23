/* Managed headless-adobe JSX body authoring one After Effects colour
 * fixture per call (data.fixture). One 1920x1080 30 fps 2 s composition holding the
 * pinned chart picture (asset "media") or a solid, the case's native effect(s)
 * and controls, saved to context.output_path; every touched control is read
 * back. Native errors are recorded as text per step; a failed required step throws.
 */
function text(value) {
    try { return String(value && value.message ? value.message : value); } catch (ignored) { return 'unprintable'; }
}
function plain(value) {
    if (value === undefined || value === null) return null;
    if (value instanceof Array) { var list = []; for (var i = 0; i < value.length; i++) list.push(plain(value[i])); return list; }
    if (typeof value === 'number') return isFinite(value) ? value : String(value);
    if (typeof value === 'string' || typeof value === 'boolean') return value;
    return '[' + typeof value + ']';
}
function readValue(prop) {
    try { return plain(prop.value); } catch (error) { return 'ERR ' + text(error); }
}
function setParam(effect, matchName, value) {
    var row = {param: matchName, requested: value};
    var prop = effect.property(matchName);
    if (!prop) throw new Error('Missing control ' + matchName + ' on ' + effect.matchName);
    row.name = prop.name;
    try { prop.setValue(value); row.set = true; } catch (error) { row.set = false; row.error = text(error); }
    row.observed = readValue(prop);
    return row;
}
function addEffect(layer, matchName) {
    var effect = layer.property('ADBE Effect Parade').addProperty(matchName);
    if (!effect || effect.matchName !== matchName) throw new Error('Effect unavailable: ' + matchName);
    return effect;
}

var project = app.project, data = context.data, steps = {};
if (!project || project.numItems !== 0) throw new Error('Expected the owned empty project');
if (data.project_color) {
    var pc = data.project_color;
    steps.workingSpaceBefore = plain(project.workingSpace);
    try { steps.listColorProfiles = plain(project.listColorProfiles()); } catch (error) { steps.listColorProfiles = 'ERR ' + text(error); }
    try { project.bitsPerChannel = pc.bits_per_channel; } catch (error) { steps.bitsError = text(error); }
    try { project.workingSpace = pc.working_space; } catch (error) { steps.workingSpaceError = text(error); }
    steps.bitsPerChannel = plain(project.bitsPerChannel);
    steps.workingSpace = plain(project.workingSpace);
    try { steps.linearBlending = plain(project.linearBlending); } catch (error) {}
    if (steps.workingSpace !== pc.working_space || steps.bitsPerChannel !== pc.bits_per_channel)
        throw new Error('Project colour settings did not apply: ' + steps.workingSpace + ' / ' + steps.bitsPerChannel +
                        ' errors ' + steps.workingSpaceError + ' / ' + steps.bitsError + ' profiles ' + String(steps.listColorProfiles).substring(0, 3000));
}
var comp = project.items.addComp(data.comp_name, 1920, 1080, 1, 2, 30);
var layer;
if (data.solid_color) {
    layer = comp.layers.addSolid(data.solid_color, 'Solid', 1920, 1080, 1, 2);
    steps.solidColor = plain(layer.source.mainSource.color);
} else {
    var footage = project.importFile(new ImportOptions(new File(context.assets.media)));
    if (!footage) throw new Error('Chart import failed');
    layer = comp.layers.add(footage, 2);
    layer.startTime = 0;
    layer.outPoint = 2;
}
var applied = [];
for (var e = 0; e < data.effects.length; e++) {
    var spec = data.effects[e], effect = addEffect(layer, spec.match_name), rows = [];
    for (var p = 0; spec.values && p < spec.values.length; p++) rows.push(setParam(effect, spec.values[p].param, spec.values[p].value));
    applied.push({effect: spec.match_name, index: effect.propertyIndex, values: rows});
}
if (data.compositing) {
    var c = data.compositing, comp_steps = {};
    var mask = layer.property('ADBE Mask Parade').addProperty('ADBE Mask Atom');
    var shape = new Shape();
    shape.vertices = c.mask_vertices; shape.closed = true;
    mask.property('ADBE Mask Shape').setValue(shape);
    mask.maskMode = MaskMode.NONE;
    comp_steps.maskIndex = mask.propertyIndex;
    comp_steps.maskMode = String(mask.maskMode);
    var target = layer.property('ADBE Effect Parade').property(1);
    var builtIn = target.property('ADBE Effect Built In Params');
    if (!builtIn) throw new Error('Compositing Options group unavailable');
    var names = [];
    for (var b = 1; b <= builtIn.numProperties; b++) names.push(builtIn.property(b).matchName + ':' + builtIn.property(b).name);
    comp_steps.builtInParams = names;
    try { builtIn.property('ADBE Effect Mask Opacity').setValue(c.effect_opacity); comp_steps.opacity = readValue(builtIn.property('ADBE Effect Mask Opacity')); }
    catch (error) { comp_steps.opacityError = text(error); }
    try {
        var parade = builtIn.property('ADBE Effect Mask Parade'), ref = null, can = {};
        if (!parade) throw new Error('ADBE Effect Mask Parade unavailable');
        for (var k = 0; k < c.mask_reference_candidates.length; k++) {
            var candidate = c.mask_reference_candidates[k];
            try { can[candidate] = parade.canAddProperty(candidate); } catch (error) { can[candidate] = 'ERR ' + text(error); }
            if (!ref && can[candidate] === true) ref = parade.addProperty(candidate);
        }
        comp_steps.canAdd = can;
        if (!ref) throw new Error('No mask reference candidate can be added');
        var refNames = [];
        for (var r = 1; r <= ref.numProperties; r++) refNames.push(ref.property(r).matchName + ':' + ref.property(r).name);
        comp_steps.maskReference = {matchName: ref.matchName, children: refNames};
        var leaf = ref.property(1);
        leaf.setValue(mask.propertyIndex);
        comp_steps.maskReference.value = readValue(leaf);
    } catch (error) { comp_steps.maskReferenceError = text(error); }
    steps.compositing = comp_steps;
}
var output = new File(context.output_path);
var saved = project.save(output);
if (!output.exists) throw new Error('AEP not saved; save returned ' + String(saved));
return {result: {version: app.version, fixture: data.fixture, compositionId: comp.id, compositionName: comp.name,
                 layer: layer.name, applied: applied, steps: steps},
        targets: [{id: 'main', name: comp.name, native_id: String(comp.id)}]};
