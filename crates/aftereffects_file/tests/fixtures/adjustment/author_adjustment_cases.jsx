/* Independent Adobe-native Adjustment Layer authoring recipe.
 *
 * The reviewed parent injects the absolute ADJUSTMENT_SOURCE_PATH and invokes
 * runAudioCase(project) through scripts/aep_audio_adobe.py's owned-session
 * wrapper. Despite the wrapper-compatible name, this script authors no audio
 * and performs no render. It refuses an existing destination and saves only
 * after every composition has been authored and read back successfully.
 */

var ADJUSTMENT_CASE_NAMES = [
    "adjustment-scope", "adjustment-stack-order", "adjustment-effect-order",
    "adjustment-keys", "adjustment-disabled", "adjustment-span",
    "adjustment-nested", "adjustment-masks", "adjustment-matte",
    "adjustment-parent"
];

function adjustmentComp(project, name) {
    var comp = project.items.addComp(name, 320, 180, 1, 2, 24);
    comp.displayStartTime = 0;
    comp.workAreaStart = 0;
    comp.workAreaDuration = 2;
    return comp;
}

function asymmetricSolid(comp, name, color, width, height, position, rotation) {
    var layer = comp.layers.addSolid(color, name, width, height, 1, 2);
    layer.property("ADBE Transform Group").property("ADBE Position").setValue(position);
    layer.property("ADBE Transform Group").property("ADBE Rotate Z").setValue(rotation || 0);
    return layer;
}

function adjustmentBase(comp) {
    asymmetricSolid(comp, "lower-cobalt-127x71", [0.08, 0.24, 0.88], 127, 71, [77, 58], 7);
    asymmetricSolid(comp, "lower-amber-83x117", [0.96, 0.38, 0.05], 83, 117, [238, 108], -11);
}

function upperSentinel(comp) {
    return asymmetricSolid(comp, "upper-sentinel-37x23", [1, 0.05, 0.72], 37, 23, [287, 19], 17);
}

function adjustmentLayer(comp, name) {
    // AE's native adjustment layer requires a source plane. Only the independent
    // visual controls use asymmetric solids; this canonical plane never renders
    // as ordinary source content once adjustmentLayer is set.
    var layer = comp.layers.addSolid([1, 1, 1], name, 320, 180, 1, 2);
    layer.adjustmentLayer = true;
    if (!layer.adjustmentLayer) throw new Error("failed to set adjustment flag: " + name);
    return layer;
}

function addBlur(layer, amount) {
    var effect = layer.property("ADBE Effect Parade").addProperty("ADBE Gaussian Blur 2");
    if (!effect || effect.matchName != "ADBE Gaussian Blur 2") throw new Error("Gaussian Blur unavailable");
    effect.property("ADBE Gaussian Blur 2-0001").setValue(amount);
    effect.property("ADBE Gaussian Blur 2-0002").setValue(1);
    effect.property("ADBE Gaussian Blur 2-0003").setValue(1);
    return effect;
}

function addLevels(layer, gamma) {
    var effect = layer.property("ADBE Effect Parade").addProperty("ADBE Pro Levels2");
    if (!effect || effect.matchName != "ADBE Pro Levels2") throw new Error("Individual Levels unavailable");
    effect.property("ADBE Pro Levels2-0004").setValue(0.08);
    effect.property("ADBE Pro Levels2-0005").setValue(0.82);
    effect.property("ADBE Pro Levels2-0006").setValue(gamma);
    effect.property("ADBE Pro Levels2-0007").setValue(0.04);
    effect.property("ADBE Pro Levels2-0008").setValue(0.93);
    return effect;
}

function linearKeys(prop, points) {
    for (var i = 0; i < points.length; i++) prop.setValueAtTime(points[i][0], points[i][1]);
    for (var k = 1; k <= prop.numKeys; k++)
        prop.setInterpolationTypeAtKey(k, KeyframeInterpolationType.LINEAR, KeyframeInterpolationType.LINEAR);
}

function asymmetricMask(layer, name) {
    var mask = layer.property("ADBE Mask Parade").addProperty("ADBE Mask Atom");
    mask.name = name;
    mask.maskMode = MaskMode.ADD;
    mask.inverted = false;
    var shape = new Shape();
    shape.vertices = [[24, 18], [269, 37], [244, 151], [61, 132], [17, 81]];
    shape.inTangents = [[0, 0], [0, 0], [0, 0], [0, 0], [0, 0]];
    shape.outTangents = [[0, 0], [0, 0], [0, 0], [0, 0], [0, 0]];
    shape.closed = true;
    mask.property("ADBE Mask Shape").setValue(shape);
    mask.property("ADBE Mask Opacity").setValue(68);
    mask.property("ADBE Mask Feather").setValue([9, 4]);
    mask.property("ADBE Mask Offset").setValue(6);
    return mask;
}

function finishCase(comp) {
    upperSentinel(comp);
    if (comp.width != 320 || comp.height != 180 || comp.frameRate != 24 || Math.abs(comp.duration - 2) > 0.000001)
        throw new Error("composition settings drift: " + comp.name);
    return comp;
}

function authorAdjustmentCases(project) {
    var comp, adj, effect, provider, support, nested, parent;

    comp = adjustmentComp(project, "adjustment-scope");
    adjustmentBase(comp); adj = adjustmentLayer(comp, "scope-adjustment"); addBlur(adj, 19); finishCase(comp);

    comp = adjustmentComp(project, "adjustment-stack-order");
    adjustmentBase(comp);
    // Bottom Levels/gamma then top Gaussian is deliberately noncommutative.
    adj = adjustmentLayer(comp, "stack-levels-lower"); addLevels(adj, 2.2);
    adj = adjustmentLayer(comp, "stack-blur-upper"); addBlur(adj, 23); finishCase(comp);

    comp = adjustmentComp(project, "adjustment-effect-order");
    adjustmentBase(comp); adj = adjustmentLayer(comp, "effect-order-adjustment");
    // Effect Parade order is Gaussian then nonlinear Levels/gamma.
    addBlur(adj, 21); addLevels(adj, 2.35); finishCase(comp);

    comp = adjustmentComp(project, "adjustment-keys");
    adjustmentBase(comp); adj = adjustmentLayer(comp, "keyed-adjustment-owner-offset");
    adj.startTime = 0.25; adj.inPoint = 0.25; adj.outPoint = 1.75;
    effect = addBlur(adj, 4);
    linearKeys(effect.property("ADBE Gaussian Blur 2-0001"), [[0.5, 4], [1.25, 31]]);
    linearKeys(adj.property("ADBE Transform Group").property("ADBE Opacity"), [[0.75, 28], [1.5, 86]]);
    finishCase(comp);

    comp = adjustmentComp(project, "adjustment-disabled");
    adjustmentBase(comp); adj = adjustmentLayer(comp, "disabled-adjustment-with-effect"); addBlur(adj, 27); adj.enabled = false;
    adjustmentLayer(comp, "enabled-empty-adjustment"); finishCase(comp);

    comp = adjustmentComp(project, "adjustment-span");
    adjustmentBase(comp); adj = adjustmentLayer(comp, "finite-span-adjustment");
    adj.inPoint = 0.5; adj.outPoint = 1.5; addLevels(adj, 2.4); finishCase(comp);

    support = adjustmentComp(project, "adjustment-nested__support");
    adjustmentBase(support); adj = adjustmentLayer(support, "nested-local-adjustment"); addBlur(adj, 25); upperSentinel(support);
    comp = adjustmentComp(project, "adjustment-nested");
    asymmetricSolid(comp, "outer-lower-sibling-91x43", [0.12, 0.72, 0.24], 91, 43, [249, 139], -13);
    nested = comp.layers.add(support); nested.name = "linked-nested-support";
    nested.property("ADBE Transform Group").property("ADBE Position").setValue([142, 92]);
    nested.property("ADBE Transform Group").property("ADBE Scale").setValue([74, 81]);
    finishCase(comp);

    comp = adjustmentComp(project, "adjustment-masks");
    adjustmentBase(comp); adj = adjustmentLayer(comp, "masked-adjustment"); addLevels(adj, 2.1); asymmetricMask(adj, "asymmetric-static-gate"); finishCase(comp);

    comp = adjustmentComp(project, "adjustment-matte");
    adjustmentBase(comp); adj = adjustmentLayer(comp, "matted-adjustment"); addBlur(adj, 29);
    provider = asymmetricSolid(comp, "independent-alpha-matte-provider-119x67", [1, 1, 1], 119, 67, [103, 83], 16);
    if (typeof adj.setTrackMatte != "function") throw new Error("native setTrackMatte API unavailable");
    adj.setTrackMatte(provider, TrackMatteType.ALPHA_INVERTED);
    if (!adj.hasTrackMatte || adj.trackMatteLayer !== provider || !provider.isTrackMatte)
        throw new Error("native independent track matte link failed");
    finishCase(comp);

    comp = adjustmentComp(project, "adjustment-parent");
    adjustmentBase(comp); adj = adjustmentLayer(comp, "parented-masked-adjustment"); addLevels(adj, 2.3); asymmetricMask(adj, "parented-asymmetric-gate");
    parent = comp.layers.addNull(2); parent.name = "gate-parent-null";
    adj.parent = parent;
    parent.property("ADBE Transform Group").property("ADBE Anchor Point").setValue([43, 29]);
    parent.property("ADBE Transform Group").property("ADBE Position").setValue([184, 102]);
    parent.property("ADBE Transform Group").property("ADBE Scale").setValue([73, 119]);
    parent.property("ADBE Transform Group").property("ADBE Rotate Z").setValue(23);
    finishCase(comp);

    return support;
}

function adjustmentValue(value) {
    if (value === null || value === undefined) return null;
    if (value instanceof Array) { var a=[]; for (var i=0;i<value.length;i++) a.push(adjustmentValue(value[i])); return a; }
    if (typeof value == "number" || typeof value == "string" || typeof value == "boolean") return value;
    if (value.vertices !== undefined) return {vertices:adjustmentValue(value.vertices), inTangents:adjustmentValue(value.inTangents), outTangents:adjustmentValue(value.outTangents), closed:value.closed};
    return String(value);
}

function adjustmentProperty(prop) {
    var keys = [];
    for (var k = 1; k <= prop.numKeys; k++) keys.push({
        time:prop.keyTime(k), value:adjustmentValue(prop.keyValue(k)),
        inInterpolation:String(prop.keyInInterpolationType(k)),
        outInterpolation:String(prop.keyOutInterpolationType(k))
    });
    var row = {name:prop.name, matchName:prop.matchName, propertyIndex:prop.propertyIndex, keys:keys};
    try { row.value = adjustmentValue(prop.value); } catch (_) { row.value = null; }
    return row;
}

function adjustmentTransform(layer) {
    var group = layer.property("ADBE Transform Group"), names = [
        "ADBE Anchor Point", "ADBE Position", "ADBE Scale", "ADBE Rotate Z", "ADBE Opacity"
    ], result = [];
    for (var i = 0; i < names.length; i++) { var p=group.property(names[i]); if (p) result.push(adjustmentProperty(p)); }
    return result;
}

function adjustmentEffects(layer) {
    var parade = layer.property("ADBE Effect Parade"), result = [];
    for (var i = 1; i <= parade.numProperties; i++) {
        var effect = parade.property(i), controls = [];
        for (var j = 1; j <= effect.numProperties; j++) {
            var p = effect.property(j);
            if (p.propertyType == PropertyType.PROPERTY) controls.push(adjustmentProperty(p));
        }
        result.push({index:i, name:effect.name, matchName:effect.matchName, enabled:effect.enabled, controls:controls});
    }
    return result;
}

function adjustmentMasks(layer) {
    var parade = layer.property("ADBE Mask Parade"), result = [];
    for (var i = 1; i <= parade.numProperties; i++) {
        var mask=parade.property(i);
        result.push({index:i, name:mask.name, matchName:mask.matchName, mode:String(mask.maskMode), inverted:mask.inverted,
            shape:adjustmentProperty(mask.property("ADBE Mask Shape")),
            opacity:adjustmentProperty(mask.property("ADBE Mask Opacity")),
            feather:adjustmentProperty(mask.property("ADBE Mask Feather")),
            expansion:adjustmentProperty(mask.property("ADBE Mask Offset"))});
    }
    return result;
}

function adjustmentLayerReadback(layer) {
    var source=layer.source, matte=layer.hasTrackMatte ? layer.trackMatteLayer : null;
    return {index:layer.index, id:layer.id, name:layer.name,
        flags:{enabled:layer.enabled, solo:layer.solo, shy:layer.shy, locked:layer.locked,
            guideLayer:layer.guideLayer, adjustmentLayer:layer.adjustmentLayer,
            threeDLayer:layer.threeDLayer, collapseTransformation:layer.collapseTransformation,
            hasTrackMatte:layer.hasTrackMatte, isTrackMatte:layer.isTrackMatte},
        timing:{startTime:layer.startTime, inPoint:layer.inPoint, outPoint:layer.outPoint, stretch:layer.stretch},
        source:source ? {id:source.id, name:source.name, kind:source instanceof CompItem ? "composition" : "footage",
            width:source.width, height:source.height, duration:source.duration} : null,
        parentLayerId:layer.parent ? layer.parent.id : null,
        trackMatte:{layerId:matte ? matte.id : null, type:layer.hasTrackMatte ? String(layer.trackMatteType) : null},
        transform:adjustmentTransform(layer), masks:adjustmentMasks(layer), effects:adjustmentEffects(layer)};
}

function adjustmentReadback(project, support) {
    var compositions=[], roots=[], supportRow=null;
    for (var i=1;i<=project.numItems;i++) {
        var item=project.item(i); if (!(item instanceof CompItem)) continue;
        var row={id:item.id, name:item.name, width:item.width, height:item.height, duration:item.duration,
            frameRate:item.frameRate, frameDuration:item.frameDuration, sourceFrameCount:Math.round(item.duration*item.frameRate),
            displayStart:item.displayStartTime, workAreaStart:item.workAreaStart, workAreaDuration:item.workAreaDuration, layers:[]};
        for (var j=1;j<=item.numLayers;j++) row.layers.push(adjustmentLayerReadback(item.layer(j)));
        compositions.push(row);
        if (item.name.indexOf("__support") >= 0) supportRow=row; else roots.push(row);
    }
    if (roots.length != 10 || !supportRow || supportRow.id != support.id) throw new Error("expected ten roots and one linked support composition");
    for (var n=0;n<ADJUSTMENT_CASE_NAMES.length;n++) {
        var found=0; for (var r=0;r<roots.length;r++) if (roots[r].name==ADJUSTMENT_CASE_NAMES[n]) found++;
        if (found != 1) throw new Error("missing or duplicate target: " + ADJUSTMENT_CASE_NAMES[n]);
    }
    var nestedRoot=null;
    for (var q=0;q<roots.length;q++) if (roots[q].name=="adjustment-nested") nestedRoot=roots[q];
    var linked=false;
    for (var l=0;l<nestedRoot.layers.length;l++) if (nestedRoot.layers[l].source && nestedRoot.layers[l].source.id==support.id) linked=true;
    if (!linked) throw new Error("nested support is not linked from adjustment-nested");
    return {schema:"aftereffects-adjustment-native-authoring-readback/v1",
        "native":{application:"Adobe After Effects", version:app.version, build:app.buildNumber,
            width:320, height:180, durationSeconds:2, sourceFps:24, sourceFrameCount:48,
            referenceRender:{renderSettingsFps:30, expectedFrameCount:60, aerenderStartFrame:0, aerenderEndFrame:47}},
        targetCompositionIds:(function(){var x=[];for(var z=0;z<roots.length;z++)x.push({id:roots[z].id,name:roots[z].name});return x;})(),
        supportRelations:[{supportCompositionId:supportRow.id, supportCompositionName:supportRow.name,
            consumerCompositionId:nestedRoot.id, consumerCompositionName:nestedRoot.name,
            consumerLayerId:(function(){for(var z=0;z<nestedRoot.layers.length;z++)if(nestedRoot.layers[z].source&&nestedRoot.layers[z].source.id==supportRow.id)return nestedRoot.layers[z].id;return null;})()}],
        compositions:compositions};
}

function runAudioCase(project) {
    if (!project || project.numItems !== 0 || project.file !== null)
        throw new Error("requires a newly owned empty unsaved project");
    if (typeof ADJUSTMENT_SOURCE_PATH != "string" || !ADJUSTMENT_SOURCE_PATH.length)
        throw new Error("ADJUSTMENT_SOURCE_PATH must be an injected absolute destination");
    var destination=new File(ADJUSTMENT_SOURCE_PATH);
    if (!destination.absoluteURI || destination.exists) throw new Error("adjustment source destination must be absolute and absent: " + ADJUSTMENT_SOURCE_PATH);
    var support=authorAdjustmentCases(project);
    // Complete independent readback before the first and only source save.
    var result=adjustmentReadback(project, support);
    project.save(destination);
    if (!project.file || project.file.fsName != destination.fsName || !destination.exists)
        throw new Error("native source save failed");
    result.source={path:project.file.fsName, fileName:project.file.name, saved:true};
    return result;
}
