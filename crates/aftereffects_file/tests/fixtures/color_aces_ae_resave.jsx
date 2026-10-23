/* Managed headless-adobe JSX body: open the pinned AEP asset "project" (an OCIO
 * colour-managed project whose working space / footage OCIO input colourspaces were set
 * in its saved declarations), read the project colour settings back, and save it natively
 * to context.output_path. No mutator besides save. */
function text(value) { try { return String(value && value.message ? value.message : value); } catch (e) { return 'unprintable'; } }
var file = new File(context.assets.project);
if (!file.exists) throw new Error('Pinned project snapshot missing');
var opened = app.open(file);
if (!opened) throw new Error('AE did not open the project');
var project = app.project, color = {}, comps = [], targets = [], footage = [];
var names = ['bitsPerChannel', 'colorManagementSystem', 'ocioConfigurationFile', 'workingSpace', 'linearBlending',
             'linearizeWorkingSpace', 'lutInterpolationMethod', 'compensateForSceneReferredProfiles', 'workingGamma'];
for (var n = 0; n < names.length; n++) { try { color[names[n]] = project[names[n]]; } catch (e) { color[names[n]] = 'ERR ' + text(e); } }
for (var i = 1; i <= project.numItems; i++) {
    var item = project.item(i);
    if (item instanceof CompItem) {
        comps.push({id: item.id, name: item.name, numLayers: item.numLayers});
        targets.push({id: 'comp' + item.id, name: item.name, native_id: String(item.id)});
    } else if (item instanceof FootageItem) {
        footage.push({id: item.id, name: item.name, missing: item.footageMissing});
    }
}
var output = new File(context.output_path);
var saved = project.save(output);
if (!output.exists) throw new Error('AEP not saved; save returned ' + String(saved));
return {result: {version: app.version, color: color, compositions: comps, footage: footage}, targets: targets};
