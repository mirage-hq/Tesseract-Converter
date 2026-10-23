/* Managed headless-adobe JSX body authoring color_aces_ae_project: an OCIO
 * colour-managed After Effects project (explicit bit depth, colour engine, OCIO
 * config and working space from context.data), one 1920x1080 30 fps 4 s
 * composition holding the in-house chart (0-2 s, Lumetri Exposure +1) and the
 * in-house S-Log3/S-Gamut3.Cine clip (2-4 s) with the footage colour interpretation
 * requested in context.data, saved to context.output_path. Every colour property
 * the scripting API exposes on the project and the footage is read back; native
 * errors are recorded per step and a required step that fails throws.
 */
function text(value) {
    try { return String(value && value.message ? value.message : value); } catch (ignored) { return 'unprintable'; }
}
function plain(value) {
    if (value === undefined || value === null) return null;
    if (value instanceof Array) { var list = []; for (var i = 0; i < value.length && i < 400; i++) list.push(plain(value[i])); return list; }
    if (typeof value === 'number') return isFinite(value) ? value : String(value);
    if (typeof value === 'string' || typeof value === 'boolean') return value.length > 4000 ? value.substring(0, 4000) : value;
    return '[' + typeof value + ']';
}
function reflect(object) {
    var rows = {};
    try {
        var props = object.reflect.properties;
        for (var i = 0; i < props.length; i++) {
            var name = String(props[i].name);
            if (name === '__proto__' || name === 'constructor' || name === 'hasOwnProperty' || name === 'reflect' ||
                    name === 'items' || name === 'rootFolder' || name === 'selection' || name === 'layers') continue;
            try { rows[name] = plain(object[name]); } catch (error) { rows[name] = 'ERR ' + text(error); }
        }
    } catch (error2) { rows.reflectError = text(error2); }
    return rows;
}
function methods(object) {
    var names = [];
    try { var list = object.reflect.methods; for (var i = 0; i < list.length; i++) names.push(String(list[i].name)); }
    catch (error) { names.push('ERR ' + text(error)); }
    return names;
}
function enumKeys(holder) {
    var rows = {};
    try { for (var key in holder) rows[key] = plain(holder[key]); } catch (error) { rows.error = text(error); }
    return rows;
}
function attempt(steps, key, fn) {
    try { steps[key] = plain(fn()); } catch (error) { steps[key] = 'ERR ' + text(error); }
}
function addLayer(comp, path, start) {
    var footage = app.project.importFile(new ImportOptions(new File(path)));
    if (!footage) throw new Error('Import failed: ' + path);
    var layer = comp.layers.add(footage, 2);
    layer.startTime = start;
    layer.inPoint = start;
    layer.outPoint = start + 2;
    return {footage: footage, layer: layer};
}

var project = app.project, data = context.data, steps = {};
if (!project || project.numItems !== 0) throw new Error('Expected the owned empty project');
steps.projectBefore = reflect(project);
attempt(steps, 'colorManagementSystemEnum', function () {
    return typeof ColorManagementSystem === 'undefined' ? 'undefined' : enumKeys(ColorManagementSystem);
});
attempt(steps, 'projectMethods', function () { return methods(project); });
attempt(steps, 'setBitsPerChannel', function () { project.bitsPerChannel = data.bits_per_channel; return project.bitsPerChannel; });
attempt(steps, 'setColorManagementSystem', function () {
    // AE 26.5 exposes the property but no ColorManagementSystem enum: 0 Adobe, 1 OCIO.
    project.colorManagementSystem = data.color_management_system;
    return project.colorManagementSystem;
});
attempt(steps, 'ocioConfigurationFileBefore', function () { return project.ocioConfigurationFile; });
if (data.ocio_configuration_file)
    attempt(steps, 'setOcioConfigurationFile', function () { project.ocioConfigurationFile = data.ocio_configuration_file; return project.ocioConfigurationFile; });
attempt(steps, 'listColorProfiles', function () { return project.listColorProfiles(); });
attempt(steps, 'setWorkingSpace', function () { project.workingSpace = data.working_space; return project.workingSpace; });
steps.projectAfter = reflect(project);

var comp = project.items.addComp(data.comp_name, 1920, 1080, 1, 4, 30);
var chart = addLayer(comp, context.assets.media, 0), log = addLayer(comp, context.assets.log, 2);
steps.chartSource = reflect(chart.footage.mainSource);
steps.logSourceBefore = reflect(log.footage.mainSource);
steps.logFootageBefore = reflect(log.footage);
steps.footageMethods = methods(log.footage);
steps.sourceMethods = methods(log.footage.mainSource);
for (var p = 0; data.footage_settings && p < data.footage_settings.length; p++) {
    (function (setting) {
        attempt(steps, 'log.' + setting.property, function () {
            var owner = setting.on === 'footage' ? log.footage : log.footage.mainSource;
            owner[setting.property] = setting.value;
            return owner[setting.property];
        });
    })(data.footage_settings[p]);
}
steps.logSourceAfter = reflect(log.footage.mainSource);

var lumetri = chart.layer.property('ADBE Effect Parade').addProperty('ADBE Lumetri');
if (!lumetri || lumetri.matchName !== 'ADBE Lumetri') throw new Error('Lumetri unavailable');
var exposure = lumetri.property('ADBE Lumetri-0011');
exposure.setValue(data.exposure);
steps.exposure = {name: exposure.name, value: plain(exposure.value)};

var output = new File(context.output_path);
var saved = project.save(output);
if (!output.exists) throw new Error('AEP not saved; save returned ' + String(saved));
steps.projectSaved = reflect(project);
var required = steps.projectAfter;
if (required.bitsPerChannel !== data.bits_per_channel)
    throw new Error('Bit depth did not apply: ' + required.bitsPerChannel);
return {result: {version: app.version, fixture: 'color_aces_ae_project', compositionId: comp.id, compositionName: comp.name,
                 steps: steps},
        targets: [{id: 'main', name: comp.name, native_id: String(comp.id)}]};
