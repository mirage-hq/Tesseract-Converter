/* Independent Adobe-native fixture body. The Python wrapper injects the only
 * destination and calls runAudioCase(project) in an exclusively owned session.
 * Readback and all six evaluated samples are captured before the single save.
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
        throw new Error("sampled Position case requires a newly owned empty unsaved project");
    if (typeof AEP_FIXTURE_DESTINATION !== "string" || !AEP_FIXTURE_DESTINATION.length)
        throw new Error("AEP_FIXTURE_DESTINATION was not injected");

    var destination = new File(AEP_FIXTURE_DESTINATION);
    if (!destination.absoluteURI || destination.exists)
        throw new Error("refusing non-absolute or existing native destination: " + AEP_FIXTURE_DESTINATION);

    var comp = project.items.addComp("ExpressionPosition", 320, 180, 1, 2, 24);
    comp.displayStartTime = 0;
    comp.workAreaStart = 0;
    comp.workAreaDuration = 2;
    comp.bgColor = [0, 0, 0];

    var controller = comp.layers.addNull(2);
    controller.name = "Controller";
    var sliderEffect = controller.property("ADBE Effect Parade").addProperty("ADBE Slider Control");
    if (!sliderEffect || sliderEffect.matchName !== "ADBE Slider Control")
        throw new Error("Adobe Slider Control is unavailable");
    var slider = sliderEffect.property("ADBE Slider Control-0001");
    if (!slider) throw new Error("native Slider control leaf is missing");
    slider.setValue(35);
    controller.enabled = false;

    var subject = comp.layers.addSolid([0.95, 0.35, 0.08], "Expression Subject 24x24", 24, 24, 1, 2);
    var position = subject.property("ADBE Transform Group").property("ADBE Position");
    position.setValue([23, 41]);
    var storedBase = aepCaseValue(position.value);
    var expressionSource = 'var a=thisComp.layer("Controller").effect("Amplitude")("Slider"); [80+a*Math.sin(2*Math.PI*time)+15*time,80+12*time*time]';
    sliderEffect.name = "Amplitude";
    position.expression = expressionSource;
    position.expressionEnabled = true;
    if (!position.expressionEnabled || position.expression !== expressionSource)
        throw new Error("native Position expression did not persist in the live project");

    var sampleTimes = [0, 0.25, 0.5, 0.75, 1.25, 47 / 24];
    var samples = [];
    for (var i = 0; i < sampleTimes.length; i++) {
        samples.push({
            timeSeconds: sampleTimes[i],
            evaluated: aepCaseValue(position.valueAtTime(sampleTimes[i], false)),
            preExpression: aepCaseValue(position.valueAtTime(sampleTimes[i], true))
        });
    }
    if (samples.length !== 6) throw new Error("expected exactly six evaluated samples");

    var result = {
        schema: "sampled-position-expression-native-authoring/v1",
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
            id: subject.id, index: subject.index, name: subject.name,
            source: {id: subject.source.id, width: subject.source.width, height: subject.source.height},
            position: {matchName: position.matchName, storedBase: storedBase,
                       expression: position.expression, expressionEnabled: position.expressionEnabled}
        },
        controller: {
            id: controller.id, index: controller.index, name: controller.name,
            enabled: controller.enabled, nullLayer: controller.nullLayer,
            effect: {index: sliderEffect.propertyIndex, name: sliderEffect.name, matchName: sliderEffect.matchName,
                     control: {index: slider.propertyIndex, name: slider.name, matchName: slider.matchName,
                               value: slider.value}}
        },
        samples: samples,
        expectedReference: {outputFps: 30, frameCount: 60, aerenderStartFrame: 0, aerenderEndFrame: 47}
    };

    if (comp.width !== 320 || comp.height !== 180 || comp.frameRate !== 24 ||
        Math.abs(comp.duration - 2) > 0.000001 || comp.numLayers !== 2)
        throw new Error("ExpressionPosition metadata or layer count drifted");
    project.save(destination);
    if (!destination.exists || !project.file || project.file.fsName !== destination.fsName)
        throw new Error("Adobe failed to save the fresh sampled Position source");
    result.saved = {path: project.file.fsName, bytes: destination.length};
    return result;
}
