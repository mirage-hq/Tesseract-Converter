// Managed function body. Never abort on a semantic mismatch: return raw observations.
var observations={errors:[],samples:[],requestedOpacityTimes:[0,0.04,0.08],requestedOpacityValues:[0,50,100],requestedRemapTimes:[2.85,7.75,11.45,17.95,18],requestedRemapValues:[0,5.5,8.2,14.958,14.958]}, targets=[];
function compNamed(name) {
    var found=null, count=0;
    for(var i=1;i<=app.project.numItems;i++) {
        var item=app.project.item(i);
        if(item instanceof CompItem && item.name===name) {found=item;count++;}
    }
    if(count!==1) observations.errors.push('composition identity '+name+': '+count);
    return found;
}
function keys(track) {
    var result=[];
    for(var k=1;k<=track.numKeys;k++) result.push({time:track.keyTime(k),value:track.keyValue(k),incoming:Number(track.keyInInterpolationType(k)),outgoing:Number(track.keyOutInterpolationType(k))});
    return result;
}
function linear(track) {
    for(var k=1;k<=track.numKeys;k++) track.setInterpolationTypeAtKey(k,KeyframeInterpolationType.LINEAR,KeyframeInterpolationType.LINEAR);
}
function evaluate(track,time,pre) {
    try { return {value:track.valueAtTime(time,pre),expressionError:track.expressionError}; }
    catch(error) { return {error:String(error)}; }
}
var probe=null;
try {
    var source=app.project.items.addComp('P019 public source clock plane',320,180,1,16,30);
    var clip=app.project.importFile(new ImportOptions(new File(context.assets.clip)));
    observations.clip={width:clip.width,height:clip.height,duration:clip.duration,frameRate:clip.frameRate};
    var video=source.layers.add(clip);video.name='Timecoded source-owned controls';video.audioEnabled=false;
    video.transform.opacity.setValuesAtTimes(observations.requestedOpacityTimes,observations.requestedOpacityValues);linear(video.transform.opacity);
    video.property('ADBE Effect Parade').addProperty('ADBE Mosaic');
    var mosaic=video.property('ADBE Effect Parade').property('ADBE Mosaic');
    mosaic.property('ADBE Mosaic-0001').setValuesAtTimes([0,2.5,5.5,16],[54,54,320,320]);
    mosaic.property('ADBE Mosaic-0002').setValuesAtTimes([0,2.5,5.5,16],[30,30,180,180]);
    linear(mosaic.property('ADBE Mosaic-0001'));linear(mosaic.property('ADBE Mosaic-0002'));
    var root=app.project.items.addComp('P019 public dual-clock occurrence',320,180,1,18,30);
    var sibling=root.layers.addSolid([0,1,0],'Unaffected sibling',8,8,1,18);sibling.transform.position.setValue([8,8]);
    var outer=root.layers.add(source);outer.name='Source-clock occurrence';outer.collapseTransformation=false;
    outer.startTime=2.85;outer.inPoint=2.85;outer.outPoint=18;outer.timeRemapEnabled=true;
    var remap=outer.property('ADBE Time Remapping'),times=observations.requestedRemapTimes,values=observations.requestedRemapValues;
    remap.setValuesAtTimes(times,values);
    for(var k=remap.numKeys;k>=1;k--) {
        var keep=false;
        for(var t=0;t<times.length;t++) if(Math.abs(remap.keyTime(k)-times[t])<0.000001) keep=true;
        if(!keep) remap.removeKey(k);
    }
    linear(remap);
    app.project.save(new File(context.output_path));app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES);app.open(new File(context.output_path));
    root=compNamed('P019 public dual-clock occurrence');source=compNamed('P019 public source clock plane');
    outer=root.layer('Source-clock occurrence');video=source.layer('Timecoded source-owned controls');
    remap=outer.property('ADBE Time Remapping');mosaic=video.property('ADBE Effect Parade').property('ADBE Mosaic');
    observations.root={name:root.name,id:root.id,width:root.width,height:root.height,frameRate:root.frameRate,duration:root.duration,numLayers:root.numLayers};
    observations.source={name:source.name,id:source.id,width:source.width,height:source.height,frameRate:source.frameRate,duration:source.duration,numLayers:source.numLayers};
    observations.outer={startTime:outer.startTime,inPoint:outer.inPoint,outPoint:outer.outPoint,stretch:outer.stretch,collapseTransformation:outer.collapseTransformation,timeRemapEnabled:outer.timeRemapEnabled};
    observations.fixture={footageMissing:video.source.footageMissing,file:video.source.file.fsName,videoEnabled:video.enabled,outerEnabled:outer.enabled,videoIn:video.inPoint,videoOut:video.outPoint,videoStart:video.startTime,videoStretch:video.stretch};
    observations.video={name:video.name,timeRemapEnabled:video.timeRemapEnabled,audioEnabled:video.audioEnabled,numEffects:video.property('ADBE Effect Parade').numProperties};
    observations.remapKeys=keys(remap);observations.opacityKeys=keys(video.transform.opacity);
    observations.horizontalKeys=keys(mosaic.property('ADBE Mosaic-0001'));observations.verticalKeys=keys(mosaic.property('ADBE Mosaic-0002'));
    probe=root.layers.addNull();probe.name='Temporary mapped-source probe';
    var effect=probe.property('ADBE Effect Parade').addProperty('ADBE Slider Control');effect.name='SourceTime Probe';
    effect=probe.property('ADBE Effect Parade').addProperty('ADBE Slider Control');effect.name='Plane Alpha Probe';
    var slider=probe.property('ADBE Effect Parade').property('SourceTime Probe').property('ADBE Slider Control-0001');
    var alpha=probe.property('ADBE Effect Parade').property('Plane Alpha Probe').property('ADBE Slider Control-0001');
    slider.expression='thisComp.layer("Source-clock occurrence").sourceTime(time)';
    alpha.expression='100*thisComp.layer("Source-clock occurrence").sampleImage([160,90],[1,1],true,time)[3]';
    var sampleTimes=[2.85,2.8505,2.88,2.85+0.08*4.9/5.5,3,5,7.75,9,11.45,15,17.95,17.999];
    for(var i=0;i<sampleTimes.length;i++) {
        var time=sampleTimes[i],mapped=evaluate(slider,time,false),sample={parentTime:time,sourceTime:mapped,remapAtParentTime:evaluate(remap,time,true),nativePlaneAlpha:evaluate(alpha,time,false)};
        if(typeof mapped.value==='number' && isFinite(mapped.value)) {
            sample.opacity=evaluate(video.transform.opacity,mapped.value,true);
            sample.horizontalBlocks=evaluate(mosaic.property('ADBE Mosaic-0001'),mapped.value,true);
            sample.verticalBlocks=evaluate(mosaic.property('ADBE Mosaic-0002'),mapped.value,true);
        }
        observations.samples.push(sample);
    }
    probe.remove();probe=null;observations.temporaryProbeRemoved=root.numLayers===2;
    targets=[{id:'source-plane',name:source.name,native_id:String(source.id)},{id:'occurrence',name:root.name,native_id:String(root.id)}];
} catch(error) {observations.errors.push(String(error));if(probe) {try {probe.remove();} catch(cleanupError) {observations.errors.push(String(cleanupError));}}}
return {result:observations,targets:targets};
