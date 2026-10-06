app.newProject();
var comp=app.project.items.addComp('W09 aligned COLOR cubic native oracle',32,32,1,1.25,24);
comp.layers.addSolid([0.5,0.5,0.5],'carrier',32,32,1,1.25).property('ADBE Effect Parade').addProperty('ADBE Tint');
function control(name){var p=comp.layer('carrier').property('ADBE Effect Parade').property(1).property(name);if(!p||p.matchName!==name)throw new Error('missing '+name);return p;}
var blackName='ADBE Tint-0001', whiteName='ADBE Tint-0002',amountName='ADBE Tint-0003';
var defaults={black:control(blackName).value,white:control(whiteName).value,amount:control(amountName).value};
if(control(blackName).propertyValueType!==PropertyValueType.COLOR)throw new Error('not COLOR');
var start=[.1,.2,.3,defaults.black[3]], end=[.8,.7,.6,defaults.black[3]];
control(whiteName).setValue([.52,1,.7,defaults.white[3]]);control(amountName).setValue(73);
function author(to){var p=control(blackName);p.setValueAtTime(0,start);p.setValueAtTime(1,to);var distance=255*Math.sqrt(Math.pow(to[0]-start[0],2)+Math.pow(to[1]-start[1],2)+Math.pow(to[2]-start[2],2));for(var k=1;k<=2;k++){p.setInterpolationTypeAtKey(k,KeyframeInterpolationType.BEZIER,KeyframeInterpolationType.BEZIER);p.setTemporalAutoBezierAtKey(k,false);p.setTemporalContinuousAtKey(k,false);p.setTemporalEaseAtKey(k,[new KeyframeEase(distance*.4,25)],[new KeyframeEase(distance*.4,25)]);}return distance;}
function ease(values){var out=[];for(var i=0;i<values.length;i++)out.push({speed:values[i].speed,influence:values[i].influence});return out;}
function read(){var p=control(blackName),keys=[],samples=[],times=[0,.125,.25,.5,.75,.875,1];for(var k=1;k<=p.numKeys;k++)keys.push({time:p.keyTime(k),value:p.keyValue(k),incoming:ease(p.keyInTemporalEase(k)),outgoing:ease(p.keyOutTemporalEase(k)),inInterpolation:String(p.keyInInterpolationType(k)),outInterpolation:String(p.keyOutInterpolationType(k))});for(var i=0;i<times.length;i++)samples.push({time:times[i],value:p.valueAtTime(times[i],true)});return {keys:keys,samples:samples,white:control(whiteName).value,amount:control(amountName).value};}
var beforeDistance=author(end),before=read();
var edited=[.9,.7,.6,defaults.black[3]],afterDistance=author(edited),after=read();
app.project.save(new File(context.output_path));
return {result:{defaults:defaults,beforeDistance:beforeDistance,afterDistance:afterDistance,before:before,after:after,bitsPerChannel:app.project.bitsPerChannel},targets:[{id:'color',name:comp.name,native_id:String(comp.id)}]};
