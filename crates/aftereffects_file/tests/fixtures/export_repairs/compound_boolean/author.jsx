// Independently author native Boolean operand scopes; no lifecycle, I/O or expressions.
function authorCompoundBoolean(project) {
    var comp = project.items.addComp('compound-boolean-operand-scope-v1', 320, 180, 1, 2, 24);
    var cases = [];
    function rectPath(contents, name, box, reverse) {
        var points = [[box[0], box[1]], [box[2], box[1]], [box[2], box[3]], [box[0], box[3]]];
        if (reverse) points.reverse();
        var shape = new Shape();
        shape.vertices = points;
        shape.inTangents = [[0,0],[0,0],[0,0],[0,0]];
        shape.outTangents = [[0,0],[0,0],[0,0],[0,0]];
        shape.closed = true;
        var path = contents.addProperty('ADBE Vector Shape - Group');
        path.name = name;
        path.property('ADBE Vector Shape').setValue(shape);
    }
    function merge(contents, name, mode) {
        var operation = contents.addProperty('ADBE Vector Filter - Merge');
        operation.name = name;
        operation.property('ADBE Vector Merge Type').setValue(mode);
    }
    function compound(contents, overlap) {
        var group = contents.addProperty('ADBE Vector Group');
        group.name = 'One compound operand: two outer contours and a reverse hole';
        var nested = group.property('ADBE Vectors Group');
        rectPath(nested, 'Left outer', [-34,-30,overlap ? 8 : -9,30], false);
        rectPath(nested, 'Right outer', [overlap ? -8 : 9,-30,34,30], false);
        rectPath(nested, 'Reverse hole', [-28,-20,-18,-7], true);
        merge(nested, 'Merge contours without a Boolean Add operation', 1);
    }
    function snapshot(contents) {
        var out = [], i, property, record, shape;
        for (i = 1; i <= contents.numProperties; i += 1) {
            property = contents.property(i);
            record = {name: property.name, matchName: property.matchName};
            if (property.matchName === 'ADBE Vector Group') record.contents = snapshot(property.property('ADBE Vectors Group'));
            if (property.matchName === 'ADBE Vector Filter - Merge') record.mode = property.property('ADBE Vector Merge Type').value;
            if (property.matchName === 'ADBE Vector Shape - Group') {
                shape = property.property('ADBE Vector Shape').value;
                record.vertices = shape.vertices;
                record.inTangents = shape.inTangents;
                record.outTangents = shape.outTangents;
                record.closed = shape.closed;
            }
            if (property.matchName === 'ADBE Vector Graphic - Fill') {
                record.color = property.property('ADBE Vector Fill Color').value;
                record.fillRule = property.property('ADBE Vector Fill Rule').value;
                record.opacity = property.property('ADBE Vector Fill Opacity').value;
            }
            out.push(record);
        }
        return out;
    }
    var modes = [3, 4, 5], names = ['Subtract', 'Intersect', 'Exclude'], row, column;
    for (row = 0; row < 2; row += 1) {
        for (column = 0; column < 3; column += 1) {
            var layer = comp.layers.addShape();
            layer.name = names[column] + (row ? ' overlapping contours' : ' disjoint contours');
            var transform = layer.property('ADBE Transform Group');
            var position = [53 + 107 * column, 45 + 90 * row];
            transform.property('ADBE Anchor Point').setValue([0, 0]);
            transform.property('ADBE Position').setValue(position);
            var contents = layer.property('ADBE Root Vectors Group');
            // FX Subtract retains its last operand; AE retains its first.
            // This comment was corrected after authoring; executable code is unchanged.
            if (modes[column] === 3) rectPath(contents, 'Cutter', [-24,-13,24,13], false);
            compound(contents, row === 1);
            if (modes[column] !== 3) rectPath(contents, 'Cutter', [-24,-13,24,13], false);
            merge(contents, 'Outer ' + names[column], modes[column]);
            var fill = contents.addProperty('ADBE Vector Graphic - Fill');
            fill.property('ADBE Vector Fill Color').setValue([1,1,1,1]);
            fill.property('ADBE Vector Fill Rule').setValue(1);
            cases.push({name: layer.name, position: position, operation: names[column], overlap: row === 1, contents: snapshot(layer.property('ADBE Root Vectors Group'))});
        }
    }
    comp.time = 0;
    return {adobeVersion: app.version, composition: {id: comp.id, name: comp.name, width: comp.width, height: comp.height, frameRate: comp.frameRate, duration: comp.duration}, cases: cases, expressions: false, rendered: false};
}
