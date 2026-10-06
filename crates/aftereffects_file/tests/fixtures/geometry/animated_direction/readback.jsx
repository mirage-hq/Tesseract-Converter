function paths(group,out){
 for(var i=1;i<=group.numProperties;i++){
  var p=group.property(i);
  if(p.matchName==='ADBE Vector Shape'){
   var keys=[];for(var k=1;k<=p.numKeys;k++){var s=p.keyValue(k);keys.push({time:p.keyTime(k),vertices:s.vertices,inTangents:s.inTangents,outTangents:s.outTangents,closed:s.closed});}out.push(keys);
  }else if(p.numProperties){paths(p,out);}
 }
}
var names=['exported','edited'];var result=[];
for(var j=0;j<names.length;j++){
 app.open(new File(context.assets[names[j]]));
 var out=[];for(var i=1;i<=app.project.numItems;i++){var item=app.project.item(i);if(item instanceof CompItem){for(var l=1;l<=item.numLayers;l++){var g=item.layer(l).property('ADBE Root Vectors Group');if(g){paths(g,out);}}}}
 var checked=0;var dx=j===1?30:0;
 for(var n=0;n<out.length;n++){var keys=out[n];if(keys.length===0)continue;if(keys.length!==2)throw new Error('Wrong key count');
  for(var k=0;k<2;k++){var s=keys[k];if(!s.closed||s.vertices.length!==3||Math.abs(s.time-k)>0.001)throw new Error('Key topology/time mismatch');
   var expected=[[-80+dx,-40+k*20],[20+dx,70],[70+dx,-20]];
   for(var v=0;v<3;v++)for(var a=0;a<2;a++){if(Math.abs(s.vertices[v][a]-expected[v][a])>0.001)throw new Error('Reversed vertex order mismatch');}
   if(Math.abs(s.outTangents[0][0]-15)>0.001||Math.abs(s.outTangents[0][1]+20)>0.001||Math.abs(s.inTangents[1][0]+25)>0.001||Math.abs(s.inTangents[1][1]+15)>0.001)throw new Error('Reversed cubic handle mismatch');
   checked++;
  }
 }
 if(checked<2)throw new Error('No keyed native paths');result.push({name:names[j],paths:out,checked:checked});app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES);
}
return {result:{controls:result},targets:[]};
