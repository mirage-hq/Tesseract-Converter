/* Independent Adobe-native authoring recipe for the first three export-repair cases.
 *
 * SCRIPT ONLY: this file does not launch Adobe, render, publish, register cases,
 * close projects, or save files. A reviewed parent must provide a newly owned,
 * empty, unsaved project, call buildAep(project), then call readbackAep(project)
 * before saving to a new destination. Source compositions remain 24fps; later
 * independent references must be rendered by Adobe at 30fps.
 */

var EXPORT_REPAIR_CASE_NAMES = [
    "export-repair-compound-static-paths",
    "export-repair-scale-hold-cubic",
    "export-repair-independent-axis-union"
];

function exportRepairFail(message) {
    throw new Error("export repair fixture: " + message);
}

function exportRepairComp(project, name) {
    var comp = project.items.addComp(name, 320, 180, 1, 2, 24);
    comp.displayStartTime = 0;
    comp.workAreaStart = 0;
    comp.workAreaDuration = 2;
    return comp;
}

function exportRepairSetLinear(prop) {
    for (var i = 1; i <= prop.numKeys; i++) {
        prop.setInterpolationTypeAtKey(
            i,
            KeyframeInterpolationType.LINEAR,
            KeyframeInterpolationType.LINEAR
        );
    }
}

function exportRepairSetHold(prop) {
    for (var i = 1; i <= prop.numKeys; i++) {
        prop.setInterpolationTypeAtKey(
            i,
            KeyframeInterpolationType.HOLD,
            KeyframeInterpolationType.HOLD
        );
    }
}

function exportRepairShape(vertices, closed, inTangents, outTangents) {
    var shape = new Shape();
    var i;
    shape.vertices = vertices;
    shape.closed = closed;
    var incoming = inTangents || [];
    var outgoing = outTangents || [];
    if (!inTangents) {
        for (i = 0; i < vertices.length; i++) incoming.push([0, 0]);
    }
    if (!outTangents) {
        for (i = 0; i < vertices.length; i++) outgoing.push([0, 0]);
    }
    shape.inTangents = incoming;
    shape.outTangents = outgoing;
    return shape;
}

function exportRepairVectorGroup(root, name) {
    var group = root.addProperty("ADBE Vector Group");
    if (!group || group.matchName != "ADBE Vector Group") {
        exportRepairFail("cannot add native vector group " + name);
    }
    group.name = name;
    return group.property("ADBE Vectors Group");
}

function exportRepairPath(vectors, name, shape) {
    var path = vectors.addProperty("ADBE Vector Shape - Group");
    if (!path || path.matchName != "ADBE Vector Shape - Group") {
        exportRepairFail("cannot add native path " + name);
    }
    path.name = name;
    path.property("ADBE Vector Shape").setValue(shape);
    return path;
}

function exportRepairFill(vectors, name, color, rule) {
    var fill = vectors.addProperty("ADBE Vector Graphic - Fill");
    if (!fill || fill.matchName != "ADBE Vector Graphic - Fill") {
        exportRepairFail("cannot add native fill " + name);
    }
    fill.name = name;
    fill.property("ADBE Vector Fill Color").setValue(color);
    fill.property("ADBE Vector Fill Opacity").setValue(100);
    fill.property("ADBE Vector Fill Rule").setValue(rule);
    return fill;
}

function exportRepairStroke(vectors, name, color, width) {
    var stroke = vectors.addProperty("ADBE Vector Graphic - Stroke");
    if (!stroke || stroke.matchName != "ADBE Vector Graphic - Stroke") {
        exportRepairFail("cannot add native stroke " + name);
    }
    stroke.name = name;
    stroke.property("ADBE Vector Stroke Color").setValue(color);
    stroke.property("ADBE Vector Stroke Opacity").setValue(100);
    stroke.property("ADBE Vector Stroke Width").setValue(width);
    stroke.property("ADBE Vector Stroke Line Cap").setValue(2);
    stroke.property("ADBE Vector Stroke Line Join").setValue(2);
    return stroke;
}

function exportRepairBackground(comp, name, color) {
    var layer = comp.layers.addSolid(color, name, 320, 180, 1, 2);
    layer.property("ADBE Transform Group").property("ADBE Position").setValue([160, 90]);
    return layer;
}

function exportRepairCompoundCase(project) {
    var comp = exportRepairComp(project, EXPORT_REPAIR_CASE_NAMES[0]);
    exportRepairBackground(comp, "compound-background", [0.025, 0.035, 0.07]);

    var layer = comp.layers.addShape();
    layer.name = "compound-static-native-shapes";
    layer.property("ADBE Transform Group").property("ADBE Position").setValue([160, 90]);
    layer.property("ADBE Transform Group").property("ADBE Scale").setValue([50, 50]);
    var root = layer.property("ADBE Root Vectors Group");

    // Non-zero winding hole: the inner contour deliberately runs opposite to
    // the outer contour. Both native Path properties share one Fill operator.
    var winding = exportRepairVectorGroup(root, "winding-hole-shared-fill");
    exportRepairPath(winding, "winding-outer-clockwise", exportRepairShape([
        [-292, -138], [-42, -126], [-58, 18], [-278, 34]
    ], true));
    exportRepairPath(winding, "winding-inner-counterclockwise", exportRepairShape([
        [-222, -82], [-214, -12], [-112, -20], [-104, -88]
    ], true));
    exportRepairFill(winding, "winding-orange-fill", [0.96, 0.24, 0.06], 1);

    // Even-odd hole: inner and outer contours use the same direction, so the
    // hole depends on native Even-Odd rather than opposite winding.
    var evenOdd = exportRepairVectorGroup(root, "evenodd-hole-shared-fill");
    exportRepairPath(evenOdd, "evenodd-outer-clockwise", exportRepairShape([
        [38, -132], [294, -142], [278, 30], [54, 16]
    ], true));
    exportRepairPath(evenOdd, "evenodd-inner-clockwise", exportRepairShape([
        [105, -82], [218, -88], [211, -18], [112, -12]
    ], true));
    exportRepairFill(evenOdd, "evenodd-cyan-fill", [0.04, 0.76, 0.92], 2);

    // Two open contours share one contrasting Stroke and one static Trim Paths
    // operator. This discriminates contour splitting from paint/modifier scope.
    var open = exportRepairVectorGroup(root, "open-contours-shared-stroke-trim");
    exportRepairPath(open, "open-angular-contour", exportRepairShape([
        [-284, 112], [-196, 62], [-105, 126], [-18, 78]
    ], false));
    exportRepairPath(open, "open-curved-contour", exportRepairShape([
        [38, 112], [142, 64], [282, 122]
    ], false, [[0, 0], [-42, 20], [-48, -10]], [[38, -24], [54, -26], [0, 0]]));
    exportRepairStroke(open, "contrasting-lime-open-stroke", [0.72, 1.0, 0.08], 9);
    var trim = open.addProperty("ADBE Vector Filter - Trim");
    if (!trim || trim.matchName != "ADBE Vector Filter - Trim") {
        exportRepairFail("cannot add native Trim Paths");
    }
    trim.name = "static-trim-across-open-contours";
    trim.property("ADBE Vector Trim Start").setValue(13);
    trim.property("ADBE Vector Trim End").setValue(83);
    trim.property("ADBE Vector Trim Offset").setValue(21);
    trim.property("ADBE Vector Trim Type").setValue(1);

    return comp;
}

function exportRepairScaleCase(project) {
    var comp = exportRepairComp(project, EXPORT_REPAIR_CASE_NAMES[1]);
    exportRepairBackground(comp, "scale-background", [0.035, 0.025, 0.055]);

    var hold = comp.layers.addSolid([0.96, 0.16, 0.05], "asymmetric-solid-hold-scale", 113, 47, 1, 2);
    var holdTransform = hold.property("ADBE Transform Group");
    holdTransform.property("ADBE Position").setValue([77, 89]);
    holdTransform.property("ADBE Rotate Z").setValue(13);
    var holdScale = holdTransform.property("ADBE Scale");
    holdScale.setValueAtTime(0.25, [38, 142]);
    holdScale.setValueAtTime(1.10, [151, 64]);
    holdScale.setValueAtTime(1.65, [82, 123]);
    exportRepairSetHold(holdScale);

    var cubic = comp.layers.addShape();
    cubic.name = "asymmetric-vector-cubic-scale";
    var cubicTransform = cubic.property("ADBE Transform Group");
    cubicTransform.property("ADBE Position").setValue([232, 91]);
    cubicTransform.property("ADBE Rotate Z").setValue(-17);
    var vectors = cubic.property("ADBE Root Vectors Group");
    var body = exportRepairVectorGroup(vectors, "asymmetric-five-point-body");
    exportRepairPath(body, "asymmetric-five-point-path", exportRepairShape([
        [-61, -38], [47, -55], [72, 9], [13, 61], [-53, 31]
    ], true, [[0, 0], [-12, 5], [0, 0], [14, -8], [0, 0]], [[18, -6], [0, 0], [-10, 16], [0, 0], [9, -12]]));
    exportRepairFill(body, "cubic-violet-fill", [0.58, 0.16, 0.94], 1);
    exportRepairStroke(body, "cubic-white-stroke", [0.95, 0.96, 1.0], 5);

    var cubicScale = cubicTransform.property("ADBE Scale");
    cubicScale.setValueAtTime(0.20, [53, 128]);
    cubicScale.setValueAtTime(0.95, [142, 57]);
    cubicScale.setValueAtTime(1.70, [76, 151]);
    var ease = [
        {inSpeed:[18, 31], inInfluence:[29, 61], outSpeed:[84, 47], outInfluence:[67, 38]},
        {inSpeed:[73, 22], inInfluence:[54, 33], outSpeed:[36, 91], outInfluence:[26, 72]},
        {inSpeed:[41, 78], inInfluence:[64, 45], outSpeed:[15, 24], outInfluence:[31, 58]}
    ];
    for (var i = 1; i <= cubicScale.numKeys; i++) {
        cubicScale.setInterpolationTypeAtKey(
            i,
            KeyframeInterpolationType.BEZIER,
            KeyframeInterpolationType.BEZIER
        );
        cubicScale.setTemporalEaseAtKey(i, [
            new KeyframeEase(ease[i - 1].inSpeed[0], ease[i - 1].inInfluence[0]),
            new KeyframeEase(ease[i - 1].inSpeed[1], ease[i - 1].inInfluence[1]),
            new KeyframeEase(0, 33.333333)
        ], [
            new KeyframeEase(ease[i - 1].outSpeed[0], ease[i - 1].outInfluence[0]),
            new KeyframeEase(ease[i - 1].outSpeed[1], ease[i - 1].outInfluence[1]),
            new KeyframeEase(0, 33.333333)
        ]);
    }

    return comp;
}

function exportRepairAxisUnionCase(project) {
    var comp = exportRepairComp(project, EXPORT_REPAIR_CASE_NAMES[2]);
    exportRepairBackground(comp, "axis-union-background", [0.02, 0.055, 0.045]);

    var layer = comp.layers.addShape();
    layer.name = "independent-position-and-exact-scale-union";
    var root = layer.property("ADBE Root Vectors Group");
    var body = exportRepairVectorGroup(root, "axis-union-asymmetric-body");
    exportRepairPath(body, "axis-union-body-path", exportRepairShape([
        [-56, -42], [68, -31], [51, 54], [-34, 67], [-72, 9]
    ], true));
    exportRepairFill(body, "axis-union-gold-fill", [1.0, 0.64, 0.04], 1);
    exportRepairStroke(body, "axis-union-blue-stroke", [0.08, 0.34, 1.0], 7);

    var transform = layer.property("ADBE Transform Group");
    transform.property("ADBE Rotate Z").setValue(11);
    var position = transform.property("ADBE Position");
    if (!position.isSeparationLeader) exportRepairFail("native Position is not a separation leader");
    position.dimensionsSeparated = true;
    if (!position.dimensionsSeparated) exportRepairFail("cannot separate native Position dimensions");
    var xPosition = transform.property("ADBE Position_0");
    var yPosition = transform.property("ADBE Position_1");
    if (!xPosition || !yPosition) exportRepairFail("native Position followers unavailable");
    xPosition.setValueAtTime(0.20, 52);
    xPosition.setValueAtTime(1.10, 236);
    yPosition.setValueAtTime(0.55, 46);
    yPosition.setValueAtTime(1.55, 139);
    exportRepairSetLinear(xPosition);
    exportRepairSetLinear(yPosition);

    // AE Scale does not expose Position-style separated followers. These four
    // native keys are the exact time union of independently linear X and Y
    // curves: X 63@.20 -> 151@1.10; Y 139@.55 -> 58@1.55.
    var scale = transform.property("ADBE Scale");
    scale.setValueAtTime(0.20, [63, 139]);
    scale.setValueAtTime(0.55, [97.22222222222223, 139]);
    scale.setValueAtTime(1.10, [151, 94.45]);
    scale.setValueAtTime(1.55, [151, 58]);
    exportRepairSetLinear(scale);

    return comp;
}

function buildAep(project) {
    if (!project || project.file !== null || project.dirty || project.numItems !== 0) {
        exportRepairFail("requires a newly owned empty unsaved project");
    }
    var compound = exportRepairCompoundCase(project);
    var scale = exportRepairScaleCase(project);
    var axis = exportRepairAxisUnionCase(project);
    if (compound.id == scale.id || compound.id == axis.id || scale.id == axis.id) {
        exportRepairFail("target composition IDs are not distinct");
    }
    return {compoundId:compound.id, scaleId:scale.id, axisUnionId:axis.id};
}

function exportRepairValue(value) {
    if (value === null || value === undefined) return null;
    if (value instanceof Array) {
        var array = [];
        for (var i = 0; i < value.length; i++) array.push(exportRepairValue(value[i]));
        return array;
    }
    if (typeof value == "number" || typeof value == "string" || typeof value == "boolean") return value;
    if (value.vertices !== undefined) {
        return {
            vertices:exportRepairValue(value.vertices),
            inTangents:exportRepairValue(value.inTangents),
            outTangents:exportRepairValue(value.outTangents),
            closed:value.closed
        };
    }
    return String(value);
}

function exportRepairEase(list) {
    var result = [];
    for (var i = 0; i < list.length; i++) {
        result.push({speed:list[i].speed, influence:list[i].influence});
    }
    return result;
}

function exportRepairProperty(prop) {
    var row = {
        name:prop.name,
        matchName:prop.matchName,
        propertyIndex:prop.propertyIndex,
        propertyType:String(prop.propertyType)
    };
    if (prop.propertyType == PropertyType.PROPERTY) {
        try { row.value = exportRepairValue(prop.value); } catch (_) { row.value = null; }
        row.keys = [];
        for (var k = 1; k <= prop.numKeys; k++) {
            var key = {
                time:prop.keyTime(k),
                value:exportRepairValue(prop.keyValue(k)),
                inInterpolation:String(prop.keyInInterpolationType(k)),
                outInterpolation:String(prop.keyOutInterpolationType(k))
            };
            try { key.inTemporalEase = exportRepairEase(prop.keyInTemporalEase(k)); } catch (_) { key.inTemporalEase = null; }
            try { key.outTemporalEase = exportRepairEase(prop.keyOutTemporalEase(k)); } catch (_) { key.outTemporalEase = null; }
            try { key.inSpatialTangent = exportRepairValue(prop.keyInSpatialTangent(k)); } catch (_) { key.inSpatialTangent = null; }
            try { key.outSpatialTangent = exportRepairValue(prop.keyOutSpatialTangent(k)); } catch (_) { key.outSpatialTangent = null; }
            row.keys.push(key);
        }
        try { row.isSeparationLeader = prop.isSeparationLeader; } catch (_) {}
        try { row.isSeparationFollower = prop.isSeparationFollower; } catch (_) {}
        try { row.dimensionsSeparated = prop.dimensionsSeparated; } catch (_) {}
    } else {
        row.properties = [];
        for (var i = 1; i <= prop.numProperties; i++) {
            row.properties.push(exportRepairProperty(prop.property(i)));
        }
    }
    return row;
}

function exportRepairLayer(layer) {
    var source = layer.source;
    var sourceRow = null;
    if (source) {
        sourceRow = {
            id:source.id,
            name:source.name,
            kind:source instanceof CompItem ? "composition" : "footage",
            width:source.width,
            height:source.height,
            duration:source.duration
        };
        try { sourceRow.solidColor = exportRepairValue(source.mainSource.color); } catch (_) {}
    }
    var row = {
        id:layer.id,
        index:layer.index,
        name:layer.name,
        matchName:layer.matchName,
        enabled:layer.enabled,
        startTime:layer.startTime,
        inPoint:layer.inPoint,
        outPoint:layer.outPoint,
        source:sourceRow,
        transform:exportRepairProperty(layer.property("ADBE Transform Group"))
    };
    var vectors = layer.property("ADBE Root Vectors Group");
    row.vectors = vectors ? exportRepairProperty(vectors) : null;
    return row;
}

function exportRepairFindComp(project, name) {
    var found = null;
    for (var i = 1; i <= project.numItems; i++) {
        var item = project.item(i);
        if (item instanceof CompItem && item.name == name) {
            if (found) exportRepairFail("duplicate target composition " + name);
            found = item;
        }
    }
    if (!found) exportRepairFail("missing target composition " + name);
    return found;
}

function exportRepairAssertNear(actual, expected, context) {
    if (Math.abs(actual - expected) > 0.000001) {
        exportRepairFail(context + " expected " + expected + ", got " + actual);
    }
}

function exportRepairAssertKeyTimes(prop, expected, context) {
    if (prop.numKeys != expected.length) exportRepairFail(context + " key count drift");
    for (var i = 0; i < expected.length; i++) {
        // AE stores these 24fps composition keys in 1/24576-second ticks.
        // Compare against that explicit quantization, not a widened tolerance.
        var nativeTime = Math.round(expected[i] * 24576) / 24576;
        exportRepairAssertNear(prop.keyTime(i + 1), nativeTime, context + " key time " + (i + 1));
    }
}

function exportRepairFeatureAssertions(project) {
    var compound = exportRepairFindComp(project, EXPORT_REPAIR_CASE_NAMES[0]);
    var compoundLayer = compound.layer("compound-static-native-shapes");
    var root = compoundLayer.property("ADBE Root Vectors Group");
    if (!root || root.numProperties != 3) exportRepairFail("compound vector-group count drift");
    var winding = root.property("winding-hole-shared-fill").property("ADBE Vectors Group");
    var evenOdd = root.property("evenodd-hole-shared-fill").property("ADBE Vectors Group");
    var open = root.property("open-contours-shared-stroke-trim").property("ADBE Vectors Group");
    if (winding.numProperties != 3 || evenOdd.numProperties != 3 || open.numProperties != 4) {
        exportRepairFail("compound geometry/paint/modifier ownership drift");
    }
    if (winding.property("winding-orange-fill").property("ADBE Vector Fill Rule").value != 1) {
        exportRepairFail("winding fill rule drift");
    }
    if (evenOdd.property("evenodd-cyan-fill").property("ADBE Vector Fill Rule").value != 2) {
        exportRepairFail("even-odd fill rule drift");
    }
    exportRepairAssertNear(open.property("static-trim-across-open-contours").property("ADBE Vector Trim Start").value, 13, "Trim Start");
    exportRepairAssertNear(open.property("static-trim-across-open-contours").property("ADBE Vector Trim End").value, 83, "Trim End");

    var scaleComp = exportRepairFindComp(project, EXPORT_REPAIR_CASE_NAMES[1]);
    var holdScale = scaleComp.layer("asymmetric-solid-hold-scale").property("ADBE Transform Group").property("ADBE Scale");
    var cubicScale = scaleComp.layer("asymmetric-vector-cubic-scale").property("ADBE Transform Group").property("ADBE Scale");
    exportRepairAssertKeyTimes(holdScale, [0.25, 1.10, 1.65], "Hold Scale");
    exportRepairAssertKeyTimes(cubicScale, [0.20, 0.95, 1.70], "Cubic Scale");
    if (holdScale.keyOutInterpolationType(1) != KeyframeInterpolationType.HOLD) exportRepairFail("Hold Scale interpolation drift");
    if (cubicScale.keyOutInterpolationType(1) != KeyframeInterpolationType.BEZIER) exportRepairFail("Cubic Scale interpolation drift");

    var axisComp = exportRepairFindComp(project, EXPORT_REPAIR_CASE_NAMES[2]);
    var transform = axisComp.layer("independent-position-and-exact-scale-union").property("ADBE Transform Group");
    var position = transform.property("ADBE Position");
    if (!position.dimensionsSeparated) exportRepairFail("Position separation drift");
    exportRepairAssertKeyTimes(transform.property("ADBE Position_0"), [0.20, 1.10], "Position X");
    exportRepairAssertKeyTimes(transform.property("ADBE Position_1"), [0.55, 1.55], "Position Y");
    var unionScale = transform.property("ADBE Scale");
    exportRepairAssertKeyTimes(unionScale, [0.20, 0.55, 1.10, 1.55], "Scale exact union");
    var expectedValues = [[63, 139], [97.22222222222223, 139], [151, 94.45], [151, 58]];
    for (var k = 1; k <= unionScale.numKeys; k++) {
        var actual = unionScale.keyValue(k);
        for (var axis = 0; axis < 2; axis++) {
            var expected = expectedValues[k - 1][axis];
            // Native Scale narrows the requested percentages to float32.
            var roundingBound = Math.max(1, Math.abs(expected)) * Math.pow(2, -24);
            if (Math.abs(actual[axis] - expected) > roundingBound) {
                exportRepairFail("Scale union component " + axis + " key " + k + " exceeds float32 rounding");
            }
        }
    }
}

function readbackAep(project) {
    if (!project || app.project !== project) exportRepairFail("owned project identity lost before readback");
    exportRepairFeatureAssertions(project);
    var compositions = [];
    for (var n = 0; n < EXPORT_REPAIR_CASE_NAMES.length; n++) {
        var comp = exportRepairFindComp(project, EXPORT_REPAIR_CASE_NAMES[n]);
        if (comp.width != 320 || comp.height != 180 || comp.frameRate != 24 || Math.abs(comp.duration - 2) > 0.000001) {
            exportRepairFail("composition settings drift for " + comp.name);
        }
        var row = {
            id:comp.id,
            name:comp.name,
            width:comp.width,
            height:comp.height,
            pixelAspect:comp.pixelAspect,
            duration:comp.duration,
            frameRate:comp.frameRate,
            frameDuration:comp.frameDuration,
            sourceFrameCount:Math.round(comp.duration * comp.frameRate),
            displayStart:comp.displayStartTime,
            workAreaStart:comp.workAreaStart,
            workAreaDuration:comp.workAreaDuration,
            layers:[]
        };
        for (var i = 1; i <= comp.numLayers; i++) row.layers.push(exportRepairLayer(comp.layer(i)));
        compositions.push(row);
    }
    return {
        schema:"aftereffects-export-repairs-native-authoring-readback/v1",
        "native":{
            application:"Adobe After Effects",
            version:app.version,
            build:app.buildNumber,
            width:320,
            height:180,
            durationSeconds:2,
            sourceFps:24,
            sourceFrameCount:48,
            referenceRender:{
                outputFps:30,
                expectedFrameCount:60,
                aerenderStartFrame:0,
                aerenderEndFrame:47,
                renderSettings:"Use this frame rate: 30; Quality: Best; Resolution: Full",
                outputModule:"H.264 - Match Render Settings - 40 Mbps"
            }
        },
        authoringIntent:{
            caseCount:3,
            plannedPanelCount:12,
            importedOrCopiedShowreelValues:false,
            compound:"opposite-winding/non-zero and same-winding/even-odd holes; two open contours share Stroke and Trim Paths",
            scaleInterpolation:"asymmetric native solid Hold Scale and asymmetric native vector Cubic Scale with editable temporal eases",
            axisUnion:"separated native Position X/Y times plus exact four-time union of independently linear Scale axes"
        },
        targetCompositionIds:(function () {
            var result = [];
            for (var i = 0; i < compositions.length; i++) result.push({id:compositions[i].id, name:compositions[i].name});
            return result;
        })(),
        compositions:compositions
    };
}
