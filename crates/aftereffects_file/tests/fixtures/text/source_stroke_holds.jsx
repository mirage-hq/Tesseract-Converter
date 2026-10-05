var c = app.project.items.addComp('W03 source stroke holds',320,180,1,1,30);
function observe(d){
 var v={text:d.text,applyFill:d.applyFill,fillColor:d.fillColor,applyStroke:d.applyStroke,baselineShift:d.baselineShift,justification:Number(d.justification),boxText:d.boxText};
 if(d.applyStroke){v.strokeColor=d.strokeColor;v.strokeWidth=d.strokeWidth;v.strokeOverFill=d.strokeOverFill;}
 else {v.strokeControls='unavailable while disabled';}
 return v;
}
var before=[];
for(var i=0;i<2;i++){
 var l=i===0?c.layers.addText('AB\rCD'):c.layers.addBoxText([180,90],'AB\rCD');
 l.name=i===0?'Point source stroke':'Box source stroke';
 var p=l.property('ADBE Text Properties').property('ADBE Text Document');
 var d=p.value;d.font='ArialMT';d.fontSize=32;d.applyFill=true;d.fillColor=[.2,.4,.6];d.applyStroke=true;d.strokeColor=[.8,.3,.1];d.strokeWidth=3;d.strokeOverFill=true;d.baselineShift=7;d.justification=ParagraphJustification.RIGHT_JUSTIFY;
 p.setValueAtTime(0,d);
 d=p.keyValue(1);d.strokeColor=[.1,.7,.9];d.strokeWidth=7;p.setValueAtTime(.5,d);
 before.push({name:l.name,value:observe(p.keyValue(2))});
 d=p.keyValue(2);d.strokeWidth=9;p.setValueAtKey(2,d);
 d=p.keyValue(2);d.applyStroke=false;p.setValueAtTime(.75,d);
 for(var k=1;k<=3;k++)p.setInterpolationTypeAtKey(k,KeyframeInterpolationType.HOLD,KeyframeInterpolationType.HOLD);
}
function read(){
 var comp=null;for(var i=1;i<=app.project.numItems;i++){var item=app.project.item(i);if(item instanceof CompItem && item.name==='W03 source stroke holds')comp=item;}
 if(!comp)throw new Error('Missing source comp');
 var out=[];
 for(var i=1;i<=comp.numLayers;i++){
  var l=comp.layer(i),p=l.property('ADBE Text Properties').property('ADBE Text Document'),keys=[];
  for(var k=1;k<=p.numKeys;k++)keys.push({time:p.keyTime(k),value:observe(p.keyValue(k)),inHold:p.keyInInterpolationType(k)===KeyframeInterpolationType.HOLD,outHold:p.keyOutInterpolationType(k)===KeyframeInterpolationType.HOLD});
  if(keys.length!==3 || keys[0].value.strokeWidth!==3 || keys[1].value.strokeWidth!==9 || keys[2].value.applyStroke!==false)throw new Error('Stroke control mismatch');
  out.push({name:l.name,keys:keys});
 }
 return {observations:out,nativeId:String(comp.id)};
}
var opened=read();app.project.save(new File(context.output_path));
app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES);app.open(new File(context.output_path));
var reopened=read();
return {result:{beforeEdit:before,opened:opened,reopened:reopened},targets:[{id:'main',name:'W03 source stroke holds',native_id:reopened.nativeId}]};
