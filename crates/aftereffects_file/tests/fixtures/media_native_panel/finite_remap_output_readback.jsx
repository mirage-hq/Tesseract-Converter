app.open(new File(context.assets.project));
var footage=0,comps=[];
for(var i=1;i<=app.project.numItems;i++){
 var item=app.project.item(i);
 if(item instanceof FootageItem && item.mainSource instanceof FileSource){
  footage++;item.replace(new File(context.assets.clip));
 }
}
if(footage!==1)throw new Error('expected exactly one pinned movie');
var remaps=0,outer=0;
for(var i=1;i<=app.project.numItems;i++){
 var comp=app.project.item(i);if(!(comp instanceof CompItem))continue;
 var layers=[];
 for(var j=1;j<=comp.numLayers;j++){
  var layer=comp.layer(j),keys=[],p=layer.property('ADBE Time Remapping');
  if(p&&p.numKeys>0){
   remaps++;
   if(layer.name!=='Authored source remap'||p.numKeys!==3)throw new Error('unexpected remap owner/key count');
   var times=[0,2,8],values=[0.75,0.75,8];
   for(var k=1;k<=p.numKeys;k++){
    var key={time:p.keyTime(k),value:p.keyValue(k),incoming:Number(p.keyInInterpolationType(k)),outgoing:Number(p.keyOutInterpolationType(k))};
    if(key.time!==times[k-1]||key.value!==values[k-1]||key.incoming!==6612||key.outgoing!==6612)throw new Error('remap key changed');
    keys.push(key);
   }
   if(layer.source.duration!==8)throw new Error('certified source duration changed');
  }
  if(comp.name==='media-static-remap'&&layer.name==='Held Blue Frame'){
   outer++;
   if(layer.startTime!==-0.25||layer.inPoint!==0.25||layer.outPoint!==1.25||layer.stretch!==100||layer.source.duration!==8)throw new Error('edited outer occurrence clock changed');
  }
  layers.push({name:layer.name,start:layer.startTime,inPoint:layer.inPoint,outPoint:layer.outPoint,stretch:layer.stretch,sourceDuration:layer.source?layer.source.duration:null,keys:keys});
 }
 comps.push({name:comp.name,id:comp.id,duration:comp.duration,layers:layers});
}
if(remaps!==1||outer!==1)throw new Error('missing or ambiguous native proof target');
return {result:{footage:footage,remaps:remaps,outer:outer,compositions:comps},targets:[]};
