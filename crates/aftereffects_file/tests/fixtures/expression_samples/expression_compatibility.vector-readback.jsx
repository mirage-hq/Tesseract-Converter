/* Managed readback of a pinned private source copy; no save or render. */
var project=app.open(new File(context.assets.project));
var comp=null;
for(var i=1;i<=project.numItems;i++) if(project.item(i).id===1) comp=project.item(i);
if(!comp || !(comp instanceof CompItem)) throw new Error('Pinned composition1 missing');
var subject=comp.layer('Subject');
var transform=subject.property('ADBE Transform Group');
var controls={};
var names=['ADBE Anchor Point','ADBE Position','ADBE Scale','ADBE Orientation','ADBE Rotate X','ADBE Rotate Y','ADBE Rotate Z'];
for(var i=0;i<names.length;i++) controls[names[i]]=transform.property(names[i]).valueAtTime(0,true);
var probe=comp.layers.addNull(1); probe.threeDLayer=true; probe.name='Private vector readback';
var position=probe.property('ADBE Transform Group').property('ADBE Position');
var expressions={vector:"thisComp.layer('Subject').toWorldVec([1,2,3])",point:"thisComp.layer('Subject').toWorld([1,2,3])",origin:"thisComp.layer('Subject').toWorld([0,0,0])",difference:"var l=thisComp.layer('Subject'); l.toWorld([1,2,3])-l.toWorld([0,0,0])",basisX:"thisComp.layer('Subject').toWorldVec([1,0,0])",basisY:"thisComp.layer('Subject').toWorldVec([0,1,0])",basisZ:"thisComp.layer('Subject').toWorldVec([0,0,1])"};
var values={};
for(var key in expressions) if(expressions.hasOwnProperty(key)) {
 position.expression=expressions[key]; position.expressionEnabled=true;
 var value=position.valueAtTime(0,false);
 if(position.expressionError) throw new Error(key+': '+position.expressionError);
 values[key]=value;
}
return {result:{application_version:app.version,composition_id:1,controls:controls,values:values},targets:[]};
