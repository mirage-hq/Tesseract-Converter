app.newProject();
var c=app.project.items.addComp('p033-anchor-union-controls',160,120,1,2,24);
var l=c.layers.addSolid([1,0.2,0.1],'anchor-owner',160,120,1,2);
var a=l.property('ADBE Transform Group').property('ADBE Anchor Point');
a.setValueAtTime(0,[0,0,0]);a.setValueAtTime(0.5,[40,10,0]);a.setValueAtTime(1,[40,20,0]);
for(var k=1;k<=3;k++){
 a.setInterpolationTypeAtKey(k,KeyframeInterpolationType.LINEAR,KeyframeInterpolationType.LINEAR);
 a.setSpatialTangentsAtKey(k,[0,0,0],[0,0,0]);
}
app.project.save(new File(context.output_path));
app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES);app.open(new File(context.output_path));
var count=0;c=null;
for(var i=1;i<=app.project.numItems;i++){var item=app.project.item(i);if(item instanceof CompItem && item.name==='p033-anchor-union-controls'){c=item;count++;}}
if(count!==1||c.width!==160||c.height!==120||c.frameRate!==24||c.duration!==2||c.numLayers!==1)throw new Error('unexpected Anchor target');
l=c.layer('anchor-owner');a=l.property('ADBE Transform Group').property('ADBE Anchor Point');
if(a.matchName!=='ADBE Anchor Point'||a.numKeys!==3)throw new Error('unexpected Anchor controls');
var keys=[],samples=[],times=[0,0.25,0.5,0.75,1,1.25,1.75];
for(var k=1;k<=3;k++)keys.push({time:a.keyTime(k),value:a.keyValue(k),incoming:Number(a.keyInInterpolationType(k)),outgoing:Number(a.keyOutInterpolationType(k)),spatialIn:a.keyInSpatialTangent(k),spatialOut:a.keyOutSpatialTangent(k)});
for(var j=0;j<times.length;j++)samples.push({time:times[j],value:a.valueAtTime(times[j],true)});
return {result:{keys:keys,samples:samples},targets:[{id:'p033-anchor-union',name:c.name,native_id:String(c.id)}]};
