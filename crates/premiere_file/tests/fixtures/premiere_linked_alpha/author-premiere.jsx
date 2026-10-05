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


if(app.projects.numProjects!==0)throw new Error("Expected empty owned host");
app.newProject(context.output_path);requireOwnedPremiereProject(context.output_path);
if(typeof app.project.importAEComps!=="function")throw new Error("Premiere importAEComps native API unavailable");
var raw=app.project.importAEComps(context.assets.aep,["Linked Alpha source"],app.project.rootItem);
var items=[],matches=[];
function scan(bin){for(var i=0;i<bin.children.numItems;i++){var child=bin.children[i];items.push({name:child.name,type:child.type,offline:child.type===1?child.isOffline():null});if(child.type===2){scan(child);}else if(child.name.indexOf("Linked Alpha source")===0){matches.push(child);}}}
scan(app.project.rootItem);
if(matches.length!==1||matches[0].isOffline()!==false)throw new Error("Native AE target inventory: "+JSON.stringify(items));
var item=matches[0];
app.project.createNewSequenceFromClips("Premiere linked Alpha15",[item],app.project.rootItem);
var sequence=app.project.activeSequence;var clip=sequence.videoTracks[0].clips[0];
var a=new Time();a.seconds=0.5;var b=new Time();b.seconds=1.5;var e=new Time();e.seconds=2;var start=new Time();start.seconds=1;
clip.inPoint=a;clip.outPoint=b;clip.start=start;clip.end=e;
for(var t=0;t<sequence.audioTracks.numTracks;t++){for(var c=0;c<sequence.audioTracks[t].clips.numItems;c++){var sound=sequence.audioTracks[t].clips[c];sound.inPoint=a;sound.outPoint=b;sound.start=start;sound.end=e;}}
app.enableQE();var invert=qe.project.getVideoEffectByName("Invert");if(!invert)throw new Error("Invert absent");
var track=qe.project.getActiveSequence().getVideoTrackAt(0), clips=[];for(var q=0;q<track.numItems;q++){var item=track.getItemAt(q);if(item.type==="Clip")clips.push(item);}if(clips.length!==1)throw new Error("Expected exactly one QE clip after placement gap");clips[0].addVideoEffect(invert);
function alphaEffect(){var c=app.project.activeSequence.videoTracks[0].clips[0];for(var n=0;n<c.components.numItems;n++){if(c.components[n].matchName==="AE.ADBE Invert")return c.components[n];}throw new Error("Invert component absent");}
var effect=alphaEffect();effect.properties[0].setValue(15,true);effect.properties[1].setValue(0,true);
var uid=sequence.sequenceID,observations={};savePremiereProject(context.output_path,null,observations);closePremiereProject(context.output_path,observations);
if(app.openDocument(context.output_path,true,true,true,true)!==true)throw new Error("Reopen failed");
requireOwnedPremiereProject(context.output_path);clip=app.project.activeSequence.videoTracks[0].clips[0];
if(clip.inPoint.seconds!==0.5||clip.outPoint.seconds!==1.5||clip.start.seconds!==1||clip.end.seconds!==2)throw new Error("trim not saved");
effect=alphaEffect();if(effect.properties[0].getValue()!==15||effect.properties[1].getValue()!==0)throw new Error("saved clip Alpha15 mismatch");
return {result:{channel:15,blend:0,startTicks:clip.start.ticks,sequence:uid,importReturn:String(raw),itemName:clip.projectItem.name,inTicks:clip.inPoint.ticks,outTicks:clip.outPoint.ticks,endTicks:clip.end.ticks,observations:observations},targets:[{id:"main",name:"Premiere linked Alpha15",native_id:uid}]};
