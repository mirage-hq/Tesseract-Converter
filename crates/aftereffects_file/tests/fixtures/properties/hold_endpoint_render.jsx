app.newProject();
var c=app.project.items.addComp('hold-render-public',128,64,1,2,24);
var driver=c.layers.addSolid([0,0,0],'driver',1,1,1,2);
driver.enabled=false;
var alias=c.layers.addSolid([1,0,0],'rotation-alias',48,8,1,2);
alias.property('ADBE Transform Group').property('ADBE Position').setValue([32,32]);
alias.property('ADBE Transform Group').property('ADBE Rotate Z').expression='thisComp.layer("driver").transform.rotation';
var opacity=c.layers.addSolid([1,0,0],'opacity-control',64,64,1,2);
opacity.property('ADBE Transform Group').property('ADBE Position').setValue([96,32]);
function keys(p,values){
 var ts=[-0.5,0.5,1.5];
 for(var i=0;i<3;i++)p.setValueAtTime(ts[i],values[i]);
 for(var j=1;j<=3;j++)p.setInterpolationTypeAtKey(j,KeyframeInterpolationType.LINEAR,j<3?KeyframeInterpolationType.HOLD:KeyframeInterpolationType.LINEAR);
}
keys(driver.property('ADBE Transform Group').property('ADBE Rotate Z'),[0,90,180]);
keys(opacity.property('ADBE Transform Group').property('ADBE Opacity'),[0,100,40]);
app.project.save(new File(context.output_path));
app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES);
app.open(new File(context.output_path));
var target=null;
for(var n=1;n<=app.project.numItems;n++)if(app.project.item(n) instanceof CompItem && app.project.item(n).name==='hold-render-public'){
 if(target!==null)throw new Error('ambiguous comp');target=app.project.item(n);
}
if(target===null || target.numLayers!==3)throw new Error('unexpected comp/layers');
if(target.frameRate!==24 || target.duration!==2 || target.width!==128 || target.height!==64)throw new Error('metadata drift');
var byName={};
for(var l=1;l<=target.numLayers;l++)byName[target.layer(l).name]=target.layer(l);
if(byName.driver.enabled || !byName['rotation-alias'].enabled || !byName['opacity-control'].enabled)throw new Error('visibility drift');
var result={composition:{id:String(target.id),name:target.name,width:target.width,height:target.height,fps:target.frameRate,duration:target.duration},layers:[],samples:[]};
for(var name in byName){
 var layer=byName[name],tr=layer.property('ADBE Transform Group');
 var p=tr.property(name==='opacity-control'?'ADBE Opacity':'ADBE Rotate Z'),ks=[];
 for(var k=1;k<=p.numKeys;k++)ks.push({time:p.keyTime(k),value:p.keyValue(k),incoming:Number(p.keyInInterpolationType(k)),outgoing:Number(p.keyOutInterpolationType(k))});
 result.layers.push({name:name,enabled:layer.enabled,position:tr.property('ADBE Position').value,keys:ks,expression:p.expression});
}
var times=[0,14/30,0.5,16/30,1,44/30,1.5,46/30,59/30];
var rot=byName['rotation-alias'].property('ADBE Transform Group').property('ADBE Rotate Z');
var op=byName['opacity-control'].property('ADBE Transform Group').property('ADBE Opacity');
for(var t=0;t<times.length;t++){
 var time=times[t],er=time<0.5?0:(time<1.5?90:180),eo=time<0.5?0:(time<1.5?100:40);
 var r=rot.valueAtTime(time,false),o=op.valueAtTime(time,false);
 if(Math.abs(r-er)>0.000001 || Math.abs(o-eo)>0.000001)throw new Error('native sample mismatch');
 result.samples.push({time:time,rotation:r,opacity:o});
}
return {result:result,targets:[{id:'public-hold',name:target.name,native_id:String(target.id)}]};
