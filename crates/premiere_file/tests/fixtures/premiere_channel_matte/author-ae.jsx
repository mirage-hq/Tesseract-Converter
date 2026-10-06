var footage = app.project.importFile(new ImportOptions(new File(context.assets.video)));
var provider=app.project.items.addComp("Channel provider",320,180,1,3,30);
var colors=[[0.8,0.2,0.1],[0.1,0.7,0.3],[0.2,0.4,0.9]], alphas=[100,50,0];
for(var i=0;i<3;i++){var cell=provider.layers.addSolid(colors[i],"Cell "+i,106,180,1,3);cell.property("ADBE Transform Group").property("ADBE Position").setValue([53+106*i,90]);cell.property("ADBE Transform Group").property("ADBE Opacity").setValue(alphas[i]);}
var targetSource=app.project.items.addComp("Video source",320,180,1,3,30);targetSource.layers.add(footage);
var rows=[],targets=[];
for(var channel=2;channel<=3;channel++){
 var name=channel===2?"Set Matte Green":"Set Matte Blue";
 var comp=app.project.items.addComp(name,320,180,1,3,30);
 var sample=comp.layers.add(provider);sample.enabled=false;
 var target=comp.layers.add(targetSource);
 var fx=target.property("ADBE Effect Parade").addProperty("ADBE Set Matte3");
 fx.property("ADBE Set Matte3-0001").setValue(sample.index);
 fx.property("ADBE Set Matte3-0002").setValue(channel);
 rows.push({composition:comp.id,name:name,channel:channel,target:target.id,provider:sample.id});
 targets.push({id:channel===2?"green":"blue",name:name,native_id:String(comp.id)});
}
app.project.save(new File(context.output_path));app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES);app.open(new File(context.output_path));
for(var j=0;j<rows.length;j++){var found=null;for(var n=1;n<=app.project.numItems;n++){if(app.project.item(n).id===rows[j].composition)found=app.project.item(n);}if(!found||found.layer(1).property("ADBE Effect Parade").property(1).property("ADBE Set Matte3-0002").value!==rows[j].channel)throw new Error("saved channel mismatch");}
return {result:{rows:rows,colors:colors,alphas:alphas},targets:targets};
