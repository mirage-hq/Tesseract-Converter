// Reverse one native source contour, preserving closed-path start and handles.
var points=nativePath.commands.slice(), closed=points.length && points[points.length-1].type==='close';
if(closed) points.pop();
if(!points.length) return {commands:[]};
function anchor(c,type) {
    var a={type:type,x:c.x,y:c.y};
    if(c.mirror!==undefined)a.mirror=c.mirror;
    if(c.cornerRadius!==undefined)a.cornerRadius=c.cornerRadius;
    return a;
}
var first=points[0], last=points[points.length-1], segments=[];
for(var i=1;i<points.length;i++)segments.push([points[i-1],points[i]]);
if(closed && (last.x!==first.x || last.y!==first.y))segments.push([last,anchor(first,'lineTo')]);
var result=[anchor(closed?first:last,'moveTo')];
for(var j=segments.length-1;j>=0;j--) {
    var from=segments[j][0], to=segments[j][1], out=anchor(from,to.type==='cubicTo'?'cubicTo':'lineTo');
    if(to.type==='cubicTo') {
        out.c1x=to.c2x;out.c1y=to.c2y;out.c2x=to.c1x;out.c2y=to.c1y;
    }
    result.push(out);
}
if(closed) {
    var end=result[result.length-1];
    if(end.type==='lineTo' && end.x===first.x && end.y===first.y)result.pop();
    result.push({type:'close'});
}
return {commands:result};
