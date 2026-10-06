var comp=app.project.items.addComp("Linked Alpha source",320,180,1,3,30);
comp.bgColor=[0.4,0.1,0.6];
var image=app.project.importFile(new ImportOptions(new File(context.assets.cells)));
comp.layers.add(image);
var sound=app.project.importFile(new ImportOptions(new File(context.assets.video)));
var audio=comp.layers.add(sound);audio.enabled=false;audio.audioEnabled=true;
var id=comp.id;
app.project.save(new File(context.output_path));app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES);app.open(new File(context.output_path));
var saved=null;for(var i=1;i<=app.project.numItems;i++){if(app.project.item(i).id===id)saved=app.project.item(i);}
if(!saved||saved.numLayers!==2||saved.layer(1).enabled!==false||saved.layer(1).audioEnabled!==true)throw new Error("saved source identity/audio mismatch");
return {result:{composition:id,width:320,height:180,duration:3,fps:30,background:saved.bgColor,pictureLayer:saved.layer(2).id,audioLayer:saved.layer(1).id},targets:[{id:"source",name:"Linked Alpha source",native_id:String(id)}]};
