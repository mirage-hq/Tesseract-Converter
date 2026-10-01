/* One explicitly selected, independently Adobe-authored vector proof.
 * Caller supplies trusted AEP_VECTOR_CONFIG {mode, source, receipt}.
 * Never closes unknown/dirty work, overwrites a source, suppresses dialogs,
 * or changes native FPS to produce the 30fps reference.
 */
(function () {
    var config = AEP_VECTOR_CONFIG;
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
    if (!config || (config.mode !== 'author' && config.mode !== 'inspect')) throw new Error('Explicit author/inspect mode required');
    var receipt = new File(config.receipt), source = new File(config.source);
    if (receipt.exists) throw new Error('Refusing receipt overwrite');
    var result = {"case": 'vector-rect-size', mode: config.mode, adobeVersion: app.version,
        adobeBuild: app.buildNumber, completed: false, source: source.fsName};
    var owned = null;
    function values(v) { return v instanceof Array ? v : [v]; }
    function captureProperty(property) {
        var out = {name: property.matchName, value: values(property.value), keys: [], samples: []};
        for (var k = 1; k <= property.numKeys; k++) {
            out.keys.push({time: property.keyTime(k), value: values(property.keyValue(k)),
                inCode: String(property.keyInInterpolationType(k)), outCode: String(property.keyOutInterpolationType(k))});
        }
        var times = [0, 0.5, 29 / 30, 1, 31 / 30, 59 / 30];
        for (var s = 0; s < times.length; s++) out.samples.push({time: times[s], value: values(property.valueAtTime(times[s], false))});
        return out;
    }
    function linearKeys(property, first, last) {
        property.setValueAtTime(0, first);
        property.setValueAtTime(1, last);
        for (var k = 1; k <= 2; k++) property.setInterpolationTypeAtKey(k, KeyframeInterpolationType.LINEAR, KeyframeInterpolationType.LINEAR);
    }
    function capture(comp) {
        if (comp.numLayers !== 1) throw new Error('Expected one editable Shape layer');
        var layer = comp.layer(1);
        if (!(layer instanceof ShapeLayer)) throw new Error('Not an editable Shape layer');
        var vectors = layer.property('ADBE Root Vectors Group');
        function locate(group, name) {
            for (var i = 1; i <= group.numProperties; i++) {
                var p = group.property(i);
                if (p.matchName === name) return p;
                if (p.propertyType !== PropertyType.PROPERTY) {
                    var nested = locate(p, name);
                    if (nested) return nested;
                }
            }
            return null;
        }
        var names = ['ADBE Vector Rect Size', 'ADBE Vector Rect Position',
            'ADBE Vector Rect Roundness', 'ADBE Vector Fill Color'];
        var controls = [];
        for (var i = 0; i < names.length; i++) {
            var p = locate(vectors, names[i]);
            if (!p) throw new Error('Missing editable control ' + names[i]);
            controls.push(captureProperty(p));
        }
        result.composition = {id: comp.id, name: comp.name, width: comp.width, height: comp.height,
            duration: comp.duration, frameRate: comp.frameRate, pixelAspect: comp.pixelAspect,
            displayStartTime: comp.displayStartTime, workAreaStart: comp.workAreaStart,
            workAreaDuration: comp.workAreaDuration, layers: comp.numLayers};
        result.layer = {name: layer.name, inPoint: layer.inPoint, outPoint: layer.outPoint,
            startTime: layer.startTime, stretch: layer.stretch,
            anchor: layer.transform.anchorPoint.value, position: layer.transform.position.value,
            scale: layer.transform.scale.value, opacity: layer.transform.opacity.value};
        result.controls = controls;
        result.project = {bitsPerChannel: owned.bitsPerChannel, workingSpace: owned.workingSpace,
            linearBlending: owned.linearBlending};
    }
    try {
        var prior = app.project;
        if (prior && (prior.file !== null || prior.dirty || prior.numItems !== 0)) throw new Error('Refusing existing file-backed, dirty or nonempty project');
        if (config.mode === 'author') {
            if (source.exists) throw new Error('Refusing native source overwrite');
            owned = app.newProject();
            if (!owned || app.project !== owned) throw new Error('Could not establish project ownership');
            owned.bitsPerChannel = 8;
            var comp = owned.items.addComp('vector-rect-size', 320, 180, 1, 2, 24);
            comp.bgColor = [0, 0, 0];
            comp.motionBlur = false;
            var shape = comp.layers.addShape();
            shape.name = 'Edited Rectangle';
            var vectors = shape.property('ADBE Root Vectors Group');
            vectors.addProperty('ADBE Vector Shape - Rect');
            vectors.addProperty('ADBE Vector Graphic - Fill');
            var rect = vectors.property('ADBE Vector Shape - Rect');
            linearKeys(rect.property('ADBE Vector Rect Size'), [80, 40], [150, 70]);
            linearKeys(rect.property('ADBE Vector Rect Position'), [47, 17], [82, 32]);
            rect.property('ADBE Vector Rect Roundness').setValue(2);
            vectors.property('ADBE Vector Graphic - Fill').property('ADBE Vector Fill Color').setValue([0.2, 0.4, 0.6, 1]);
            shape.transform.anchorPoint.setValue([0, 0]);
            shape.transform.position.setValue([100, 70]);
            shape.transform.scale.setValue([100, 100]);
            shape.transform.opacity.setValue(100);
            owned.save(source);
            capture(comp);
        } else {
            if (!source.exists) throw new Error('Native project missing');
            owned = app.open(source);
            if (!owned || app.project !== owned) throw new Error('Could not establish opened-project ownership');
            var selected = null;
            for (var i = 1; i <= owned.numItems; i++) {
                var item = owned.item(i);
                if (item instanceof CompItem && item.name === 'vector-rect-size') {
                    if (selected) throw new Error('Ambiguous composition');
                    selected = item;
                }
            }
            if (!selected) throw new Error('Missing vector-rect-size composition');
            capture(selected);
        }
        if (app.project !== owned) throw new Error('Project ownership changed');
        owned.close(CloseOptions.DO_NOT_SAVE_CHANGES);
        owned = null;
        result.completed = true;
        result.ownedProjectClosed = true;
    } catch (error) {
        result.error = String(error);
        // Do not discard or close a failed project; retain it for diagnosis.
    }
    receipt.encoding = 'UTF-8';
    if (!receipt.open('w')) throw new Error('Cannot write fresh receipt');
    receipt.write(encode(result));
    receipt.close();
})();
