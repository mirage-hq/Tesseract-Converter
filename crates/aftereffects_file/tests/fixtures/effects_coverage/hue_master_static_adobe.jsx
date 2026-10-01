// Adobe-native fixture authoring body. Supply DESTINATION as a new .aep path
// and invoke runAudioCase with a fresh Adobe project in an exclusively owned
// session. Extracted from the executed probe; only the local path is externalized.
function runAudioCase(project) {
  var target = new File(DESTINATION);
  if (target.exists) throw new Error('Refusing to overwrite existing AEP');
  var comp = project.items.addComp('hueMasterStatic', 320, 180, 1, 2, 24);
  comp.bgColor = [0, 0, 0];
  var layer = comp.layers.addSolid([48/255, 100/255, 151/255], 'Adobe-authored colored subject', 120, 80, 1, 2);
  layer.property('ADBE Transform Group').property('ADBE Position').setValue([160, 90]);
  var effect = layer.property('ADBE Effect Parade').addProperty('ADBE HUE SATURATION');
  if (!effect) throw new Error('Adobe cannot add native Hue/Saturation');
  var result = {compositionId: comp.id, layerId: layer.id, effect: effect.matchName, controls: [], attempts: []};
  for (var i = 4; i <= 10; i++) {
    var id = 'ADBE HUE SATURATION-' + ('000' + i).slice(-4), prop = effect.property(id);
    if (prop) result.controls.push({name: id, label: prop.name, value: prop.value, canVaryOverTime: prop.canVaryOverTime});
  }
  var settings = [['ADBE HUE SATURATION-0007', 0], ['ADBE HUE SATURATION-0004', 50], ['ADBE HUE SATURATION-0005', -60], ['ADBE HUE SATURATION-0006', 20]];
  for (var j = 0; j < settings.length; j++) {
    var prop = effect.property(settings[j][0]);
    if (!prop) throw new Error('Missing native control ' + settings[j][0]);
    var issue = null;
    try { prop.setValue(settings[j][1]); } catch (error) { issue = String(error); }
    result.attempts.push({name: settings[j][0], requested: settings[j][1], valueAfter: prop.value, canVaryOverTime: prop.canVaryOverTime, error: issue});
  }
  var hue = effect.property('ADBE HUE SATURATION-0004');
  if (hue.canVaryOverTime) {
    try { hue.setValueAtTime(0, 50); hue.setValueAtTime(1, 90); result.keyProbe = {count: hue.numKeys, valueAtTime: hue.valueAtTime(0.5, false)}; }
    catch (e) { result.keyProbe = {error: String(e)}; }
  } else result.keyProbe = {unavailable: true};
  project.save(target);
  if (!target.exists || !project.file || project.file.fsName !== target.fsName) throw new Error('Adobe failed to save own source');
  result.saved = target.fsName;
  result.bytes = target.length;
  return result;
}
