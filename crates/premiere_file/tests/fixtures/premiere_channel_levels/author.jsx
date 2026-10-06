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

if (app.projects.numProjects !== 0) throw new Error("Expected no open project before native authoring");
app.newProject(context.output_path);
requireOwnedPremiereProject(context.output_path);
if (app.project.importFiles([context.assets.cells],true,app.project.rootItem,false)!==true) throw new Error("PNG import failed");
var item=null;
for(var i=0;i<app.project.rootItem.children.numItems;i++) {var candidate=app.project.rootItem.children[i];if(candidate.getMediaPath && candidate.getMediaPath()===context.assets.cells)item=candidate;}
if(!item || item.isOffline()!==false) throw new Error("Missing admitted online alpha PNG");
app.project.createNewSequenceFromClips("Premiere Channel Levels",[item],app.project.rootItem);
var sequence=app.project.activeSequence;
if(!sequence || sequence.videoTracks[0].clips.numItems!==1) throw new Error("Expected one source picture");
var clip=sequence.videoTracks[0].clips[0]; var end=new Time();end.ticks="508540032000";clip.end=end;
app.enableQE();
var effect=qe.project.getVideoEffectByName("Levels");
if(!effect) throw new Error("Native Invert unavailable");
qe.project.getActiveSequence().getVideoTrackAt(0).getItemAt(0).addVideoEffect(effect);
function readEffect(){var c=app.project.activeSequence.videoTracks[0].clips[0];for(var j=0;j<c.components.numItems;j++){if(c.components[j].matchName==="PR.ADBE Levels")return c.components[j];}throw new Error("Native matchName did not verify");}

var component=readEffect();var values=[30,255,0,255,100,0,200,0,255,2,0,255,0,255,100,0,255,0,255,100];
if(component.properties.numItems!==20)throw new Error("Expected twenty Levels controls");
for(var k=0;k<20;k++){component.properties[k].setValue(values[k],true);if(component.properties[k].getValue()!==values[k])throw new Error("Levels control did not apply: "+k);}
var returns={};var uid=sequence.sequenceID;
savePremiereProject(context.output_path,null,returns);closePremiereProject(context.output_path,returns);
if(app.openDocument(context.output_path,true,true,true,true)!==true)throw new Error("Reopen failed");
requireOwnedPremiereProject(context.output_path);component=readEffect();
for(var k=0;k<20;k++){if(component.properties[k].getValue()!==values[k])throw new Error("Levels control not saved: "+k);}
return {result:{values:values,sequence:uid,returns:returns},targets:[{id:"main",name:"Premiere Channel Levels",native_id:uid}]};
