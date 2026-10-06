app.newProject();
var comp=app.project.items.addComp('W09 P009 Box Vertical Alignment',1280,720,1,1,30);
if(typeof BoxVerticalAlignment==='undefined')throw new Error('native BoxVerticalAlignment enum unavailable');
var labels=['TOP','CENTER','BOTTOM'],values=[BoxVerticalAlignment.TOP,BoxVerticalAlignment.CENTER,BoxVerticalAlignment.BOTTOM];
for(var i=0;i<3;i++){
 if(typeof values[i]==='undefined')throw new Error('missing nativeenum '+labels[i]);
 var l=comp.layers.addBoxText([360,400]);l.name='Box '+labels[i];
 var p=l.property('ADBE Text Properties').property('ADBE Text Document');var d=p.value;
 if(typeof d.boxVerticalAlignment==='undefined')throw new Error('native BoxVerticalAlignment control unavailable');
 d.text='Vertical alignment';d.font='ArialMT';d.fontSize=48;d.applyFill=true;d.fillColor=[1,1,1];d.applyStroke=false;d.autoLeading=false;d.leading=52;d.justification=ParagraphJustification.LEFT_JUSTIFY;d.boxVerticalAlignment=values[i];p.setValue(d);
 l.property('ADBE Transform Group').property('ADBE Position').setValue([220+i*400,360]);
}
app.project.save(new File(context.output_path));app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES);app.open(new File(context.output_path));
var found=null;for(var i=1;i<=app.project.numItems;i++)if(app.project.item(i) instanceof CompItem && app.project.item(i).name==='W09 P009 Box Vertical Alignment'){if(found)throw new Error('ambiguous comp');found=app.project.item(i);}
if(!found||found.numLayers!==3)throw new Error('wrong authored comp');comp=found;
function read(name){
 var l=comp.layer(name);if(!l)throw new Error('missing exact layer '+name);
 var d=l.property('ADBE Text Properties').property('ADBE Text Document').value,r=l.sourceRectAtTime(0.5,false);
 if(!d.boxText||d.font!=='ArialMT'||d.text!=='Vertical alignment')throw new Error('wrong document '+name);
 return {layer:name,layerIndex:l.index,text:d.text,font:d.font,fontSize:d.fontSize,boxText:d.boxText,boxSize:d.boxTextSize,boxPosition:d.boxTextPos,verticalAlignment:Number(d.boxVerticalAlignment),justification:Number(d.justification),leading:d.leading,autoLeading:d.autoLeading,sourceRect:{left:r.left,top:r.top,width:r.width,height:r.height}};
}
var before=[];for(var i=0;i<3;i++){var r=read('Box '+labels[i]);if(r.verticalAlignment!==Number(values[i]))throw new Error('nativeverticalreadbackmismatch');before.push(r);}
var center=comp.layer('Box CENTER').property('ADBE Text Properties').property('ADBE Text Document'),edited=center.value;edited.boxVerticalAlignment=BoxVerticalAlignment.BOTTOM;center.setValue(edited);var after=read('Box CENTER');
if(after.verticalAlignment!==Number(BoxVerticalAlignment.BOTTOM))throw new Error('nativeeditcontrolmismatch');
return {result:{applicationVersion:app.version,enumValues:{top:Number(BoxVerticalAlignment.TOP),center:Number(BoxVerticalAlignment.CENTER),bottom:Number(BoxVerticalAlignment.BOTTOM)},before:before,editedCenterToBottom:after,editChangedRect:before[1].sourceRect.top!==after.sourceRect.top},targets:[{id:'vertical-alignment',name:comp.name,native_id:String(comp.id)}]};
