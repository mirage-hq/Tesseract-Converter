app.newProject();
var c=app.project.items.addComp('endpoint-flags',64,64,1,2,24);
var result=[];
var modes=[[KeyframeInterpolationType.LINEAR,KeyframeInterpolationType.HOLD],[KeyframeInterpolationType.HOLD,KeyframeInterpolationType.HOLD],[KeyframeInterpolationType.HOLD,KeyframeInterpolationType.LINEAR]];
for(var i=0;i<modes.length;i++){
 var l=c.layers.addSolid([1,0,0],'control-'+i,64,64,1,2);
 var p=l.property('ADBE Transform Group').property('ADBE Opacity');
 p.setValueAtTime(-0.5,0);p.setValueAtTime(0.5,100);p.setValueAtTime(1.5,40);
 p.setInterpolationTypeAtKey(1,KeyframeInterpolationType.LINEAR,KeyframeInterpolationType.HOLD);
 p.setInterpolationTypeAtKey(2,modes[i][0],modes[i][1]);
 p.setInterpolationTypeAtKey(3,KeyframeInterpolationType.LINEAR,KeyframeInterpolationType.LINEAR);
}
app.project.save(new File(context.output_path));
app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES);app.open(new File(context.output_path));
c=app.project.item(1);
for(var i=1;i<=c.numLayers;i++){
 var p=c.layer(i).property('ADBE Transform Group').property('ADBE Opacity');var keys=[];
 for(var k=1;k<=p.numKeys;k++)keys.push({time:p.keyTime(k),value:p.keyValue(k),incoming:Number(p.keyInInterpolationType(k)),outgoing:Number(p.keyOutInterpolationType(k))});
 result.push({name:c.layer(i).name,keys:keys,before:p.valueAtTime(0.25,true),after:p.valueAtTime(0.75,true)});
}
return {result:{controls:result},targets:[]};
