function check(ok, why) { if (!ok) throw new Error(why); }
function near(a,b) { return Math.abs(a-b)<0.0001; }
function compNamed(name) {
 var found=null; for(var i=1;i<=app.project.numItems;i++){var x=app.project.item(i);if(x instanceof CompItem && x.name===name){check(found===null,'duplicate comp');found=x;}}
 check(found!==null,'missing comp '+name);return found;
}
function inspect(comp, width, endX) {
 check(comp.numLayers===1,'one layer required');var layer=comp.layer(1);check(layer instanceof ShapeLayer,'shape required');
 var counts={paths:0,strokes:0,trims:0}, vertices=null, stroke=null;
 function walk(group){for(var i=1;i<=group.numProperties;i++){var p=group.property(i);
  if(p.matchName==='ADBE Vector Filter - Trim')counts.trims++;
  if(p.matchName==='ADBE Vector Shape'){counts.paths++;vertices=p.value.vertices;check(p.numKeys===0,'no geometry keys invented');}
  if(p.matchName==='ADBE Vector Stroke Width'){counts.strokes++;stroke=p.value;check(p.numKeys===0,'no stroke keys invented');}
  if(p.propertyType!==PropertyType.PROPERTY)walk(p);
 }}
 walk(layer.property('ADBE Root Vectors Group'));
 check(counts.paths===1 && counts.strokes===1 && counts.trims===0,'exact editable controls');
 check(vertices.length===3 && near(vertices[0][0],0) && near(vertices[1][0],100) && near(vertices[2][0],endX),'path edit readback');check(near(stroke,width),'stroke edit readback');
 return {counts:counts,vertices:vertices,width:stroke,position:layer.property('ADBE Transform Group').property('ADBE Position').value};
}
app.newProject();var controlComp=app.project.items.addComp('independent-untrimmed',320,180,1,2,30);var layer=controlComp.layers.addShape();layer.name='Independent untrimmed stroke';layer.property('ADBE Transform Group').property('ADBE Position').setValue([80,40]);
var group=layer.property('ADBE Root Vectors Group').addProperty('ADBE Vector Group');var shape=group.property('ADBE Vectors Group').addProperty('ADBE Vector Shape - Group');var contour=new Shape();contour.vertices=[[0,0],[100,0],[100,80]];contour.inTangents=[[0,0],[0,0],[0,0]];contour.outTangents=[[0,0],[0,0],[0,0]];contour.closed=true;shape.property('ADBE Vector Shape').setValue(contour);
var stroke=group.property('ADBE Vectors Group').addProperty('ADBE Vector Graphic - Stroke');stroke.property('ADBE Vector Stroke Color').setValue([0.2,0.4,0.6]);stroke.property('ADBE Vector Stroke Width').setValue(6);
var control=inspect(controlComp,6,100);app.project.save(new File(context.output_path));app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES);
app.open(new File(context.assets.control));var generated=inspect(compNamed('inert-trim-control'),6,100);app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES);
app.open(new File(context.assets.edited));var edited=inspect(compNamed('inert-trim-control'),12,140);
check(edited.width!==generated.width && edited.vertices[2][0]!==generated.vertices[2][0],'FX edits must affect generated output');
return {result:{independent:control,generated:generated,edited:edited,numericChecks:20},targets:[]};
