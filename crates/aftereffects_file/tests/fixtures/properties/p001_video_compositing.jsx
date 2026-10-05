app.newProject();
var footage=app.project.importFile(new ImportOptions(new File(context.assets.movie)));
var comp=app.project.items.addComp('p001-video-compositing',320,180,1,2,24);
var screen=comp.layers.add(footage);screen.name='Screen video';screen.outPoint=2;screen.blendingMode=BlendingMode.SCREEN;
var inverted=comp.layers.add(footage);inverted.name='Inverted video';inverted.outPoint=2;
var invProvider=comp.layers.addSolid([0.25,0.25,0.25],'Inverted matte',320,180,1,2);inverted.setTrackMatte(invProvider,TrackMatteType.LUMA_INVERTED);invProvider.enabled=false;
var luma=comp.layers.add(footage);luma.name='Luma video';luma.outPoint=2;
var provider=comp.layers.addSolid([0.75,0.75,0.75],'Luma matte',320,180,1,2);luma.setTrackMatte(provider,TrackMatteType.LUMA);provider.enabled=false;
luma.property('ADBE Transform Group').property('ADBE Opacity').setValueAtTime(0,100);
luma.property('ADBE Transform Group').property('ADBE Opacity').setValueAtTime(1,50);
var opacity=luma.property('ADBE Transform Group').property('ADBE Opacity');for(var k=1;k<=2;k++)opacity.setInterpolationTypeAtKey(k,KeyframeInterpolationType.LINEAR,KeyframeInterpolationType.LINEAR);
app.project.save(new File(context.output_path));app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES);app.open(new File(context.output_path));
var target=null;for(var i=1;i<=app.project.numItems;i++){var item=app.project.item(i);if(item instanceof CompItem&&item.name==='p001-video-compositing'){if(target)throw Error('duplicate comp');target=item;}}
if(!target||target.numLayers!==5)throw Error('wrong composition');
var out=[];for(var n=1;n<=target.numLayers;n++){var l=target.layer(n);var row={name:l.name,index:l.index,blend:Number(l.blendingMode),matte:Number(l.trackMatteType),provider:l.trackMatteLayer?l.trackMatteLayer.name:null,enabled:l.enabled,inPoint:l.inPoint,outPoint:l.outPoint};if(l.name==='Luma video'){var p=l.property('ADBE Transform Group').property('ADBE Opacity');if(p.numKeys!==2)throw Error('wrong keys');row.keys=[[p.keyTime(1),p.keyValue(1)],[p.keyTime(2),p.keyValue(2)]];row.samples=[p.valueAtTime(0,false),p.valueAtTime(.5,false),p.valueAtTime(1,false)];}out.push(row);}
return {result:{layers:out,screen:Number(BlendingMode.SCREEN),luma:Number(TrackMatteType.LUMA),inverted:Number(TrackMatteType.LUMA_INVERTED)},targets:[{id:'control',native_id:String(target.id),name:target.name}]};
