var provider = app.project.items.addComp("Channel provider", 320, 180, 1, 2, 30);
var colors = [[0.8,0.2,0.1],[0.1,0.7,0.3],[0.2,0.4,0.9]];
var opacities = [100,50,0];
for (var i=0;i<3;i++) {
  var cell = provider.layers.addSolid(colors[i],"Cell " + i,106,180,1,2);
  cell.property("ADBE Transform Group").property("ADBE Position").setValue([53+106*i,90]);
  cell.property("ADBE Transform Group").property("ADBE Opacity").setValue(opacities[i]);
}
var comp = app.project.items.addComp("Set Matte red",320,180,1,2,30);
var sample = comp.layers.add(provider);
sample.enabled = false;
var targetSource = app.project.items.addComp("Target source",320,180,1,2,30);
targetSource.layers.addSolid([0.3,0.6,0.8],"Target paint",320,180,1,2);
var target = comp.layers.add(targetSource);
var effect = target.property("ADBE Effect Parade").addProperty("ADBE Set Matte3");
effect.property("ADBE Set Matte3-0001").setValue(sample.index);
effect.property("ADBE Set Matte3-0002").setValue(1);
var controls=[];
for(var p=1;p<=effect.numProperties;p++){
  var prop=effect.property(p);
  if(prop.propertyType===PropertyType.PROPERTY){
    controls.push({name:prop.name,matchName:prop.matchName,value:prop.value});
  }
}
app.project.save(new File(context.output_path));
return {result:{compositionId:comp.id,providerCompositionId:provider.id,targetId:target.id,providerId:sample.id,controls:controls,colors:colors,opacities:opacities},targets:[{id:"main",name:comp.name,native_id:String(comp.id)}]};
