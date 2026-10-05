var results = [];
var keys = ["original", "edited"];
for (var i = 0; i < keys.length; i++) {
    app.open(new File(context.assets[keys[i]]));
    var comp = null;
    for (var j = 1; j <= app.project.numItems; j++) {
        var item = app.project.item(j);
        if (item instanceof CompItem) { if (comp !== null) throw new Error("Ambiguous composition"); comp = item; }
    }
    if (comp === null || comp.numLayers !== 1) throw new Error("Expected one editable media layer");
    var layer = comp.layer(1);
    var source = layer.source;
    if (!(source instanceof FootageItem) || source.footageMissing) throw new Error("Missing or non-footage source");
    var before = {duration:source.duration, frameRate:source.frameRate, conform:source.mainSource.conformFrameRate};
    source.replace(new File(context.assets.movie));
    if (source.footageMissing || before.duration !== source.duration || before.frameRate !== source.frameRate) throw new Error("Pinned relink changed source metadata");
    results.push({target:keys[i], composition:{duration:comp.duration, fps:comp.frameRate, width:comp.width, height:comp.height}, source:{duration:source.duration, frameRate:source.frameRate, conform:source.mainSource.conformFrameRate, online:!source.footageMissing}, layer:{startTime:layer.startTime,inPoint:layer.inPoint,outPoint:layer.outPoint,stretch:layer.stretch,name:layer.name,timeRemapEnabled:layer.timeRemapEnabled}});
    app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES);
}
return {result:{controls:results},targets:[]};
