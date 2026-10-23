/* Managed headless-adobe JSX body authoring one After Effects fixture per call.
 * data.project_color = {bits_per_channel, working_space, linear_blending, linearize} is applied
 * explicitly and read back (the host default is not trusted). data.comp = {name, width, height,
 * duration, fps}. data.bands = list of {name, y, height, ramp: bool, color: [r,g,b], effects:
 * [{match_name, values: [{param, value}]}]}: each band is one solid layer, optionally carrying a
 * horizontal black-to-white Gradient Ramp before its effects. Saved to context.output_path; every
 * touched control is read back. A failed required step throws.
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
function setParam(effect, matchName, value) {
    var prop = effect.property(matchName);
    if (!prop) throw new Error('Missing control ' + matchName + ' on ' + effect.matchName);
    prop.setValue(value);
    var observed = plain(prop.value), want = plain(value);
    var a = observed instanceof Array ? observed : [observed], w = want instanceof Array ? want : [want];
    for (var i = 0; i < w.length; i++) {
        if (typeof a[i] !== 'number' || Math.abs(a[i] - w[i]) > 1e-6) throw new Error(matchName + ' read back ' + String(observed) + ' not ' + String(want));
    }
    return {param: matchName, name: prop.name, observed: observed};
}
function addEffect(layer, matchName) {
    var effect = layer.property('ADBE Effect Parade').addProperty(matchName);
    if (!effect || effect.matchName !== matchName) throw new Error('Effect unavailable: ' + matchName);
    return effect;
}

var project = app.project, data = context.data, steps = {};
if (!project || project.numItems !== 0) throw new Error('Expected the owned empty project');
var pc = data.project_color;
steps.before = {bits: plain(project.bitsPerChannel), workingSpace: plain(project.workingSpace)};
project.bitsPerChannel = pc.bits_per_channel;
project.workingSpace = pc.working_space;
project.linearBlending = pc.linear_blending;
try { project.linearizeWorkingSpace = pc.linearize; } catch (error) { steps.linearizeError = text(error); }
steps.after = {bits: plain(project.bitsPerChannel), workingSpace: plain(project.workingSpace),
               linearBlending: plain(project.linearBlending)};
try { steps.after.linearize = plain(project.linearizeWorkingSpace); } catch (error) {}
if (steps.after.workingSpace !== pc.working_space || steps.after.bits !== pc.bits_per_channel ||
    steps.after.linearBlending !== pc.linear_blending) {
    var profiles = '';
    try { profiles = String(project.listColorProfiles()).substring(0, 3000); } catch (error) { profiles = text(error); }
    throw new Error('Project colour settings did not apply: ' + steps.after.workingSpace + ' / ' + steps.after.bits + ' / ' + steps.after.linearBlending + ' profiles ' + profiles);
}
var c = data.comp;
var comp = project.items.addComp(c.name, c.width, c.height, 1, c.duration, c.fps);
var bands = [];
for (var b = 0; b < data.bands.length; b++) {
    var spec = data.bands[b];
    var layer = comp.layers.addSolid(spec.color, spec.name, c.width, spec.height, 1, c.duration);
    layer.property('ADBE Transform Group').property('ADBE Position').setValue([c.width / 2, spec.y + spec.height / 2]);
    var rows = [];
    if (spec.ramp) {
        var ramp = addEffect(layer, 'ADBE Ramp');
        rows.push(setParam(ramp, 'ADBE Ramp-0001', [0, spec.height / 2]));
        rows.push(setParam(ramp, 'ADBE Ramp-0002', [0, 0, 0, 1]));
        rows.push(setParam(ramp, 'ADBE Ramp-0003', [c.width, spec.height / 2]));
        rows.push(setParam(ramp, 'ADBE Ramp-0004', [1, 1, 1, 1]));
    }
    for (var e = 0; spec.effects && e < spec.effects.length; e++) {
        var es = spec.effects[e], effect = addEffect(layer, es.match_name);
        for (var p = 0; es.values && p < es.values.length; p++) rows.push(setParam(effect, es.values[p].param, es.values[p].value));
    }
    bands.push({name: layer.name, index: layer.index, controls: rows});
}
var output = new File(context.output_path);
project.save(output);
if (!output.exists) throw new Error('AEP not saved');
return {result: {version: app.version, fixture: data.fixture, compositionId: comp.id, steps: steps, bands: bands},
        targets: [{id: 'main', name: comp.name, native_id: String(comp.id)}]};
