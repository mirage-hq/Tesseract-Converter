function ease(values){var out=[];for(var i=0;i<values.length;i++)out.push({speed:values[i].speed,influence:values[i].influence});return out;}
function read(asset,name){
 app.open(new File(context.assets[asset]));
 var comp=null;for(var i=1;i<=app.project.numItems;i++){var item=app.project.item(i);if(item instanceof CompItem && item.name===name){if(comp)throw new Error('ambiguous '+name);comp=item;}}
 if(!comp || comp.numLayers!==1)throw new Error('missing unique owner '+name);
 var parade=comp.layer(1).property('ADBE Effect Parade');if(!parade||parade.numProperties!==1)throw new Error('missing unique effect');
 var effect=parade.property(1);if(effect.matchName!=='ADBE Tint')throw new Error('not Tint');
 var p=effect.property('ADBE Tint-0001');if(!p||p.propertyValueType!==PropertyValueType.COLOR||p.numKeys!==2)throw new Error('COLOR keys absent');
 var keys=[],samples=[],times=[0,.125,.25,.5,.75,.875,1];
 for(var k=1;k<=p.numKeys;k++)keys.push({time:p.keyTime(k),value:p.keyValue(k),incoming:ease(p.keyInTemporalEase(k)),outgoing:ease(p.keyOutTemporalEase(k)),inInterpolation:String(p.keyInInterpolationType(k)),outInterpolation:String(p.keyOutInterpolationType(k))});
 for(var t=0;t<times.length;t++)samples.push({time:times[t],value:p.valueAtTime(times[t],true)});
 var result={compositionId:comp.id,compositionName:comp.name,keys:keys,samples:samples,white:effect.property('ADBE Tint-0002').value,amount:effect.property('ADBE Tint-0003').value,effectEnabled:effect.enabled};
 app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES);return result;
}
var before=read('original','aligned-color-cubic-original');
var after=read('edited','aligned-color-cubic-edited');
var reference=read('oracle','W09 aligned COLOR cubic native oracle');
return {result:{before:before,after:after,reference:reference},targets:[]};
