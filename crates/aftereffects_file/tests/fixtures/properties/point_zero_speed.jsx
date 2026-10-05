app.newProject();
var c=app.project.items.addComp('point-zero-speed-controls',160,120,1,2,24);
var l=c.layers.addSolid([1,0,0],'point-owner',120,80,1,2);
l.property('ADBE Effect Parade').addProperty('ADBE Radial Blur');
var p=l.property('ADBE Effect Parade').property('ADBE Radial Blur').property('ADBE Radial Blur-0002');
p.setValueAtTime(0.5,[60,40]);p.setValueAtTime(1.5,[72,33]);
for(var k=1;k<=2;k++){
 p.setInterpolationTypeAtKey(k,KeyframeInterpolationType.BEZIER,KeyframeInterpolationType.BEZIER);
 p.setTemporalEaseAtKey(k,[new KeyframeEase(0,100/3)],[new KeyframeEase(0,100/3)]);
 p.setSpatialTangentsAtKey(k,[0,0],[0,0]);
}
app.project.save(new File(context.output_path));
app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES);app.open(new File(context.output_path));
var count=0;c=null;
for(var i=1;i<=app.project.numItems;i++){var item=app.project.item(i);if(item instanceof CompItem && item.name==='point-zero-speed-controls'){c=item;count++;}}
if(count!==1||c.width!==160||c.height!==120||c.frameRate!==24||c.duration!==2||c.numLayers!==1)throw new Error('unexpected native Point target');
l=c.layer('point-owner');p=l.property('ADBE Effect Parade').property('ADBE Radial Blur').property('ADBE Radial Blur-0002');
if(p.matchName!=='ADBE Radial Blur-0002'||p.numKeys!==2)throw new Error('unexpected Point key count');
var keys=[],samples=[],times=[0,0.5,0.75,1,1.25,1.5,1.75];
for(var k=1;k<=2;k++)keys.push({time:p.keyTime(k),value:p.keyValue(k),incoming:Number(p.keyInInterpolationType(k)),outgoing:Number(p.keyOutInterpolationType(k)),inSpeed:p.keyInTemporalEase(k)[0].speed,outSpeed:p.keyOutTemporalEase(k)[0].speed,inInfluence:p.keyInTemporalEase(k)[0].influence,outInfluence:p.keyOutTemporalEase(k)[0].influence,spatialIn:p.keyInSpatialTangent(k),spatialOut:p.keyOutSpatialTangent(k)});
for(var j=0;j<times.length;j++)samples.push({time:times[j],value:p.valueAtTime(times[j],true)});
return {result:{ownerWidth:l.width,ownerHeight:l.height,keys:keys,samples:samples},targets:[{id:'point-zero-speed',name:c.name,native_id:String(c.id)}]};
