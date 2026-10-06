var footage = app.project.importFile(new ImportOptions(new File(context.assets.movie)));
var controls = [];
var targets = [];
for (var i = 0; i < 2; i++) {
    var duration = i === 0 ? footage.duration : footage.duration / 2;
    var comp = app.project.items.addComp(i === 0 ? "NTSC original" : "NTSC rate edit", 160, 90, 1, duration, 30);
    var layer = comp.layers.add(footage);
    layer.stretch = i === 0 ? 100 : 50;
    layer.startTime = 0;
    layer.inPoint = 0;
    layer.outPoint = duration;
    controls.push({id:comp.id, name:comp.name, duration:comp.duration, fps:comp.frameRate, startTime:layer.startTime, inPoint:layer.inPoint, outPoint:layer.outPoint, stretch:layer.stretch});
    targets.push({id:i === 0 ? "original" : "edited", name:comp.name, native_id:String(comp.id)});
}
app.project.save(new File(context.output_path));
return {result:{footage:{width:footage.width,height:footage.height,frameRate:footage.frameRate,duration:footage.duration,conformFrameRate:footage.mainSource.conformFrameRate},controls:controls},targets:targets};
