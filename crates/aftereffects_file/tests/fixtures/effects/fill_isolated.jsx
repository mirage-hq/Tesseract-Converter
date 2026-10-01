/* Independent Adobe-native fixture body. The Python wrapper injects the only
 * destination and calls runAudioCase(project) in an exclusively owned session.
 * Feature readback and six samples are captured before the single save.
 */

function aepCaseValue(value) {
    if (value instanceof Array) {
        var result = [];
        for (var i = 0; i < value.length; i++) result.push(aepCaseValue(value[i]));
        return result;
    }
    return value;
}

function runAudioCase(project) {
    if (!project || project.file !== null || project.dirty || project.numItems !== 0)
        throw new Error("isolated Fill case requires a newly owned empty unsaved project");
    if (typeof AEP_FIXTURE_DESTINATION !== "string" || !AEP_FIXTURE_DESTINATION.length)
        throw new Error("AEP_FIXTURE_DESTINATION was not injected");

    var destination = new File(AEP_FIXTURE_DESTINATION);
    if (!destination.absoluteURI || destination.exists)
        throw new Error("refusing non-absolute or existing native destination: " + AEP_FIXTURE_DESTINATION);

    var comp = project.items.addComp("FillIsolated", 320, 180, 1, 2, 24);
    comp.displayStartTime = 0;
    comp.workAreaStart = 0;
    comp.workAreaDuration = 2;
    comp.bgColor = [0, 0, 0];

    var sourceBlue = [0.05, 0.15, 0.90];
    var subject = comp.layers.addSolid(sourceBlue, "Opaque Blue Subject 120x80", 120, 80, 1, 2);
    subject.property("ADBE Transform Group").property("ADBE Position").setValue([160, 90]);
    var effect = subject.property("ADBE Effect Parade").addProperty("ADBE Fill");
    if (!effect || effect.matchName !== "ADBE Fill") throw new Error("ordinary ADBE Fill is unavailable");
    var color = effect.property("ADBE Fill-0002");
    var opacity = effect.property("ADBE Fill-0005");
    if (!color || !opacity) throw new Error("required Fill Color or Opacity leaf is missing");
    color.setValue([0.8, 0.2, 0.1, 1]);
    // Fill's native API uses 0..1 although the Adobe UI displays percent.
    opacity.setValue(0.65);
    if (Math.abs(opacity.value - 0.65) > 0.00001)
        throw new Error("Fill Opacity native units did not retain 0.65");

    var controls = [];
    for (var i = 1; i <= effect.numProperties; i++) {
        var control = effect.property(i);
        if (control.propertyType === PropertyType.PROPERTY) {
            var controlValue = null;
            try { controlValue = aepCaseValue(control.value); } catch (_) {}
            controls.push({index: control.propertyIndex, name: control.name,
                           matchName: control.matchName, value: controlValue,
                           canVaryOverTime: control.canVaryOverTime, numKeys: control.numKeys});
        }
    }

    var sampleTimes = [0, 0.25, 0.5, 0.75, 1.25, 47 / 24];
    var samples = [];
    for (var j = 0; j < sampleTimes.length; j++) {
        samples.push({timeSeconds: sampleTimes[j],
                      color: aepCaseValue(color.valueAtTime(sampleTimes[j], false)),
                      opacityNative: opacity.valueAtTime(sampleTimes[j], false)});
    }
    if (samples.length !== 6) throw new Error("expected exactly six Fill samples");

    var result = {
        schema: "fill-isolated-native-authoring/v1",
        "native": {application: "Adobe After Effects", version: app.version, build: app.buildNumber},
        composition: {
            id: comp.id, name: comp.name, width: comp.width, height: comp.height,
            pixelAspect: comp.pixelAspect, durationSeconds: comp.duration,
            frameRate: comp.frameRate, frameDuration: comp.frameDuration,
            sourceFrameCount: Math.round(comp.duration * comp.frameRate),
            displayStartSeconds: comp.displayStartTime,
            workAreaStartSeconds: comp.workAreaStart, workAreaDurationSeconds: comp.workAreaDuration
        },
        subject: {
            id: subject.id, index: subject.index, name: subject.name, enabled: subject.enabled,
            source: {id: subject.source.id, width: subject.source.width, height: subject.source.height,
                     opaqueBlueRgb: sourceBlue},
            effect: {index: effect.propertyIndex, name: effect.name, matchName: effect.matchName,
                     enabled: effect.enabled, controls: controls}
        },
        authored: {colorRgba: [0.8, 0.2, 0.1, 1], opacityNative: 0.65, opacityPercent: 65},
        samples: samples,
        expectedReference: {outputFps: 30, frameCount: 60, aerenderStartFrame: 0, aerenderEndFrame: 47}
    };

    if (comp.width !== 320 || comp.height !== 180 || comp.frameRate !== 24 ||
        Math.abs(comp.duration - 2) > 0.000001 || comp.numLayers !== 1)
        throw new Error("FillIsolated metadata or layer count drifted");
    project.save(destination);
    if (!destination.exists || !project.file || project.file.fsName !== destination.fsName)
        throw new Error("Adobe failed to save the fresh isolated Fill source");
    result.saved = {path: project.file.fsName, bytes: destination.length};
    return result;
}
