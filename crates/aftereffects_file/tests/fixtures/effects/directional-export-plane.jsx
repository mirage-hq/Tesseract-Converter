// Independent native controls and fresh forward-export readback; managed body only.
function read(c) {
    if (c.numLayers !== 1) throw new Error('Expected one editable owner');
    var l = c.layer(1), t = l.property('ADBE Transform Group');
    var p = l.property('ADBE Effect Parade');
    if (p.numProperties !== 1) throw new Error('Expected sole editable effect');
    var e = p.property(1);
    return {solid: l.source.mainSource instanceof SolidSource,
        direction: e.property('ADBE Motion Blur-0001').value,
        length: e.property('ADBE Motion Blur-0002').value,
        effect: e.matchName, enabled: e.enabled,
        scale: t.property('ADBE Scale').value,
        rotation: t.property('ADBE Rotate Z').value,
        anchor: t.property('ADBE Anchor Point').value,
        position: t.property('ADBE Position').value};
}
function sourceComp() {
    var found = null;
    for (var i = 1; i <= app.project.numItems; i++) {
        if (app.project.item(i) instanceof CompItem) {
            if (found !== null) throw new Error('Ambiguous exported composition');
            found = app.project.item(i);
        }
    }
    if (found === null) throw new Error('Missing exported composition');
    return found;
}
var observations = {};
var inputs = ['red', 'base', 'edited'];
for (var i = 0; i < inputs.length; i++) {
    var key = inputs[i];
    app.open(new File(context.assets[key]));
    observations[key] = read(sourceComp());
    app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES);
}
app.newProject();
function author(name, scale, rotation, direction, length) {
    var c = app.project.items.addComp(name, 180, 160, 1, 1, 30);
    var l = c.layers.addSolid([1,0,0], 'Independent directional solid', 12, 12, 1, 1);
    var t = l.property('ADBE Transform Group');
    t.property('ADBE Anchor Point').setValue([6,6]);
    t.property('ADBE Position').setValue([90,80]);
    t.property('ADBE Scale').setValue([scale,scale]);
    t.property('ADBE Rotate Z').setValue(rotation);
    var e = l.property('ADBE Effect Parade').addProperty('ADBE Motion Blur');
    e.property('ADBE Motion Blur-0001').setValue(direction);
    e.property('ADBE Motion Blur-0002').setValue(length);
    return c;
}
var base = author('W07 independent export base', 200, 90, 0, 20);
var edited = author('W07 independent export edit', 300, -45, 60, 30);
observations.oracleBase = read(base);
observations.oracleEdited = read(edited);
app.project.save(new File(context.output_path));
return {result: observations, targets: [
    {id:'base', name:base.name, native_id:String(base.id)},
    {id:'edited', name:edited.name, native_id:String(edited.id)}
]};
