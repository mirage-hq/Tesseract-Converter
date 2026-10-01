/* Independent native fixture production only. Never imports/exports FX or runs tests.
 * Trusted caller supplies AEP_REFERENCE_ADDITIONS_CONFIG {cases, receipt, failureDir}.
 * Each author_jsx receives a fresh `comp`, `mediaRoot`, and linearKeys().
 * Receipts describe production, NOT editable-assertion or render-comparison results.
 */
(function () {
    var config = AEP_REFERENCE_ADDITIONS_CONFIG;
    function encode(value) {
        if (value === null || value === undefined) return 'null';
        if (typeof value === 'string') return '"' + value.replace(/\\/g, '\\\\').replace(/"/g, '\\"').replace(/\r/g, '\\r').replace(/\n/g, '\\n').replace(/\t/g, '\\t') + '"';
        if (typeof value === 'number' || typeof value === 'boolean') return String(value);
        var parts = [], key;
        if (value instanceof Array) {
            for (key = 0; key < value.length; key++) parts.push(encode(value[key]));
            return '[' + parts.join(',') + ']';
        }
        for (key in value) if (value.hasOwnProperty(key)) parts.push(encode(key) + ':' + encode(value[key]));
        return '{' + parts.join(',') + '}';
    }
    function saveReceipt(file, result) {
        file.encoding = 'UTF-8';
        if (!file.open('w')) throw new Error('Cannot write owned receipt');
        file.write(encode(result));
        file.close();
    }
    function linearKeys(property, times, values) {
        row.last_keyed_property = property.matchName;
        row.last_keyed_interpolation = 'linear';
        for (var i = 0; i < times.length; i++) {
            property.setValueAtTime(times[i], values[i]);
            property.setInterpolationTypeAtKey(i + 1, KeyframeInterpolationType.LINEAR, KeyframeInterpolationType.LINEAR);
        }
    }
    function holdKeys(property, times, values) {
        row.last_keyed_property = property.matchName;
        row.last_keyed_interpolation = 'hold';
        for (var i = 0; i < times.length; i++) {
            property.setValueAtTime(times[i], values[i]);
            property.setInterpolationTypeAtKey(i + 1, KeyframeInterpolationType.HOLD, KeyframeInterpolationType.HOLD);
        }
    }
    function authorContent(comp, mediaRoot, code) {
        // Isolate snippet declarations (e.g. `var source`) from producer ownership state.
        eval(code);
    }
    if (!config || !config.cases || !config.cases.length) throw new Error('Explicit nonempty native authoring selection required');
    var receipt = new File(config.receipt);
    if (receipt.exists) throw new Error('Refusing receipt overwrite');
    var prior = app.project;
    if (prior && (prior.file !== null || prior.dirty || prior.numItems !== 0)) throw new Error('Refusing existing file-backed, dirty or nonempty project');
    var result = {schema_version: 1, purpose: 'independent_native_fixture_production_only',
        adobe_version: app.version, adobe_build: app.buildNumber, cases: [],
        tests_executed: false, comparison_executed: false};
    saveReceipt(receipt, result);
    for (var index = 0; index < config.cases.length; index++) {
        var spec = config.cases[index], owned = null;
        var row = {case_id: spec.case_id, completed: false, source: spec.source,
            test_execution: 'UNRUN', visual_comparison: 'unmeasured'};
        result.cases.push(row);
        try {
            var source = new File(spec.source);
            if (source.exists) throw new Error('Refusing native source overwrite');
            if (!source.parent.exists) throw new Error('Source parent must be prepared by caller');
            prior = app.project;
            if (prior && (prior.file !== null || prior.dirty || prior.numItems !== 0)) throw new Error('Project ownership changed between cases');
            owned = app.newProject();
            if (!owned || app.project !== owned) throw new Error('Could not establish new project ownership');
            owned.bitsPerChannel = 8;
            var comp = owned.items.addComp(spec.composition_name, spec.width, spec.height, 1, spec.duration, spec.fps);
            comp.bgColor = [0, 0, 0];
            var mediaRoot = spec.media_root || source.parent.fsName;
            // This is repository-authored Adobe API code, never generated from converter output.
            authorContent(comp, mediaRoot, spec.author_jsx);
            if (app.project !== owned) throw new Error('Authoring changed project ownership');
            owned.save(source);
            row.composition = {id: comp.id, dynamic_link_guid: comp.dynamicLinkGUID, name: comp.name, width: comp.width, height: comp.height,
                duration: comp.duration, frame_rate: comp.frameRate, pixel_aspect: comp.pixelAspect,
                display_start: comp.displayStartTime, work_area_start: comp.workAreaStart,
                work_area_duration: comp.workAreaDuration, layers: comp.numLayers};
            row.supporting_compositions = [];
            row.media = [];
            for (var itemIndex = 1; itemIndex <= owned.numItems; itemIndex++) {
                var item = owned.item(itemIndex);
                if (item instanceof CompItem && item !== comp) row.supporting_compositions.push({id: item.id,
                    dynamic_link_guid: item.dynamicLinkGUID, name: item.name, width: item.width, height: item.height,
                    duration: item.duration, frame_rate: item.frameRate, pixel_aspect: item.pixelAspect,
                    display_start: item.displayStartTime, consumed_by: spec.case_id});
                if (item instanceof FootageItem && item.file) row.media.push({id: item.id, name: item.name, path: item.file.fsName});
            }
            row.project = {bits_per_channel: owned.bitsPerChannel, working_space: owned.workingSpace,
                linear_blending: owned.linearBlending};
            owned.close(CloseOptions.DO_NOT_SAVE_CHANGES);
            owned = null;
            row.completed = true;
            row.owned_project_closed = true;
        } catch (error) {
            row.error = String(error);
            // Preserve our failed authoring before closing only the project created in this iteration.
            if (owned && app.project === owned) {
                var failed = new File(config.failureDir + '/' + spec.case_id + '-failed.aep');
                if (!failed.exists && failed.parent.exists) {
                    try {
                        owned.save(failed);
                        row.failed_source = failed.fsName;
                        owned.close(CloseOptions.DO_NOT_SAVE_CHANGES);
                        owned = null;
                        row.owned_project_closed = true;
                    } catch (saveError) { row.preservation_error = String(saveError); }
                }
            }
            saveReceipt(receipt, result);
            if (owned || (app.project && (app.project.file !== null || app.project.dirty || app.project.numItems !== 0))) break;
        }
        saveReceipt(receipt, result);
    }
})();
