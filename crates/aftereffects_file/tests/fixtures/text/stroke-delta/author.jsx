var c = app.project.items.addComp('W03 animator units',320,180,1,1,30);
var l=c.layers.addText('AB\rCD');
var text=l.property('ADBE Text Properties');
var p=text.property('ADBE Text Document'); var d=p.value;
d.font='ArialMT';d.fontSize=32;d.baselineShift=7;d.tracking=125;d.justification=ParagraphJustification.RIGHT_JUSTIFY;p.setValue(d);
var anim=text.property('ADBE Text Animators').addProperty('ADBE Text Animator');
var group=anim.property('ADBE Text Animator Properties');
var names=['ADBE Text Stroke Width'];
var vals=[-4];
for(var i=0;i<names.length;i++){group.addProperty(names[i]).setValue(vals[i]);}
var before={};for(var i=0;i<names.length;i++){before[names[i]]=group.property(names[i]).value;}
var stroke=group.property('ADBE Text Stroke Width');
var bounds={hasMin:stroke.hasMin,hasMax:stroke.hasMax,min:stroke.minValue,max:stroke.maxValue};
stroke.setValue(-2);
var edited=stroke.value;
app.project.save(new File(context.output_path));
return {result:{before:before,edited:edited,bounds:bounds,sourceText:{baselineShift:p.value.baselineShift,tracking:p.value.tracking,justification:Number(p.value.justification)}},targets:[{id:'main',name:c.name,native_id:String(c.id)}]};
