app.newProject();
var targets=[],controls=[];
var variants=[{id:"mask_wide",origin:[-1024,-1024],size:[4096,3072]},{id:"mask_crop",origin:[0,0],size:[1920,1080]}];
var r=76,k=0.5522847498307936;
var vertices=[[76,0],[1844,0],[1920,76],[1920,1004],[1844,1080],[76,1080],[0,1004],[0,76]];
var ins=[[-k*r,0],[0,0],[0,-k*r],[0,0],[k*r,0],[0,0],[0,k*r],[0,0]];
var outs=[[0,0],[k*r,0],[0,0],[0,k*r],[0,0],[-k*r,0],[0,0],[0,-k*r]];
for(var v=0;v<variants.length;v++){
 var spec=variants[v],source=app.project.items.addComp("source_"+spec.id,spec.size[0],spec.size[1],1,1,30);
 var bg=source.layers.addSolid([0.08,0.18,0.32],"opaque boundary plane",spec.size[0],spec.size[1],1,1);
 for(var x=-1024;x<3072;x+=96){var stripe=source.layers.addSolid([0.85,0.2,0.1],"stripe"+x,24,spec.size[1],1,1);stripe.property("ADBE Transform Group").property("ADBE Position").setValue([x-spec.origin[0],spec.size[1]/2]);}
 var text=source.layers.addText("BOUNDARY CONTROL");
 var doc=text.property("ADBE Text Properties").property("ADBE Text Document").value;doc.font="ArialMT";doc.fontSize=160;doc.fillColor=[0.1,0.95,0.4];text.property("ADBE Text Properties").property("ADBE Text Document").setValue(doc);
 text.property("ADBE Transform Group").property("ADBE Position").setValue([-180-spec.origin[0],560-spec.origin[1]]);
 var output=app.project.items.addComp(spec.id,4096,3072,1,1,30),layer=output.layers.add(source);
 layer.property("ADBE Transform Group").property("ADBE Anchor Point").setValue([0,0]);
 layer.property("ADBE Transform Group").property("ADBE Position").setValue([spec.origin[0]+1024,spec.origin[1]+1024]);
 var mask=layer.property("ADBE Mask Parade").addProperty("ADBE Mask Atom");
 mask.maskMode=MaskMode.ADD;mask.inverted=false;
 var shape=new Shape(),nativeVertices=[];for(var i=0;i<vertices.length;i++)nativeVertices.push([vertices[i][0]-spec.origin[0],vertices[i][1]-spec.origin[1]]);shape.vertices=nativeVertices;
 shape.inTangents=ins;shape.outTangents=outs;shape.closed=true;
 mask.property("ADBE Mask Shape").setValue(shape);mask.property("ADBE Mask Feather").setValue([0,0]);mask.property("ADBE Mask Opacity").setValue(100);mask.property("ADBE Mask Offset").setValue(0);
 targets.push({id:spec.id,name:output.name,native_id:String(output.id)});
 controls.push({id:spec.id,origin:spec.origin,size:spec.size,mode:String(mask.maskMode),inverted:mask.inverted,vertices:mask.property("ADBE Mask Shape").value.vertices,feather:mask.property("ADBE Mask Feather").value,opacity:mask.property("ADBE Mask Opacity").value,expansion:mask.property("ADBE Mask Offset").value});
}
app.project.save(new File(context.output_path));
return {result:{feature:"inline-rounded-hard-add-source-crop",controls:controls,fps:30,duration_seconds:1},targets:targets};
