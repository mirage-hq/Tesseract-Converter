function read(spec) {
 var found=null;
 for(var i=1;i<=app.project.numItems;i++) {
  var item=app.project.item(i);
  if(item instanceof CompItem && item.name===spec.name) {
   if(found!==null)throw new Error('ambiguous exact duration composition');
   found=item;
  }
 }
 if(found===null)throw new Error('missing exact duration composition');
 if(found.frameRate!==30 || found.width!==64 || found.height!==64 || found.numLayers!==1)throw new Error('unexpected generated composition profile');
 if(Math.round(found.duration*30)!==spec.frames || Math.abs(found.duration-spec.frames/30)>1/24576)throw new Error('generated native duration mismatch');
 var layer=found.layer(1);
 if(layer.name!=='Editable duration control')throw new Error('editable layer missing');
 var solid=layer.source;
 if(!(solid instanceof FootageItem) || !(solid.mainSource instanceof SolidSource))throw new Error('editable native solid source missing');
 var color=solid.mainSource.color;
 if(color[0]!==1 || color[1]!==0 || color[2]!==0)throw new Error('editable solid color changed');
 return {name:found.name,id:found.id,duration:found.duration,fps:found.frameRate,frames:Math.round(found.duration*30),layers:found.numLayers,layer:layer.name,solidSource:true,solidColor:color,start:layer.startTime,inPoint:layer.inPoint,outPoint:layer.outPoint};
}
var outputs=[];
for(var n=0;n<context.data.cases.length;n++) {
 var spec=context.data.cases[n];
 app.open(new File(context.assets[spec.asset]));
 var opened=read(spec);
 app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES);
 app.open(new File(context.assets[spec.asset]));
 var reopened=read(spec);
 app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES);
 outputs.push({asset:spec.asset,opened:opened,reopened:reopened});
}
return {result:{outputs:outputs},targets:[]};
