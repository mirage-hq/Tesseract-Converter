app.newProject();
var comp=app.project.items.addComp('W06 Animated Bezier Direction',320,240,1,2,24);
var layer=comp.layers.addShape();layer.name='Reversed keyed Bezier';
layer.property('ADBE Transform Group').property('ADBE Position').setValue([160,120]);
var contents=layer.property('ADBE Root Vectors Group');
var shapeGroup=contents.addProperty('ADBE Vector Shape - Group');
shapeGroup.property('ADBE Vector Shape Direction').setValue(3);
var p=shapeGroup.property('ADBE Vector Shape');
for(var i=0;i<2;i++) {var s=new Shape();s.vertices=[[-80,-40+i*20],[70,-20],[20,70]];s.inTangents=[[15,-20],[-25,-15],[20,10]];s.outTangents=[[30,10],[10,30],[-25,-15]];s.closed=true;p.setValueAtTime(i,s);p.setInterpolationTypeAtKey(i+1,KeyframeInterpolationType.LINEAR,KeyframeInterpolationType.LINEAR);}
var stroke=contents.addProperty('ADBE Vector Graphic - Stroke');stroke.property('ADBE Vector Stroke Color').setValue([0,0.8,1]);stroke.property('ADBE Vector Stroke Width').setValue(5);
app.project.save(new File(context.output_path));app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES);app.open(new File(context.output_path));
comp=app.project.item(1);shapeGroup=comp.layer(1).property('ADBE Root Vectors Group').property(1);p=shapeGroup.property('ADBE Vector Shape');
var keys=[];for(var k=1;k<=p.numKeys;k++){var v=p.keyValue(k);keys.push({time:p.keyTime(k),vertices:v.vertices,inTangents:v.inTangents,outTangents:v.outTangents,closed:v.closed});}
return {result:{direction:shapeGroup.property('ADBE Vector Shape Direction').value,keys:keys,composition_id:comp.id},targets:[{id:'main',name:comp.name,native_id:String(comp.id)}]};
