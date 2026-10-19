var width=160,height=90,duration=1,fps=24;
function composition(name,color){var c=app.project.items.addComp(name,width,height,1,duration,fps);c.bgColor=color;return c;}
function rectangle(c,name,color,w,h){var l=c.layers.addSolid(color,name,w,h,1,duration);l.property('ADBE Transform Group').property('ADBE Position').setValue([80,45]);return l;}
var transparent=composition('CanvasTransparent',[0.2,0.4,0.6]);
rectangle(transparent,'Authored foreground',[1,0.2,0.1],40,20);
var opaque=composition('CanvasAuthoredBackground',[0.8,0.1,0.3]);
rectangle(opaque,'Authored background',[0.1,0.3,0.5],width,height);
rectangle(opaque,'Authored foreground',[1,0.2,0.1],40,20);
var nested=composition('CanvasNestedTransparent',[0.7,0.6,0.2]);
var child=nested.layers.add(transparent);child.name='Authored precomposition';child.property('ADBE Transform Group').property('ADBE Position').setValue([80,45]);
var compositions=[transparent,opaque,nested],results=[],targets=[];
for(var i=0;i<compositions.length;i++){
var c=compositions[i],layers=[];
for(var j=1;j<=c.numLayers;j++){
var l=c.layer(j),source=l.source;
layers.push({name:l.name,id:l.id,index:l.index,source_id:source.id,width:source.width,height:source.height,position:l.property('ADBE Transform Group').property('ADBE Position').value,color:source.mainSource instanceof SolidSource?source.mainSource.color:null});
}
results.push({name:c.name,id:c.id,width:c.width,height:c.height,duration:c.duration,fps:c.frameRate,preview_background:c.bgColor,layers:layers});
targets.push({id:['transparent','authored_background','nested'][i],name:c.name,native_id:String(c.id)});
}
app.project.save(new File(context.output_path));
return {result:{application_version:app.version,feature:'preview_background_is_not_render_content',compositions:results},targets:targets};
