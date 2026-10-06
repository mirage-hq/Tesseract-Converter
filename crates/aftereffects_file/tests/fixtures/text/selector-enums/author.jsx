var c=app.project.items.addComp('W03 selector enums',320,180,1,1,30);
var l=c.layers.addText('AB CD'); var text=l.property('ADBE Text Properties');
var p=text.property('ADBE Text Document'); var d=p.value; d.font='ArialMT';d.fontSize=32;p.setValue(d);
var anim=text.property('ADBE Text Animators').addProperty('ADBE Text Animator');
anim.property('ADBE Text Animator Properties').addProperty('ADBE Text Position 3D').setValue([0,16,0]);
var s=anim.property('ADBE Text Selectors').addProperty('ADBE Text Selector');
var advanced=s.property('ADBE Text Range Advanced');
var names=['ADBE Text Range Units','ADBE Text Range Type2','ADBE Text Selector Mode','ADBE Text Range Shape'];
var values=[2,3,2,4];var before={},after={};
for(var i=0;i<names.length;i++){advanced.property(names[i]).setValue(values[i]);before[names[i]]=advanced.property(names[i]).value;}
advanced.property('ADBE Text Range Type2').setValue(4);
for(var i=0;i<names.length;i++){after[names[i]]=advanced.property(names[i]).value;}
app.project.save(new File(context.output_path));
return {result:{before:before,edited:after},targets:[{id:'main',name:c.name,native_id:String(c.id)}]};
