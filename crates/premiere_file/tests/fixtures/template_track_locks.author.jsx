// Optional ES3 source snippets for managed Client.run_jsx bodies, not a runner.
// Paths must come from the managed context. Select exact targets in the lane.
function requireOwnedPremiereProject(ownedPath) {
    if (app.projects.numProjects !== 1 || !app.project ||
            new File(app.project.path).fsName !== new File(ownedPath).fsName ||
            new File(app.projects[0].path).fsName !== new File(ownedPath).fsName)
        throw new Error('Expected only the owned private project');
    return app.project;
}

// newPath is null for save(), or the unused context.output_path for saveAs().
// These checks establish a save acknowledgment/file, NOT persisted controls.
function savePremiereProject(ownedPath, newPath, observations) {
    var project = requireOwnedPremiereProject(ownedPath);
    var destination = newPath === null ? ownedPath : newPath;
    if (newPath !== null && new File(destination).exists)
        throw new Error('saveAs destination already exists');
    var raw = newPath === null ? project.save() : project.saveAs(destination);
    observations.save = {method: newPath === null ? 'save' : 'saveAs', return_type: typeof raw, value: raw};
    // Method-specific: documented numeric zero and observed Boolean true.
    if (raw !== 0 && raw !== true) throw new Error('Unverified Project save return: ' + typeof raw + ' ' + String(raw));
    requireOwnedPremiereProject(destination);
    var file = new File(destination);
    if (!file.exists || !(file.length > 0)) throw new Error('Saved project file is missing or empty');
    return raw;
}

function closePremiereProject(ownedPath, observations) {
    var raw = requireOwnedPremiereProject(ownedPath).closeDocument(0, 0);
    observations.close = {return_type: typeof raw, value: raw};
    // Do not reuse any project/clip/parameter handles after this call.
    if (raw !== 0 && raw !== true) throw new Error('Unverified closeDocument return: ' + typeof raw + ' ' + String(raw));
    if (app.projects.numProjects !== 0 || (app.project && app.project.path))
        throw new Error('Owned project did not close');
    return raw;
}

function relinkPremiereItem(ownedPath, item, authoredPath, privatePath, observations) {
    requireOwnedPremiereProject(ownedPath);
    var before = item.getMediaPath(), destination = new File(privatePath).fsName;
    if (typeof before !== 'string' ||
            (new File(before).fsName !== new File(authoredPath).fsName && new File(before).fsName !== destination))
        throw new Error('Selected media is not the pinned dependency');
    if (!new File(destination).exists) throw new Error('Private media is missing');
    if (item.canChangeMediaPath() !== true) throw new Error('Media cannot be relinked');
    var raw = item.changeMediaPath(destination, false);
    observations.relink = {return_type: typeof raw, value: raw};
    if (raw !== 0 && raw !== true) throw new Error('Unverified changeMediaPath return: ' + typeof raw + ' ' + String(raw));
    var after = item.getMediaPath(), offline = item.isOffline();
    if (typeof after !== 'string' || new File(after).fsName !== destination || offline !== false)
        throw new Error('Private media path/online postcondition failed');
    // Initial offline=true is not terminal. Offline=false remains mandatory here.
    // The lane must also compare timing, source type/rate and clip inventory.
    return raw;
}

function readPremiereKeys(parameter) {
    var supported = parameter.areKeyframesSupported(), varying = parameter.isTimeVarying();
    if ((supported !== true && supported !== false) || (varying !== true && varying !== false))
        throw new Error('Key state getters must return Booleans');
    if (!supported) return {status: 'unavailable', reason: 'Keyframes are not supported', keys: null};
    var raw = parameter.getKeys(), rows = [], i, j;
    if (raw === undefined && varying === false)
        return {status: 'unavailable', return_type: 'undefined',
            reason: 'getKeys returned undefined; native key inventory unavailable', keys: null};
    if (raw === 0) return {status: 'observed', return_type: 'number', value: raw, keys: rows};
    // Native collections can be list-shaped objects rather than JavaScript Arrays.
    if (raw === null || typeof raw !== 'object' || typeof raw.length !== 'number' ||
            raw.length % 1 !== 0 || raw.length < 0 || raw.length > 1000)
        throw new Error('getKeys returned an invalid or unbounded key inventory');
    for (i = 0; i < raw.length; i++) {
        var ticks = raw[i] ? raw[i].ticks : undefined;
        if (typeof ticks !== 'string' || !/^(0|-?[1-9][0-9]{0,18})$/.test(ticks))
            throw new Error('Key ticks must be exact canonical strings');
        var magnitude = ticks.charAt(0) === '-' ? ticks.slice(1) : ticks;
        var limit = ticks.charAt(0) === '-' ? '9223372036854775808' : '9223372036854775807';
        if (magnitude.length === 19 && magnitude > limit) throw new Error('Key ticks exceed signed 64-bit range');
        for (j = 0; j < rows.length; j++) if (rows[j].ticks === ticks) throw new Error('Duplicate key ticks');
        var value = parameter.getValueAtKey(raw[i]);
        if (typeof value === 'number' && isFinite(value)) {
            // Keep the raw scalar, including ambiguous zero.
        } else if (value !== null && typeof value === 'object' &&
                typeof value.length === 'number' && value.length % 1 === 0 && value.length >= 2 && value.length <= 4) {
            var vector = [];
            for (j = 0; j < value.length; j++) {
                if (typeof value[j] !== 'number' || !isFinite(value[j])) throw new Error('Invalid key vector');
                vector.push(value[j]);
            }
            value = vector;
        } else throw new Error('getValueAtKey did not return a finite numeric value');
        rows.push({ticks: ticks, value: value, status: value === 0 ? 'ambiguous_zero' : 'observed',
            reason: value === 0 ? 'ComponentParam.getValueAtKey documents 0 as its unsuccessful result; this exact 0 is either a zero-valued key or a failed read' : null});
    }
    return {status: 'observed', return_type: typeof raw, keys: rows};
}
// Shared native Premiere authoring helpers (ES3 managed-JSX body prefix).
function exactNumber(value) {
    return isFinite(value) && !(Math.floor(value) === value && Math.abs(value) > 9007199254740991);
}
function describe(value) {
    if (value === undefined) return {type: 'undefined'};
    if (value === null) return {type: 'null'};
    if (typeof value === 'number') return exactNumber(value) ? {type: 'number', value: value} : {type: 'number', text: String(value)};
    if (typeof value === 'boolean') return {type: 'boolean', value: value};
    if (typeof value === 'string') {
        var codes = [];
        for (var c = 0; c < value.length && c < 16; c++) codes.push(value.charCodeAt(c));
        return {type: 'string', length: value.length, head: value.substring(0, 256), codes: codes};
    }
    if (typeof value === 'object' && typeof value.length === 'number' && value.length <= 4) {
        var list = [];
        for (var i = 0; i < value.length; i++) list.push(typeof value[i] === 'number' && exactNumber(value[i]) ? value[i] : String(value[i]));
        return {type: 'vector', value: list};
    }
    return {type: typeof value};
}
function safe(fn) {
    try { return describe(fn()); } catch (error) { return {error: String(error)}; }
}
function findComponent(clip, matchName) {
    var found = null;
    for (var j = 0; j < clip.components.numItems; j++) {
        if (clip.components[j].matchName === matchName) {
            if (found) throw new Error('Duplicate component ' + matchName);
            found = clip.components[j];
        }
    }
    return found;
}
function firstClip() {
    var sequence = app.project.activeSequence;
    if (!sequence || sequence.videoTracks[0].clips.numItems !== 1) throw new Error('Expected one clip on V1');
    return sequence.videoTracks[0].clips[0];
}
function dump(component) {
    var rows = [];
    for (var i = 0; i < component.properties.numItems; i++) {
        var p = component.properties[i], row = {index: i, displayName: p.displayName};
        try { row.value = describe(p.getValue()); } catch (error) { row.error = String(error); }
        rows.push(row);
    }
    return rows;
}
function mediaItem(path) {
    for (var r = 0; r < app.project.rootItem.children.numItems; r++) {
        var candidate = app.project.rootItem.children[r];
        if (candidate.getMediaPath && candidate.getMediaPath() === path) return candidate;
    }
    return null;
}
// New owned project with one 1920x1080 30 fps sequence holding `mediaPath` trimmed to 2 s.
function setupSequence(sequenceName, mediaPath) {
    if (app.projects.numProjects !== 0) throw new Error('Expected no open project before native authoring');
    app.newProject(context.output_path);
    requireOwnedPremiereProject(context.output_path);
    if (app.project.importFiles([mediaPath], true, app.project.rootItem, false) !== true) throw new Error('Media import failed');
    var item = mediaItem(mediaPath);
    if (!item || item.isOffline() !== false) throw new Error('Missing admitted online media');
    app.project.createNewSequenceFromClips(sequenceName, [item], app.project.rootItem);
    var sequence = app.project.activeSequence;
    if (!sequence || sequence.videoTracks[0].clips.numItems !== 1) throw new Error('Expected one source clip');
    var settings = sequence.getSettings();
    var rate = new Time(); rate.ticks = '8467200000';
    settings.videoFrameRate = rate;
    settings.videoFrameWidth = 1920;
    settings.videoFrameHeight = 1080;
    var settingsReturn = sequence.setSettings(settings);
    var after = sequence.getSettings();
    if (sequence.timebase !== '8467200000' || after.videoFrameWidth !== 1920 || after.videoFrameHeight !== 1080)
        throw new Error('Sequence settings did not apply: ' + sequence.timebase + ' ' + after.videoFrameWidth + 'x' + after.videoFrameHeight);
    var clip = firstClip();
    var end = new Time(); end.ticks = '508032000000';
    clip.end = end;
    clip = firstClip();
    var timing = {start: clip.start.ticks, end: clip.end.ticks, inPoint: clip.inPoint.ticks, outPoint: clip.outPoint.ticks};
    if (timing.start !== '0' || timing.end !== '508032000000') throw new Error('Clip timing did not apply: ' + timing.start + '-' + timing.end);
    return {sequence: sequence, item: item, timing: timing, setSettings: String(settingsReturn)};
}
function addLumetri() {
    app.enableQE();
    var effect = qe.project.getVideoEffectByName('Lumetri Color');
    if (!effect) throw new Error('Native Lumetri Color unavailable');
    qe.project.getActiveSequence().getVideoTrackAt(0).getItemAt(0).addVideoEffect(effect);
    var component = findComponent(firstClip(), 'AE.ADBE Lumetri');
    if (!component) throw new Error('Lumetri component absent');
    return component;
}
// Requires each control at `index` to carry `displayName` (guards against layout drift).
function requireControl(component, index, displayName) {
    var p = component.properties[index];
    if (!p || p.displayName !== displayName)
        throw new Error('Control ' + index + ' is ' + (p ? '"' + p.displayName + '"' : 'absent') + ', expected "' + displayName + '"');
    return p;
}
function setControls(component, targets) {
    var applied = [];
    for (var s = 0; s < targets.length; s++) {
        var t = targets[s], p = requireControl(component, t.index, t.name), raw;
        try { raw = String(p.setValue(t.value, true)); } catch (error) { raw = 'ERR ' + error; }
        applied.push({index: t.index, name: t.name, requested: t.value, setValue: raw, observed: safe(function () { return p.getValue(); })});
    }
    return applied;
}
function saveCloseReopen(returns) {
    savePremiereProject(context.output_path, null, returns);
    closePremiereProject(context.output_path, returns);
    if (app.openDocument(context.output_path, true, true, true, true) !== true) throw new Error('Reopen failed');
    requireOwnedPremiereProject(context.output_path);
}
// template_track_locks: a 1920x1080 29.97 fps Rec. 709 sequence with the chart
// on V1 (sync lock turned off) and on V2 (track lock turned on), both 0-60
// frames; graphics white 203. Saved, closed and reopened natively; the lock
// states are read back through QE after reopening.
function clean(s) {
    s = String(s); var out = '';
    for (var i = 0; i < s.length && i < 600; i++) { var c = s.charCodeAt(i); out += (c >= 32 && c < 127) ? s.charAt(i) : '?'; }
    return out;
}
var TB = '8475667200', END = '508540032000';
function locks() {
    app.enableQE();
    var q = qe.project.getActiveSequence(), rows = [];
    for (var t = 0; t < 2; t++) {
        var track = q.getVideoTrackAt(t);
        rows.push({track: t, isSyncLocked: String(track.isSyncLocked()), isLocked: String(track.isLocked())});
    }
    return rows;
}
function clips(sequence) {
    var rows = [];
    for (var t = 0; t < sequence.videoTracks.numTracks; t++) {
        var track = sequence.videoTracks[t];
        for (var k = 0; k < track.clips.numItems; k++)
            rows.push({track: t, name: clean(track.clips[k].name), start: track.clips[k].start.ticks, end: track.clips[k].end.ticks});
    }
    return rows;
}
if (app.projects.numProjects !== 0) throw new Error('Expected no open project before native authoring');
app.newProject(context.output_path);
requireOwnedPremiereProject(context.output_path);
if (app.project.importFiles([context.assets.media], true, app.project.rootItem, false) !== true) throw new Error('Media import failed');
var item = mediaItem(context.assets.media);
if (!item || item.isOffline() !== false) throw new Error('Missing admitted online media');
app.project.createNewSequenceFromClips(context.data.sequence_name, [item], app.project.rootItem);
var sequence = app.project.activeSequence;
if (!sequence || sequence.videoTracks[0].clips.numItems !== 1) throw new Error('Expected one source clip');
var settings = sequence.getSettings(), list = settings.workingColorSpaceList, target = null;
for (var i = 0; list && i < list.length; i++) if (clean(list[i].name) === context.data.working_space) target = list[i];
if (!target) throw new Error('Working space not offered: ' + context.data.working_space);
var rate = new Time(); rate.ticks = TB;
settings.videoFrameRate = rate; settings.videoFrameWidth = 1920; settings.videoFrameHeight = 1080;
settings.workingColorSpace = target;
var steps = {setSettings: clean(sequence.setSettings(settings))};
var after = sequence.getSettings();
if (sequence.timebase !== TB || after.videoFrameWidth !== 1920 || clean(after.workingColorSpace.name) !== context.data.working_space)
    throw new Error('Sequence settings did not apply');
steps.graphicsWhite = clean(app.project.setGraphicsWhiteLuminance(context.data.graphics_white));
var end = new Time(); end.ticks = END;
sequence.videoTracks[0].clips[0].end = end;
var start = new Time(); start.ticks = '0';
steps.overwrite = clean(sequence.videoTracks[1].overwriteClip(item, start));
if (sequence.videoTracks[1].clips.numItems !== 1) throw new Error('V2 placement failed');
sequence.videoTracks[1].clips[0].end = end;
steps.before = locks();
app.enableQE();
var q = qe.project.getActiveSequence();
steps.setSyncLock = String(q.getVideoTrackAt(0).setSyncLock(false));
steps.setLock = String(q.getVideoTrackAt(1).setLock(true));
steps.after = locks();
var authored = clips(sequence), uid = sequence.sequenceID, returns = {};
saveCloseReopen(returns);
var reopened = {locks: locks(), clips: clips(app.project.activeSequence),
                workingColorSpace: clean(app.project.activeSequence.getSettings().workingColorSpace.name),
                graphicsWhite: app.project.getGraphicsWhiteLuminance()};
if (reopened.locks[0].isSyncLocked !== 'false' || reopened.locks[1].isLocked !== 'true')
    throw new Error('Reopened locks differ: ' + reopened.locks[0].isSyncLocked + '/' + reopened.locks[1].isLocked);
return {result: {version: app.version, steps: steps, authored: authored, reopened: reopened, returns: returns, sequence: uid},
        targets: [{id: 'main', name: context.data.sequence_name, native_id: uid}]};
