function paths(group,out){
 for(var i=1;i<=group.numProperties;i++){
  var p=group.property(i);
  if(p.matchName==='ADBE Vector Shape'){
   var s=p.value;out.push({vertices:s.vertices,inTangents:s.inTangents,outTangents:s.outTangents,closed:s.closed});
  }else if(p.numProperties){paths(p,out);}
 }
}
var names=['source','exported','edited'];var result=[];
for(var j=0;j<names.length;j++){
 app.open(new File(context.assets[names[j]]));
 var out=[];for(var i=1;i<=app.project.numItems;i++){var item=app.project.item(i);if(item instanceof CompItem){for(var l=1;l<=item.numLayers;l++){var g=item.layer(l).property('ADBE Root Vectors Group');if(g){paths(g,out);}}}}
 if(out.length===0){throw new Error('No native vector paths in '+names[j]);}
 for(var k=0;k<out.length;k++){
  var s=out[k];var expectedX=names[j]==='edited'?40:0;
  if(!s.closed||s.vertices.length!==1||Math.abs(s.vertices[0][0]-expectedX)>0.001||Math.abs(s.vertices[0][1])>0.001){throw new Error('Closed one-vertex geometry mismatch in '+names[j]);}
  if(Math.abs(s.inTangents[0][0]-90)>0.001||Math.abs(s.inTangents[0][1]+100)>0.001||Math.abs(s.outTangents[0][0]+90)>0.001||Math.abs(s.outTangents[0][1]+100)>0.001){throw new Error('Bezier tangent mismatch in '+names[j]);}
 }
 result.push({name:names[j],paths:out});app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES);
}
return {result:{controls:result},targets:[]};
