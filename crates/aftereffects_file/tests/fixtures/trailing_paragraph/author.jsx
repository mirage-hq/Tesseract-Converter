var comp = app.project.items.addComp('W03 trailing paragraph', 320, 180, 1, 1, 30);
var a = comp.layers.addText('A');
var b = comp.layers.addText('A\r');
var before = [a.property('ADBE Text Properties').property('ADBE Text Document').value.text,b.property('ADBE Text Properties').property('ADBE Text Document').value.text];
var p = b.property('ADBE Text Properties').property('ADBE Text Document');
var d = p.value; d.text='A\r\r'; p.setValue(d);
var edited = p.value.text;
app.project.save(new File(context.output_path));
return {result:{before:before,edited:edited},targets:[{id:'main',name:comp.name,native_id:String(comp.id)}]};
