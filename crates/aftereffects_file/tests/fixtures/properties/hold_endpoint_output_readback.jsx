var times=[-0.5,0,0.25,0.5,0.75,1.5,1.75];
function target(name){
 var comp=null;
 for(var i=1;i<=app.project.numItems;i++)if(app.project.item(i) instanceof CompItem){
  if(comp!==null)throw new Error('ambiguous composition');comp=app.project.item(i);
 }
 if(comp===null)throw new Error('missing composition');
 var layer=null;
 for(var j=1;j<=comp.numLayers;j++)if(comp.layer(j).name===name){
  if(layer!==null)throw new Error('ambiguous layer');layer=comp.layer(j);
 }
 if(layer===null)throw new Error('missing layer '+name);
 return {comp:comp,layer:layer,property:layer.property('ADBE Transform Group').property('ADBE Opacity')};
}
function read(t){
 var p=t.property,keys=[],samples=[];
 if(p.numKeys!==3)throw new Error('expected exactly three opacity keys');
 for(var k=1;k<=p.numKeys;k++)keys.push({time:p.keyTime(k),value:p.keyValue(k),incoming:Number(p.keyInInterpolationType(k)),outgoing:Number(p.keyOutInterpolationType(k))});
 for(var n=0;n<times.length;n++)samples.push({time:times[n],value:p.valueAtTime(times[n],true)});
 return {composition:t.comp.name,layer:t.layer.name,fps:t.comp.frameRate,duration:t.comp.duration,keys:keys,samples:samples};
}
var outputs=[];
for(var i=0;i<context.data.cases.length;i++){
 var spec=context.data.cases[i];app.open(new File(context.assets[spec.asset]));
 var opened=read(target(spec.layer));
 app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES);
 app.open(new File(context.assets[spec.asset]));
 var reopened=read(target(spec.layer));
 outputs.push({asset:spec.asset,middle:spec.middle,opened:opened,reopened:reopened});
 app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES);
}
app.open(new File(context.assets.oracle));
var independent=target('control-2'),before=read(independent);
independent.property.setValueAtKey(2,40);
var edited=read(independent);
app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES);
return {result:{outputs:outputs,independent:{before:before,edited:edited}},targets:[]};
