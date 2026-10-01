/* Adobe-native audio E2E authoring recipe. Parent supplies AUDIO_FIXTURE_DIR,
 * verifies a newly owned empty AE session/project, and invokes buildAep(project),
 * project.save(File(.../audio_cases.aep)), then readbackAep(project). No Adobe
 * rendering, converter writer, session takeover, quit, close or dialog changes.
 * Keep source FPS 24; 30fps applies only to later independent reference renders.
 */
var AUDIO_ROOT_NAMES = [
    "wave-static", "native-muted", "hidden-layer", "hidden-group", "static-zero",
    "constant-zero-zero-base", "constant-zero-nonzero-base", "all-zero-one-key",
    "all-zero-two-keys", "zero-base-unmute", "hold-gain", "linear-gain",
    "bezier-gain", "trim-offset", "affine-playback", "eof-tail",
    "mov-audio-only", "audio-group", "group-affine-playback", "empty-nested-group", "fractional-duration",
    "source-switch", "nonlinear-remap", "conflicting-gain-clock",
    "invalid-gain-hull", "empty-only-group", "stereo-levels", "expression"
];
var AUDIO_DB_HALF = 20 * Math.log(0.5) / Math.LN10;
var AUDIO_DB_FLOOR = -192;

function audioItem(project, name) {
    var options = new ImportOptions(new File(AUDIO_FIXTURE_DIR + "/" + name));
    if (!options.file.exists) throw new Error("missing primary fixture: " + name);
    return project.importFile(options);
}
function audioComp(project, name, length) {
    var c = project.items.addComp(name, 320, 180, 1, length || 6, 24);
    c.displayStartTime = 0;
    c.workAreaStart = 0;
    c.workAreaDuration = c.duration;
    return c;
}
function audioLayer(comp, item, name, start, end, sourceIn) {
    var layer = comp.layers.add(item);
    layer.name = name;
    layer.startTime = start - sourceIn;
    layer.inPoint = start;
    layer.outPoint = end;
    if (!layer.hasAudio) throw new Error("media lacks audio: " + name);
    layer.audioEnabled = true;
    return layer;
}
function audioLevels(layer) {
    var levels = layer.property("ADBE Audio Group").property("ADBE Audio Levels");
    if (!levels) throw new Error("Audio Levels missing: " + layer.name);
    return levels;
}
function level(layer, db) { audioLevels(layer).setValue([db, db]); }
function levelsKeys(layer, points, mode) {
    var p = audioLevels(layer);
    for (var i = 0; i < points.length; i++) p.setValueAtTime(points[i][0], [points[i][1], points[i][1]]);
    for (var k = 1; k <= p.numKeys; k++) {
        var type = mode == "hold" ? KeyframeInterpolationType.HOLD :
            (mode == "linear" ? KeyframeInterpolationType.LINEAR : KeyframeInterpolationType.BEZIER);
        p.setInterpolationTypeAtKey(k, type, type);
        if (mode == "bezier") {
            // Distinct temporal handle influences/speeds; editable native keys,
            // not a baked waveform or reconstructed FX property identity.
            // Explicit oracle: scalar gain controls .55/.95 at temporal .25/.75.
            // This is an authored dB cubic, not exact logarithmic gain playback.
            var incoming = -(20 * Math.log(0.95) / Math.LN10) / 0.25;
            var outgoing = (20 * Math.log(0.55) / Math.LN10 - AUDIO_DB_HALF) / 0.25;
            var inEase = [new KeyframeEase(incoming, 25), new KeyframeEase(incoming, 25)];
            var outEase = [new KeyframeEase(outgoing, 25), new KeyframeEase(outgoing, 25)];
            p.setTemporalEaseAtKey(k, inEase, outEase);
        }
    }
}
function base(project, sound, slug, gain) {
    var comp = audioComp(project, slug, 6);
    var layer = audioLayer(comp, sound, "primary-sound", 1, 3, 0.5);
    level(layer, gain === undefined ? AUDIO_DB_HALF : gain);
    return {comp:comp, layer:layer};
}
function precomp(project, slug, sound, muted, emptyNested, length) {
    var root = audioComp(project, slug, 6);
    var child = audioComp(project, slug + "__audio-child", length || 3);
    var nested = null;
    if (emptyNested) {
        nested = audioComp(project, slug + "__empty-child", length || 3);
        var emptyLayer = child.layers.add(nested);
        emptyLayer.name = "empty-nested-no-audio";
    }
    var inside = audioLayer(child, sound, "child-primary-sound", 1, 3, 0.5);
    level(inside, AUDIO_DB_HALF);
    var parent = root.layers.add(child);
    parent.name = "precomp-audio-container";
    parent.startTime = 0;
    parent.inPoint = 0;
    parent.outPoint = Math.min(3, child.duration);
    parent.audioEnabled = !muted;
    return {root:root, child:child, nested:nested, parent:parent, inside:inside};
}
function buildAep(project) {
    if (project.numItems !== 0 || project.file !== null) throw new Error("requires newly owned empty unsaved project");
    if (typeof AUDIO_FIXTURE_DIR != "string" || !AUDIO_FIXTURE_DIR.length)
        throw new Error("AUDIO_FIXTURE_DIR must be an absolute path");
    var sound = audioItem(project, "sound.wav");
    var other = audioItem(project, "other.wav");
    var movie = audioItem(project, "movie.mov");
    var x = base(project, sound, "wave-static");
    x = base(project, sound, "native-muted"); x.layer.audioEnabled = false;
    x = base(project, sound, "hidden-layer"); x.layer.enabled = false; x.layer.audioEnabled = false;
    precomp(project, "hidden-group", sound, true, false, 3);
    x = base(project, sound, "static-zero", AUDIO_DB_FLOOR); x.layer.audioEnabled = false;
    x = base(project, sound, "constant-zero-zero-base", AUDIO_DB_FLOOR); x.layer.audioEnabled = false;
    x = base(project, sound, "constant-zero-nonzero-base", AUDIO_DB_FLOOR); x.layer.audioEnabled = false;
    x = base(project, sound, "all-zero-one-key", AUDIO_DB_FLOOR);
    levelsKeys(x.layer, [[1.5, AUDIO_DB_FLOOR]], "hold"); x.layer.audioEnabled = false;
    x = base(project, sound, "all-zero-two-keys", AUDIO_DB_FLOOR);
    levelsKeys(x.layer, [[1.5, AUDIO_DB_FLOOR], [2.5, AUDIO_DB_FLOOR]], "hold"); x.layer.audioEnabled = false;
    x = base(project, sound, "zero-base-unmute", AUDIO_DB_FLOOR);
    levelsKeys(x.layer, [[1.5, AUDIO_DB_FLOOR], [2.5, 0]], "hold");
    x = base(project, sound, "hold-gain"); levelsKeys(x.layer, [[1.5, AUDIO_DB_HALF], [2.5, 0]], "hold");
    x = base(project, sound, "linear-gain"); levelsKeys(x.layer, [[1.5, AUDIO_DB_HALF], [2.5, 0]], "linear");
    x = base(project, sound, "bezier-gain"); levelsKeys(x.layer, [[1.5, AUDIO_DB_HALF], [2.5, 0]], "bezier");
    // Plain FX clock: 1..3 active, 0.5 initial source offset, 1x despite
    // sourceRange.duration=1. This native source does not encode that FX-only metadata.
    base(project, sound, "trim-offset");
    x = base(project, sound, "affine-playback");
    x.layer.stretch = 200; // 200% stretch, two seconds comp per one second source
    x.layer.startTime = 0; // source 0.5 is at comp 1 when stretched 200%
    x.layer.inPoint = 1; x.layer.outPoint = 3;
    levelsKeys(x.layer, [[1, AUDIO_DB_HALF], [2, 0]], "hold");
    x = base(project, sound, "eof-tail"); x.layer.outPoint = 6; // audible to 4.5, then silence
    x = base(project, movie, "mov-audio-only"); x.layer.enabled = false; x.layer.audioEnabled = true;
    precomp(project, "audio-group", sound, false, false, 3);
    var affineGroup = precomp(project, "group-affine-playback", sound, false, false, 3);
    affineGroup.parent.stretch = 200;
    affineGroup.parent.startTime = 1;
    affineGroup.parent.inPoint = 1;
    affineGroup.parent.outPoint = 5;
    precomp(project, "empty-nested-group", sound, false, true, 3);
    var fractional = precomp(project, "fractional-duration", sound, false, true, 2 + 1 / 24);
    // Storage rounds up to 49/24 seconds, but audible/occurrence bounds do not.
    fractional.inside.outPoint = 2.001;
    fractional.parent.outPoint = 2.001;
    fractional.child.layer("empty-nested-no-audio").outPoint = 2.001;
    var switchComp = audioComp(project, "source-switch", 6);
    var first = audioLayer(switchComp, sound, "switch-sound-first", 2, 3, 0.5);
    var second = audioLayer(switchComp, other, "switch-other-second", 3, 4, 1.5);
    // Both native occurrences carry the original owner's keys at comp times
    // 2 and 3, including the pre-inPoint key on the second occurrence.
    levelsKeys(first, [[2, AUDIO_DB_HALF], [3, 0]], "hold");
    levelsKeys(second, [[2, AUDIO_DB_HALF], [3, 0]], "hold");
    second.inPoint = 3; second.outPoint = 4; // pre-inPoint key must not extend occurrence
    // Supported nonlinear playback without owned gain keys: four native Time
    // Remap guard/control keys, not a flattened or omitted sibling.
    // MOV carries the exact sound.wav PCM plus disabled picture. AE may not
    // expose Time Remap on audio-only footage; the sound remains independent.
    x = base(project, movie, "nonlinear-remap");
    x.layer.enabled = false;
    x.layer.outPoint = 2;
    if (!x.layer.canSetTimeRemapEnabled) throw new Error("native MOV Time Remap unavailable");
    x.layer.timeRemapEnabled = true;
    var remap = x.layer.property("ADBE Time Remapping");
    if (!remap) throw new Error("Time Remap missing on nonlinear-remap");
    var remapPoints = [[0, 0.5], [1.25, 0.75], [1.75, 1], [3, 1.25]];
    for (var rp = 0; rp < remapPoints.length; rp++)
        remap.setValueAtTime(remapPoints[rp][0], remapPoints[rp][1]);
    // AE adds default endpoint keys when enabling Time Remap; remove only
    // those not in the authored four-key clock, rather than silently admitting
    // a fifth key from the original four-second footage duration.
    for (var rk = remap.numKeys; rk >= 1; rk--) {
        var authored = false;
        for (var ri = 0; ri < remapPoints.length; ri++)
            if (Math.abs(remap.keyTime(rk) - remapPoints[ri][0]) < 0.000001) authored = true;
        if (!authored) remap.removeKey(rk);
    }
    if (remap.numKeys != 4) throw new Error("nonlinear-remap must have four native keys");
    for (var rt = 1; rt <= 4; rt++)
        remap.setInterpolationTypeAtKey(rt, KeyframeInterpolationType.LINEAR, KeyframeInterpolationType.LINEAR);
    x.layer.inPoint = 1; x.layer.outPoint = 2; // retain interval after AE enables remap
    // Unsupported FX export targets: native oracle contains only retained
    // sibling; it cannot prove the fidelity of an intentionally omitted target.
    base(project, sound, "conflicting-gain-clock").layer.name = "retained-sibling";
    base(project, sound, "invalid-gain-hull").layer.name = "retained-sibling";
    x = base(project, sound, "empty-only-group"); x.layer.name = "retained-sibling";
    var empty = audioComp(project, "empty-only-group__empty-child", 2);
    var emptyOccurrence = x.comp.layers.add(empty); emptyOccurrence.name = "empty-no-audio";
    x = base(project, sound, "stereo-levels");
    var stereo = audioLevels(x.layer);
    stereo.setValueAtTime(1.5, [-3, -12]); stereo.setValueAtTime(2.5, [-12, -3]);
    stereo.setInterpolationTypeAtKey(1, KeyframeInterpolationType.LINEAR, KeyframeInterpolationType.LINEAR);
    stereo.setInterpolationTypeAtKey(2, KeyframeInterpolationType.LINEAR, KeyframeInterpolationType.LINEAR);
    level(audioLayer(x.comp, other, "stereo-retained-sibling", 1, 3, 0.5), AUDIO_DB_HALF);
    x = base(project, sound, "expression");
    var expr = audioLevels(x.layer);
    expr.expression = "value + [Math.sin(time * 2) * 3, Math.sin(time * 2) * 3]";
    if (!expr.expressionEnabled || expr.expressionError) throw new Error("audio expression unavailable: " + expr.expressionError);
    level(audioLayer(x.comp, other, "expression-retained-sibling", 1, 3, 0.5), AUDIO_DB_HALF);
    var count = 0;
    for (var i = 1; i <= project.numItems; i++) {
        var item = project.item(i);
        if (item instanceof CompItem && item.name.indexOf("__") < 0) count++;
    }
    if (count !== AUDIO_ROOT_NAMES.length) throw new Error("missing root compositions: " + count);
    return {roots: count, media: [sound.id, other.id, movie.id]};
}
function easeList(list) {
    var result = [];
    for (var i = 0; i < list.length; i++) result.push({speed:list[i].speed, influence:list[i].influence});
    return result;
}
function propertyReadback(prop) {
    var keys = [];
    for (var k = 1; k <= prop.numKeys; k++) {
        keys.push({time:prop.keyTime(k), value:prop.keyValue(k),
            inType:String(prop.keyInInterpolationType(k)), outType:String(prop.keyOutInterpolationType(k)),
            inEase:easeList(prop.keyInTemporalEase(k)), outEase:easeList(prop.keyOutTemporalEase(k))});
    }
    return {value:prop.value, keys:keys,
        expression:prop.canSetExpression ? prop.expression : null,
        expressionEnabled:prop.canSetExpression ? prop.expressionEnabled : false,
        expressionError:prop.canSetExpression ? prop.expressionError : null};
}
function readbackProject(project) {
    if (!project.file) throw new Error("native project must be file-backed for readback");
    var result = {source:"Adobe After Effects authored project", projectFile:project.file.fsName,
        fps:24, roots:[], media:[], nested:[]};
    for (var i = 1; i <= project.numItems; i++) {
        var item = project.item(i);
        if (item instanceof FootageItem) {
            result.media.push({id:item.id, name:item.name, path:item.file ? item.file.fsName : null,
                duration:item.duration, hasAudio:item.hasAudio, hasVideo:item.hasVideo});
        }
        if (!(item instanceof CompItem)) continue;
        var row = {id:item.id, name:item.name, duration:item.duration, hasAudio:item.hasAudio, width:item.width,
            height:item.height, frameRate:item.frameRate, displayStart:item.displayStartTime,
            workAreaStart:item.workAreaStart, workAreaDuration:item.workAreaDuration, layers:[]};
        for (var j = 1; j <= item.numLayers; j++) {
            var layer = item.layer(j), source = layer.source;
            var detail = {index:j, name:layer.name, sourceId:source ? source.id : null,
                sourceName:source ? source.name : null, sourceKind:source instanceof CompItem ? "composition" : "footage",
                sourceWidth:source ? source.width : null, sourceHeight:source ? source.height : null,
                sourceDuration:source ? source.duration : null,
                startTime:layer.startTime, inPoint:layer.inPoint, outPoint:layer.outPoint,
                stretch:layer.stretch, enabled:layer.enabled, hasAudio:layer.hasAudio,
                audioEnabled:layer.hasAudio ? layer.audioEnabled : false,
                timeRemapEnabled:layer.timeRemapEnabled,
                audioLevels:layer.hasAudio ? propertyReadback(audioLevels(layer)) : null};
            if (layer.timeRemapEnabled) detail.timeRemap = propertyReadback(layer.property("ADBE Time Remapping"));
            row.layers.push(detail);
        }
        if (item.name.indexOf("__") >= 0) result.nested.push(row); else result.roots.push(row);
    }
    return result;
}
function readbackAep(project) {
    if (!project.file || project.file.name != "audio_cases.aep") throw new Error("save audio_cases.aep before readback");
    var result = readbackProject(project);
    if (result.roots.length != AUDIO_ROOT_NAMES.length) throw new Error("readback root count mismatch");
    for (var r = 0; r < AUDIO_ROOT_NAMES.length; r++) {
        var found = 0;
        for (var z = 0; z < result.roots.length; z++) if (result.roots[z].name == AUDIO_ROOT_NAMES[r]) found++;
        if (found != 1) throw new Error("root absent/duplicate: " + AUDIO_ROOT_NAMES[r]);
    }
    return result;
}
// ES3/ExtendScript-safe serializer used by the reviewed parent wrapper; do not
// depend on optional host JSON global. Escapes control characters and quotes.
function audioFixtureJson(value) {
    if (value === null || value === undefined) return "null";
    if (typeof value == "boolean") return value ? "true" : "false";
    if (typeof value == "number") return isFinite(value) ? String(value) : "null";
    if (typeof value == "string") return '"' + value.replace(/\\/g, "\\\\").replace(/"/g, '\\"')
        .replace(/\n/g, "\\n").replace(/\r/g, "\\r").replace(/\t/g, "\\t")
        .replace(/[\u0000-\u001f]/g, function(c) { var h=c.charCodeAt(0).toString(16); return "\\u"+("0000"+h).slice(-4); }) + '"';
    var i, parts = [];
    if (value instanceof Array) {
        for (i = 0; i < value.length; i++) parts.push(audioFixtureJson(value[i]));
        return "[" + parts.join(",") + "]";
    }
    for (var key in value) if (value.hasOwnProperty(key)) parts.push(audioFixtureJson(key) + ":" + audioFixtureJson(value[key]));
    return "{" + parts.join(",") + "}";
}
