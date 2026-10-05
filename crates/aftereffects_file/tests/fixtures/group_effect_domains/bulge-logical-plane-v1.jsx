app.newProject();
var targets=[],controls=[];
var variants=[{id:"logical_wide",origin:[-2048,-1536],size:[6144,4096]},{id:"logical_crop",origin:[0,0],size:[1920,1080]}];
for(var v=0;v<variants.length;v++){
 var spec=variants[v],source=app.project.items.addComp("source_"+spec.id,spec.size[0],spec.size[1],1,1,30);
 source.layers.addSolid([0.06,0.09,0.15],"background",spec.size[0],spec.size[1],1,1);
 for(var x=-2048;x<4096;x+=96){var stripe=source.layers.addSolid([(x+2048)%192===0?0.9:0.1,0.45,0.2],"vertical_"+x,24,spec.size[1],1,1);stripe.property("ADBE Transform Group").property("ADBE Position").setValue([x-spec.origin[0],spec.size[1]/2]);}
 for(var y=-1536;y<2560;y+=96){var row=source.layers.addSolid([0.1,0.75,0.85],"horizontal_"+y,spec.size[0],12,1,1);row.property("ADBE Transform Group").property("ADBE Position").setValue([spec.size[0]/2,y-spec.origin[1]]);}
 var output=app.project.items.addComp(spec.id,1920,1080,1,1,30),layer=output.layers.add(source);
 layer.property("ADBE Transform Group").property("ADBE Anchor Point").setValue([0,0]);layer.property("ADBE Transform Group").property("ADBE Position").setValue([spec.origin[0]+400,spec.origin[1]+267]);
 var mask=layer.property("ADBE Mask Parade").addProperty("ADBE Mask Atom");mask.maskMode=MaskMode.ADD;mask.inverted=false;
 var shape=new Shape();shape.vertices=[[-spec.origin[0],-spec.origin[1]],[1920-spec.origin[0],-spec.origin[1]],[1920-spec.origin[0],1080-spec.origin[1]],[-spec.origin[0],1080-spec.origin[1]]];shape.inTangents=[[0,0],[0,0],[0,0],[0,0]];shape.outTangents=[[0,0],[0,0],[0,0],[0,0]];shape.closed=true;mask.property("ADBE Mask Shape").setValue(shape);mask.property("ADBE Mask Feather").setValue([0,0]);
 var fx=layer.property("ADBE Effect Parade").addProperty("ADBE Bulge");fx.property("ADBE Bulge-0001").setValue(1632);fx.property("ADBE Bulge-0002").setValue(1026);fx.property("ADBE Bulge-0003").setValue([960-spec.origin[0],518.4-spec.origin[1]]);fx.property("ADBE Bulge-0004").setValue(0.08);fx.property("ADBE Bulge-0007").setValue(false);
 controls.push({id:spec.id,source_origin:spec.origin,source_size:spec.size,center:fx.property("ADBE Bulge-0003").value,horizontal_radius:fx.property("ADBE Bulge-0001").value,vertical_radius:fx.property("ADBE Bulge-0002").value,height:fx.property("ADBE Bulge-0004").value,pinning:fx.property("ADBE Bulge-0007").value});targets.push({id:spec.id,name:output.name,native_id:String(output.id)});
}
app.project.save(new File(context.output_path));return {result:{feature:"logical-root-plane-bulge-input-crop",controls:controls,logical_input:[1920,1080]},targets:targets};
