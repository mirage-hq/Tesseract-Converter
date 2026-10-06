var result={errors:[],versions:[]};
function comp(name){var found=null,n=0;for(var i=1;i<=app.project.numItems;i++){var x=app.project.item(i);if(x instanceof CompItem&&x.name===name){found=x;n++;}}if(n!==1)throw Error('identity '+name+' '+n);return found;}
function layer(l){return {name:l.name,start:l.startTime,inPoint:l.inPoint,outPoint:l.outPoint,stretch:l.stretch,quality:Number(l.quality),sampling:Number(l.samplingQuality),frameBlending:l.frameBlending,motionBlur:l.motionBlur,collapse:l.collapseTransformation,anchor:l.transform.anchorPoint.value,position:l.transform.position.value,scale:l.transform.scale.value};}
try{for(var v=0;v<2;v++){
 app.open(new File(context.assets[v===0?'independent':'generated']));
 for(var i=1;i<=app.project.numItems;i++){var item=app.project.item(i);if(item instanceof FootageItem&&item.file)item.replace(new File(context.assets.clip));}
 var root=comp(v===0?'P019 public dual-clock occurrence':'Comp 1'),plane=comp(v===0?'P019 public source clock plane':'video-8990'),outer=root.layer(v===0?'Source-clock occurrence':'video-8990'),inner=plane.layer(v===0?'Timecoded source-owned controls':'video-8990 source controls');
 var row={version:v,workingSpace:app.project.workingSpace,bits:app.project.bitsPerChannel,rootRate:root.frameRate,sourceRate:plane.frameRate,preserveNestedRate:plane.preserveNestedFrameRate,rootMotionBlur:root.motionBlur,sourceMotionBlur:plane.motionBlur,outer:layer(outer),inner:layer(inner),effects:[],samples:[]};
 var fx=inner.property('ADBE Effect Parade');for(var e=1;e<=fx.numProperties;e++){var f=fx.property(e);for(var k=1;k<=f.numProperties;k++){var p=f.property(k);if(p.propertyType===PropertyType.PROPERTY&&p.propertyValueType===PropertyValueType.OneD){var a=[];for(var q=0;q<context.data.times.length;q++)a.push(p.valueAtTime(context.data.times[q],true));row.effects.push({name:p.matchName,values:a});}}}
 var probe=root.layers.addNull();probe.property('ADBE Effect Parade').addProperty('ADBE Slider Control');var slider=probe.property('ADBE Effect Parade').property('ADBE Slider Control').property('ADBE Slider Control-0001');slider.expression=v===0?'thisComp.layer("Source-clock occurrence").sourceTime(time)':'thisComp.layer("video-8990").sourceTime(time)';
 for(var q=0;q<context.data.parentTimes.length;q++){var t=context.data.parentTimes[q];row.samples.push({time:t,sourceTime:slider.valueAtTime(t,false),error:slider.expressionError});}
 probe.remove();result.versions.push(row);app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES);
 }}catch(e){result.errors.push(String(e));}return {result:result,targets:[]};
