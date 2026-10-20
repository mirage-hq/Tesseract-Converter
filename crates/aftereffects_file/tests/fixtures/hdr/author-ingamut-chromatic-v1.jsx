if (!app.project || app.project.numItems !== 0) throw new Error('worker project not empty');
if (context.data.profile_kind === 'owned') {
    app.open(new File(context.assets.profile_seed));
} else if (context.data.profile_kind === 'builtin') {
    app.newProject();
    var profiles = app.project.listColorProfiles(), matches = 0;
    for (var i = 0; i < profiles.length; i++) if (profiles[i] === context.data.profile_name) matches++;
    if (matches !== 1) throw new Error('builtin profile unavailable/ambiguous');
    app.project.workingSpace = context.data.profile_name;
    app.project.bitsPerChannel = 32;
    app.project.linearizeWorkingSpace = false;
    app.project.linearBlending = false;
} else throw new Error('unknown profile kind');
if (app.project.numItems > 64) throw new Error('unexpected seed inventory');
for (var i = app.project.numItems; i >= 1; i--) if (app.project.item(i) instanceof CompItem) app.project.item(i).remove();
while (app.project.numItems > 0) app.project.item(app.project.numItems).remove();
var colors=context.data.working_rgb;
if (colors.length !== 10 || context.data.paint_names.length !== 10) throw new Error('declared ten patches required');
var comp=app.project.items.addComp(context.data.case_name,1280,512,1,1,30);
for(var n=0;n<10;n++) {
 var layer=comp.layers.addSolid(colors[n],context.data.paint_names[n],256,256,1,1);
 layer.property('ADBE Transform Group').property('ADBE Position').setValue([128+256*(n%5),128+256*Math.floor(n/5)]);
}
function observe() {
 var chosen=null,comps=0,solids=0;
 for(var n=1;n<=app.project.numItems;n++) {
  var item=app.project.item(n);
  if(item instanceof CompItem){comps++;chosen=item;}
  else if(item instanceof FootageItem){if(!(item.mainSource instanceof SolidSource))throw new Error('non-solid');solids++;}
  else if(!(item instanceof FolderItem))throw new Error('unexpected item');
 }
 if(comps!==1||solids!==10||chosen.numLayers!==10||chosen.name!==context.data.case_name)throw new Error('inventory');
 if(chosen.width!==1280||chosen.height!==512||chosen.frameRate!==30||chosen.duration!==1)throw new Error('clock/canvas');
 if(app.project.bitsPerChannel!==32||app.project.workingSpace!==context.data.profile_name||app.project.linearizeWorkingSpace!==false||app.project.linearBlending!==false)throw new Error('domain');
 var seen={},paints=[];
 for(var n=1;n<=chosen.numLayers;n++) {
  var layer=chosen.layer(n),name=String(layer.name),index=-1;
  for(var i=0;i<10;i++)if(context.data.paint_names[i]===name)index=i;
  if(index<0||seen[name])throw new Error('patch identity');seen[name]=true;
  var rgb=layer.source.mainSource.color;
  for(var c=0;c<3;c++)if(Math.abs(rgb[c]-colors[index][c])>0.0000001)throw new Error('paint color');
  var transform=layer.property('ADBE Transform Group'),pos=transform.property('ADBE Position').value;
  if(pos[0]!==128+256*(index%5)||pos[1]!==128+256*Math.floor(index/5))throw new Error('patch position');
  if(layer.source.width!==256||layer.source.height!==256||layer.inPoint!==0||layer.outPoint!==1||transform.property('ADBE Opacity').value!==100)throw new Error('coverage');
  paints[index]={name:name,rgb:rgb,position:pos,width:256,height:256};
 }
 var hex=Number(chosen.id).toString(16);while(hex.length<8)hex='0'+hex;
 return {composition_id:String(chosen.id),dynamic_link_guid:hex+'-0000-0000-0000-000000000000',name:String(chosen.name),width:1280,height:512,fps:30,duration:1,bpc:32,working_space:app.project.workingSpace,linearize:false,linear_blending:false,editable_solids:10,paints:paints};
}
var authored=observe();app.project.save(new File(context.output_path));app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES);app.open(new File(context.output_path));var reopened=observe();
return {result:{case_id:context.data.case_id,profile_kind:context.data.profile_kind,authored:authored,reopened:reopened,sampling_declaration_sha256:context.data.sampling_declaration_sha256,geometry_color_native_authored:true,alpha_fidelity:'UNVERIFIED opaque only',render_fidelity:'UNMEASURED'},targets:[{id:context.data.case_id,name:context.data.case_name,native_id:reopened.composition_id}]};
