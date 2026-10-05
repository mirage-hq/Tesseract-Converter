var result={errors:[],versions:[]};
function comp(name){var c=null,n=0;for(var i=1;i<=app.project.numItems;i++){var x=app.project.item(i);if(x instanceof CompItem&&x.name===name){c=x;n++;}}if(n!==1)throw Error('comp '+name+' count '+n);return c;}
function keys(p){var a=[];for(var k=1;k<=p.numKeys;k++)a.push({time:p.keyTime(k),value:p.keyValue(k),incoming:Number(p.keyInInterpolationType(k)),outgoing:Number(p.keyOutInterpolationType(k))});return a;}
try{for(var v=0;v<2;v++){
 app.open(new File(context.assets[v===0?'original':'edited']));
 for(var i=1;i<=app.project.numItems;i++){var item=app.project.item(i);if(item instanceof FootageItem&&item.file)item.replace(new File(context.assets.clip));}
 app.project.save(new File(context.output_path));app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES);app.open(new File(context.output_path));
 var root=comp('Comp 1'),plane=comp('video-8990'),outer=root.layer('video-8990'),inner=plane.layer('video-8990 source controls');
 var op=inner.property('ADBE Transform Group').property('ADBE Opacity'),remap=outer.property('ADBE Time Remapping'),fx=inner.property('ADBE Effect Parade').property('ADBE Mosaic');
 var row={version:v,root:{width:root.width,height:root.height,duration:root.duration,frameRate:root.frameRate,layers:root.numLayers},source:{duration:plane.duration,layers:plane.numLayers},online:!inner.source.footageMissing,opacity:keys(op),remap:keys(remap),horizontal:keys(fx.property('ADBE Mosaic-0001')),vertical:keys(fx.property('ADBE Mosaic-0002')),samples:[]};
 for(var t=0;t<context.data.times.length;t++){var pt=context.data.times[t],st=remap.valueAtTime(pt,true);row.samples.push({parentTime:pt,sourceTime:st,opacity:op.valueAtTime(st,true)});}
 result.versions.push(row);app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES);
 }}catch(e){result.errors.push(String(e));}
return {result:result,targets:[]};
