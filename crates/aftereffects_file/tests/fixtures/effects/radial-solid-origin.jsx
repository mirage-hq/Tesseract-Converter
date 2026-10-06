// Managed JSX body: independent native controls plus fresh forward-export readback.
function read(c) {
    if (c.numLayers !== 1) throw new Error('Expected one editable owner');
    var l = c.layer(1), t = l.property('ADBE Transform Group');
    var p = l.property('ADBE Effect Parade');
    if (p.numProperties !== 1) throw new Error('Expected sole editable effect');
    var e = p.property(1);
    return {solid: l.source.mainSource instanceof SolidSource,
        width: l.source.width, height: l.source.height,
        center: e.property('ADBE Radial Blur-0002').value,
        amount: e.property('ADBE Radial Blur-0001').value,
        mode: e.property('ADBE Radial Blur-0003').value,
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
function author(name, center) {
    var c = app.project.items.addComp(name, 180, 160, 1, 1, 30);
    var l = c.layers.addSolid([1,0,0], 'Independent radial solid', 40, 30, 1, 1);
    var t = l.property('ADBE Transform Group');
    t.property('ADBE Anchor Point').setValue([20,15]);
    t.property('ADBE Position').setValue([90,80]);
    t.property('ADBE Scale').setValue([100,100]);
    t.property('ADBE Rotate Z').setValue(0);
    var e = l.property('ADBE Effect Parade').addProperty('ADBE Radial Blur');
    e.property('ADBE Radial Blur-0001').setValue(20);
    e.property('ADBE Radial Blur-0002').setValue(center);
    e.property('ADBE Radial Blur-0003').setValue(2);
    return c;
}
// Base FX: local center[20,15],Rect origin[7,-5] => source[13,20].
// Edited FX: local center[32,6],Rect origin[12,3] => source[20,3].
var base = author('W07 independent radial origin base', [13,20]);
var edited = author('W07 independent radial origin edit', [20,3]);
observations.oracleBase = read(base);
observations.oracleEdited = read(edited);
app.project.save(new File(context.output_path));
return {result: observations, targets: [
    {id:'base', name:base.name, native_id:String(base.id)},
    {id:'edited', name:edited.name, native_id:String(edited.id)}
]};
