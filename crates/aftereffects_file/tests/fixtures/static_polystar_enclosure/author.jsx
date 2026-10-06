// Managed headless-adobe function body. Independently authors only PolyStar controls.
app.newProject();
for (var kind = 1; kind <= 2; kind++) {
    var comp = app.project.items.addComp(kind === 1 ? "Static Star" : "Static Polygon", 512, 512, 1, 1, 24);
    var layer = comp.layers.addShape();
    layer.name = kind === 1 ? "Five-point Star" : "Five-point Polygon";
    layer.property("ADBE Transform Group").property("ADBE Position").setValue([256, 256]);
    var contents = layer.property("ADBE Root Vectors Group");
    var star = contents.addProperty("ADBE Vector Shape - Star");
    star.property("ADBE Vector Star Type").setValue(kind);
    star.property("ADBE Vector Star Points").setValue(5);
    star.property("ADBE Vector Star Position").setValue([0, 0]);
    star.property("ADBE Vector Star Rotation").setValue(0);
    star.property("ADBE Vector Star Outer Radius").setValue(210);
    star.property("ADBE Vector Star Outer Roundess").setValue(0);
    if (kind === 1) {
        star.property("ADBE Vector Star Inner Radius").setValue(90);
        star.property("ADBE Vector Star Inner Roundess").setValue(0);
    }
    var fill = contents.addProperty("ADBE Vector Graphic - Fill");
    fill.property("ADBE Vector Fill Color").setValue([0.2, 0.6, 0.9]);
}
app.project.save(new File(context.output_path));
app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES);
app.open(new File(context.output_path));
var observed = [], targets = [];
for (var i = 1; i <= app.project.numItems; i++) {
    var c = app.project.item(i);
    if (!(c instanceof CompItem)) throw new Error("Unexpected non-composition item");
    if (c.numLayers !== 1) throw new Error("Fixture must contain one native Shape per composition");
    var shape = c.layer(1).property("ADBE Root Vectors Group").property("ADBE Vector Shape - Star");
    // AE exposes this popup as an enum object; normalize its numeric identity.
    var type = Number(shape.property("ADBE Vector Star Type").value);
    var points = shape.property("ADBE Vector Star Points").value;
    var radius = shape.property("ADBE Vector Star Outer Radius").value;
    var roundness = shape.property("ADBE Vector Star Outer Roundess").value;
    var expectedType = 0;
    if (String(c.name) === "Static Star") expectedType = 1;
    else if (String(c.name) === "Static Polygon") expectedType = 2;
    if (type !== expectedType || Number(points) !== 5 || Number(radius) !== 210 || Number(roundness) !== 0) throw new Error("Native PolyStar controls changed on reopen: " + c.name + " expectedType=" + expectedType + " type=" + type + " points=" + Number(points).toPrecision(17) + " radius=" + Number(radius).toPrecision(17) + " roundness=" + Number(roundness).toPrecision(17));
    observed.push({composition_id:c.id,name:c.name,type:type,points:points,outer_radius:radius,outer_roundness:roundness,position:shape.property("ADBE Vector Star Position").value});
    targets.push({id:type === 1 ? "star" : "polygon",name:c.name,native_id:String(c.id)});
}
return {result:{compositions:observed,same_host_save_reopen:true},targets:targets};
