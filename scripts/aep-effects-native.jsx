/* Local, explicitly owned Adobe host only. Configuration is a trusted JSON file
 * named by JERBOA_AEP_EFFECTS_CONFIG in the host environment. Never launches AE,
 * closes an unknown/dirty project, changes source FPS, or overwrites a source.
 * Authoring uses Adobe's native API, not converter-produced project bytes.
 */
(function () {
    function encode(v) {
        if (v === null || v === undefined) return 'null';
        if (typeof v === 'string') return '"' + v.replace(/\\/g, '\\\\').replace(/"/g, '\\"').replace(/\r/g, '\\r').replace(/\n/g, '\\n').replace(/\t/g, '\\t') + '"';
        if (typeof v === 'number' || typeof v === 'boolean') return String(v);
        var entries = [], k;
        if (v instanceof Array) {
            for (k = 0; k < v.length; k++) entries.push(encode(v[k]));
            return '[' + entries.join(',') + ']';
        }
        for (k in v) if (v.hasOwnProperty(k)) entries.push(encode(k) + ':' + encode(v[k]));
        return '{' + entries.join(',') + '}';
    }
    function load(path) {
        var file = new File(path);
        if (!file.open('r')) throw new Error('Cannot read ' + path);
        var text = file.read(); file.close();
        // Only locally authored trusted JSON is accepted by this development tool.
        return eval('(' + text + ')');
    }
    function values(value) { return value instanceof Array ? value : [value]; }
    function propertyValue(value) { return value.length === 1 ? value[0] : value; }
    var configPath = $.getenv('JERBOA_AEP_EFFECTS_CONFIG');
    if (!configPath) throw new Error('Missing explicit owned-host configuration');
    var config = load(configPath), result = {version: app.version, build: app.buildNumber, mode: config.mode, cases: []};
    if (!config.receipt || !/\.json$/.test(config.receipt) || new File(config.receipt).exists) throw new Error('Receipt must be a fresh JSON output, never a source');
    if (!(config.cases instanceof Array) || config.cases.length === 0) throw new Error('Explicit nonempty case selection required');
    for (var selected = 0; selected < config.cases.length; selected++) {
        if (!/^[A-Za-z0-9_-]+$/.test(config.cases[selected])) throw new Error('Unsafe case name');
    }
    function emit() {
        var file = new File(config.receipt);
        if (!file.open('w')) throw new Error('Cannot write receipt');
        file.write(encode(result)); file.close();
    }
    function assertInitialProject() {
        var p = app.project;
        if (!p || p.dirty) throw new Error('Refusing missing/dirty project');
        if (config.expected_project) {
            if (!p.file || p.file.fsName !== new File(config.expected_project).fsName) throw new Error('Unexpected owned project');
        } else if (p.file || p.numItems !== 0) throw new Error('Expected clean blank owned host');
    }
    function capture(comp, effect, oracle, id) {
        var record = {name: id, compositionId: comp.id, width: comp.width, height: comp.height,
            duration: comp.duration, frameRate: comp.frameRate, pixelAspect: comp.pixelAspect,
            layers: comp.numLayers, effect: effect.matchName, enabled: effect.enabled, controls: []};
        for (var i = 0; i < oracle.controls.length; i++) {
            var expected = oracle.controls[i], prop = effect.property(expected.name);
            if (!prop) throw new Error('Missing control ' + expected.name);
            var actual = {name: prop.matchName, value: values(prop.value), keys: [], samples: []};
            for (var key = 1; key <= prop.numKeys; key++) {
                // Stringify immediately. Comparing Enumerator objects in a helper
                // previously mislabeled Linear/Hold as Bezier on AE26.5.
                actual.keys.push({time: prop.keyTime(key), value: values(prop.keyValue(key)),
                    inCode: String(prop.keyInInterpolationType(key)), outCode: String(prop.keyOutInterpolationType(key))});
            }
            if (expected.valueAtTime) for (var s = 0; s < expected.valueAtTime.length; s++) {
                var time = expected.valueAtTime[s][0];
                actual.samples.push({time: time, value: values(prop.valueAtTime(time, false))});
            }
            record.controls.push(actual);
        }
        result.cases.push(record); emit();
    }
    assertInitialProject();
    app.beginSuppressDialogs();
    try {
        if (config.mode === 'author') {
            var destination = new File(config.source_output);
            if (destination.exists) throw new Error('Refuse native source overwrite');
            if (config.expected_project) throw new Error('Authoring requires a blank project');
            var project = app.project;
            for (var c = 0; c < config.cases.length; c++) {
                var id = config.cases[c], input = load(config.inputs + '/' + id + '.fx.json'), oracle = load(config.inputs + '/' + id + '.expected.json');
                var owner = input.composition.layers[0], comp = project.items.addComp(id, input.dimensions.width, input.dimensions.height, 1, input.duration, 24);
                var layer = comp.layers.addShape(), vectors = layer.property('ADBE Root Vectors Group');
                var rect = vectors.addProperty('ADBE Vector Shape - Rect');
                rect.property('ADBE Vector Rect Size').setValue(owner.rect.size);
                rect.property('ADBE Vector Rect Position').setValue([owner.rect.size[0] / 2, owner.rect.size[1] / 2]);
                rect.property('ADBE Vector Rect Roundness').setValue(owner.rect.roundness);
                var fill = vectors.addProperty('ADBE Vector Graphic - Fill');
                fill.property('ADBE Vector Fill Color').setValue(owner.rect.fillColor);
                var transform = layer.property('ADBE Transform Group');
                transform.property('ADBE Anchor Point').setValue(owner.transform.anchorPoint);
                transform.property('ADBE Position').setValue(owner.transform.position);
                var effect = layer.property('ADBE Effect Parade').addProperty(oracle.effect);
                effect.enabled = oracle.enabled;
                for (var j = 0; j < oracle.controls.length; j++) {
                    var control = oracle.controls[j], property = effect.property(control.name);
                    if (!property) throw new Error(id + ': missing native control ' + control.name);
                    if (control.keys && !property.canVaryOverTime) {
                        if (!result.unsupported_native_keys) result.unsupported_native_keys = [];
                        result.unsupported_native_keys.push({case_name: id, control: control.name,
                            requested_keys: control.keys, reason: 'Adobe scripting leaf cannot vary over time; source retains first value. This target is not an equivalent animated export oracle.'});
                        property.setValue(propertyValue(control.keys[0].slice(1)));
                        continue;
                    }
                    if (control.keys) {
                        for (var k = 0; k < control.keys.length; k++) property.setValueAtTime(control.keys[k][0], propertyValue(control.keys[k].slice(1)));
                        for (var k = 1; k <= property.numKeys; k++) {
                            var segment = control.segments[Math.min(k - 1, control.segments.length - 1)];
                            var interpolation = segment === 'hold' ? KeyframeInterpolationType.HOLD : KeyframeInterpolationType.LINEAR;
                            property.setInterpolationTypeAtKey(k, interpolation, interpolation);
                            // AE26 reports isSpatial=true for Ramp Color too.
                            // Only actual spatial Point types accept tangents.
                            var valueType = String(property.propertyValueType);
                            if (valueType === '6413' || valueType === '6415') {
                                property.setSpatialAutoBezierAtKey(k, false);
                                property.setSpatialContinuousAtKey(k, false);
                                var zero = property.keyValue(k).length === 3 ? [0, 0, 0] : [0, 0];
                                property.setSpatialTangentsAtKey(k, zero, zero);
                            }
                        }
                    } else property.setValue(propertyValue(control.value));
                }
                var entries = input.composition.dynamics.entries;
                for (var entryIndex = 0; entryIndex < entries.length; entryIndex++) {
                    var entry = entries[entryIndex];
                    if (entry.target.kind === 'effectProperty') continue;
                    if (entry.target.kind !== 'layer' || entry.target.propertyType !== 'rectSize' || entry.target.layerId !== owner.id) throw new Error('Unsupported authoring target');
                    var geometry = vectors.property('ADBE Vector Shape - Rect');
                    var sizeProperty = geometry.property('ADBE Vector Rect Size');
                    var centerProperty = geometry.property('ADBE Vector Rect Position');
                    var sizeKeys = entry.animator.keyframes;
                    for (var sizeIndex = 0; sizeIndex < sizeKeys.length; sizeIndex++) {
                        var sizeKey = sizeKeys[sizeIndex], size = sizeKey.value.value;
                        if (sizeKey.easing.type !== 'linear') throw new Error('Geometry fixture requires Linear keys');
                        sizeProperty.setValueAtTime(sizeKey.layerTime / 1000, size);
                        // FX rect.position is top-left; AE Rect Position is center.
                        centerProperty.setValueAtTime(sizeKey.layerTime / 1000, [owner.rect.position[0] + size[0] / 2, owner.rect.position[1] + size[1] / 2]);
                    }
                    for (var sizeIndex = 1; sizeIndex <= sizeProperty.numKeys; sizeIndex++) {
                        sizeProperty.setInterpolationTypeAtKey(sizeIndex, KeyframeInterpolationType.LINEAR, KeyframeInterpolationType.LINEAR);
                        centerProperty.setInterpolationTypeAtKey(sizeIndex, KeyframeInterpolationType.LINEAR, KeyframeInterpolationType.LINEAR);
                    }
                }
                capture(comp, effect, oracle, id);
            }
            project.save(destination);
            result.source = destination.fsName;
            result.bitsPerChannel = project.bitsPerChannel;
            result.workingSpace = project.workingSpace;
            result.linearBlending = project.linearBlending;
        } else if (config.mode === 'inspect') {
            if (config.expected_project) app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES);
            for (var c = 0; c < config.cases.length; c++) {
                var id = config.cases[c], file = new File(config.inputs + '/' + id + '.aep');
                try {
                    var project = app.open(file), comp = null;
                    for (var i = 1; i <= project.numItems; i++) {
                        var item = project.item(i);
                        if (item instanceof CompItem && item.id === 1) comp = item;
                    }
                    if (!comp || comp.name !== id) throw new Error('Wrong exported composition identity: ' + id);
                    if (comp.numLayers !== 1) throw new Error('Expected one editable owner, got ' + comp.numLayers);
                    var oracle = load(config.inputs + '/' + id + '.expected.json');
                    capture(comp, comp.layer(1).property('ADBE Effect Parade').property(oracle.effect), oracle, id);
                } catch (caseError) {
                    if (!result.failures) result.failures = [];
                    result.failures.push({name: id, error: String(caseError)}); emit();
                }
                var current = app.project;
                if (!current || current.dirty || !current.file || current.file.fsName !== file.fsName) throw new Error('Unsafe project state after inspection');
                current.close(CloseOptions.DO_NOT_SAVE_CHANGES);
            }
        } else throw new Error('Unknown mode');
        result.completed = true;
    } catch (error) {
        result.error = String(error);
    } finally {
        emit(); app.endSuppressDialogs(false);
    }
})();
