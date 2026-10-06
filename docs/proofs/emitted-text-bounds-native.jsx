function require(ok,msg){if(!ok)throw new Error(msg);}
function numeric(x){require(typeof x==='number'&&isFinite(x),'Nonfinite numeric result');return x;}
var reports=[];
for(var n=0;n<context.data.projects.length;n++){
 var spec=context.data.projects[n];if(app.project)app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES);app.open(new File(context.assets[spec.asset]));
 for(var i=1;i<=app.project.numItems;i++){
  var item=app.project.item(i);
  if(item instanceof FootageItem&&item.file){var key=spec.media[item.file.fsName];require(key!==undefined,'Unpinned footage');item.replace(new File(context.assets[key]));require(!item.footageMissing,'Offline footage');}
 }
 var target=null,cameraCount=0;
 for(var i=1;i<=app.project.numItems;i++){
  var item=app.project.item(i);if(!(item instanceof CompItem))continue;
  if(item.name===spec.comp&&item.id===spec.compId){require(target===null,'Ambiguous pinned target');target=item;}
  for(var j=1;j<=item.numLayers;j++)if(item.layer(j).name===spec.camera){require(item.layer(j).source instanceof CompItem,'Camera source flattened');require(item.layer(j).motionBlur===true,'Camera motion blur lost');cameraCount++;}
 }
 require(target!==null&&target.numLayers===spec.layers,'Missing source or editable layers');
 if(spec.camera)require(cameraCount===1,'Missing editable camera occurrence');
 var text=null,space=null;
 for(var i=1;i<=target.numLayers;i++){var layer=target.layer(i);if(layer.name===spec.label){require(text===null,'Ambiguous Text');text=layer;}if(layer.name==='Editable space')space=layer;}
 require(text!==null&&text.property('ADBE Text Properties')!==null,'Missing native Text');
 var prop=text.property('ADBE Text Properties').property('ADBE Text Document');
 require(prop.propertyValueType!==PropertyValueType.CUSTOM_VALUE,'Unexpected opaque Text');
 var d=prop.value;require(d.text==='Mirage'&&d.tracking===spec.tracking&&d.fontSize===spec.fontSize&&!d.boxText&&d.applyFill&&!d.applyStroke,'Source Text base/profile changed');
 var rect=text.sourceRectAtTime(0,false);require(rect.width>0&&rect.height>0,'Blank glyphs');
 var transform=text.property('ADBE Transform Group');var p=transform.property('ADBE Position').value;var a=transform.property('ADBE Anchor Point').value;
 var left=numeric(p[0]-a[0]+rect.left),top=numeric(p[1]-a[1]+rect.top);
 require(left>=-0.02&&top>=-0.02&&left+rect.width<=target.width+0.02&&top+rect.height<=target.height+0.02,'Initial native glyphs escape source enclosure');
 var whitespace=null;
 if(spec.minimal){require(space!==null&&space.property('ADBE Text Properties')!==null,'Whitespace flattened/dropped');var sp=space.property('ADBE Text Properties').property('ADBE Text Document');require(sp.value.text===' ','Whitespace content lost');var sr=space.sourceRectAtTime(0,false);require(sr.width===0&&sr.height===0,'Whitespace paints');whitespace={width:sr.width,height:sr.height};}
 reports.push({caseId:spec.id,width:target.width,height:target.height,text:d.text,font:d.font,fontSize:d.fontSize,tracking:d.tracking,sourceTextKeys:prop.numKeys,glyph:{left:rect.left,top:rect.top,width:rect.width,height:rect.height},enclosure:{left:left,top:top},cameraOccurrences:cameraCount,whitespace:whitespace});
}
require(reports.length===5,'Missing project profile');
var before=reports[3],after=reports[4];
require(Math.abs((after.glyph.width-before.glyph.width)-57.6)<0.02,'Edited FX Tracking width delta wrong');require(Math.abs(after.glyph.height-before.glyph.height)<0.001,'Tracking changed height');require(after.width>before.width,'Edited bounds not recomputed');
return {result:{projects:reports,editableTrackingResponse:true,whitespaceRetained:true},targets:[]};
