app.newProject();
var name='W09 P038 beyond-duration footage native control';
var comp=app.project.items.addComp(name,32,32,1,365/30,30);
var names=['image','movie','audio'];
for(var i=0;i<names.length;i++){
 var source=app.project.importFile(new ImportOptions(new File(context.assets[names[i]])));
 var l=comp.layers.add(source);l.name=names[i];l.startTime=0;l.inPoint=0;l.outPoint=12.167;
}
function reopen(){
 app.project.save(new File(context.output_path));app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES);app.open(new File(context.output_path));
 comp=null;for(var i=1;i<=app.project.numItems;i++)if(app.project.item(i) instanceof CompItem&&app.project.item(i).name===name){if(comp)throw new Error('ambiguous composition');comp=app.project.item(i);}
 if(!comp||comp.numLayers!==3)throw new Error('composition/layer identity mismatch');
}
function read(expected){
 if(Math.abs(comp.duration-365/30)>1e-6||comp.frameRate!==30)throw new Error('root clock changed');
 var rows=[];
 for(var i=0;i<names.length;i++){
  var l=comp.layer(names[i]);if(!l||!l.source||l.startTime!==0||l.inPoint!==0)throw new Error('source or occurrence clock mismatch');
  if(Math.abs(l.outPoint-expected)>1/1920||l.outPoint<=comp.duration+1e-6)throw new Error('native outpoint not retained beyond root duration');
  rows.push({name:l.name,sourceId:l.source.id,sourceName:l.source.name,sourceDuration:l.source.duration,sourceFrameRate:l.source.frameRate,isStill:l.source.mainSource.isStill,hasVideo:l.hasVideo,hasAudio:l.hasAudio,inPoint:l.inPoint,outPoint:l.outPoint,startTime:l.startTime});
 }
 return {compositionId:comp.id,compositionName:comp.name,duration:comp.duration,frameRate:comp.frameRate,layers:rows};
}
reopen();var before=read(12.167);
for(var i=0;i<names.length;i++)comp.layer(names[i]).outPoint=12.2;
reopen();var edited=read(12.2);
for(var i=0;i<names.length;i++)comp.layer(names[i]).outPoint=12.167;
reopen();var restored=read(12.167);
return {result:{before:before,edited:edited,restored:restored,inputEdit:'all three outPoints12.167 ->12.2 ->12.167; save/reopen each; root/source/start/inPoint unchanged'},targets:[{id:'control',name:comp.name,native_id:String(comp.id)}]};
