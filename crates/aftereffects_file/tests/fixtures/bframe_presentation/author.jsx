var footage = app.project.importFile(new ImportOptions(new File(context.assets.movie)));
var controls = [];
var targets = [];
for (var i = 0; i < 2; i++) {
    var duration = i === 0 ? 1 : 0.5;
    var comp = app.project.items.addComp(i === 0 ? "B-frame original" : "B-frame edited rate", 160, 90, 1, duration, 30);
    var layer = comp.layers.add(footage);
    layer.stretch = i === 0 ? 100 : 50;
    layer.startTime = 0;
    layer.inPoint = 0;
    layer.outPoint = duration;
    controls.push({name: comp.name, id: comp.id, duration: comp.duration, fps: comp.frameRate, startTime: layer.startTime, inPoint: layer.inPoint, outPoint: layer.outPoint, stretch: layer.stretch, timeRemapEnabled: layer.timeRemapEnabled});
    targets.push({id: i === 0 ? "original" : "edited", name: comp.name, native_id: String(comp.id)});
}
app.project.save(new File(context.output_path));
return {result: {footage: {width: footage.width, height: footage.height, frameRate: footage.frameRate, duration: footage.duration, conformFrameRate: footage.mainSource.conformFrameRate}, controls: controls}, targets: targets};
