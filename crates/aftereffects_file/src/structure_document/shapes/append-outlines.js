// Append contours; union would change winding and remove stroke boundaries.
// Group alpha is intentionally excluded from geometry transport.
function value(i) { return input.deps[i].value; }
function matrix(i) {
    var ax=value(i), ay=value(i+1), px=value(i+2), py=value(i+3);
    var sx=value(i+4)/100, sy=value(i+5)/100, d=Math.PI/180;
    var r=(value(i+6)-value(i+8))*d, a=value(i+8)*d;
    var h=-Math.tan(Math.max(-89.9,Math.min(89.9,value(i+7)))*d);
    var ca=Math.cos(a), sa=Math.sin(a), cr=Math.cos(r), sr=Math.sin(r);
    var a0=(cr*(ca+h*sa)-sr*sa)*sx;
    var b0=(sr*(ca+h*sa)+cr*sa)*sx;
    var c0=(cr*(-sa+h*ca)-sr*ca)*sy;
    var d0=(sr*(-sa+h*ca)+cr*ca)*sy;
    return [a0,b0,c0,d0,px-a0*ax-c0*ay,py-b0*ax-d0*ay];
}
function multiply(a,b) {
    return [a[0]*b[0]+a[2]*b[1], a[1]*b[0]+a[3]*b[1],
        a[0]*b[2]+a[2]*b[3], a[1]*b[2]+a[3]*b[3],
        a[0]*b[4]+a[2]*b[5]+a[4], a[1]*b[4]+a[3]*b[5]+a[5]];
}
function point(m,x,y) { return [m[0]*x+m[2]*y+m[4],m[1]*x+m[3]*y+m[5]]; }
// Resolve the imported axis-aligned Rectangle's intrinsic rounding in its
// own coordinate system. Transporting scalar radius metadata is not affine.
function roundedRectangle(path) {
    if(path.length!==5 || path[0].type!=='moveTo' || path[4].type!=='close') return path;
    var r=path[0].cornerRadius, edges=[];
    if(!(r>0))return path;
    for(var i=0;i<4;i++) {
        var a=path[i],b=path[(i+1)%4],dx=b.x-a.x,dy=b.y-a.y;
        if((i>0 && a.type!=='lineTo') || a.cornerRadius!==r || (dx===0)===(dy===0))return path;
        edges.push([dx,dy,Math.hypot(dx,dy)]);
    }
    if(edges[0][0]!==-edges[2][0] || edges[0][1]!==-edges[2][1] ||
       edges[1][0]!==-edges[3][0] || edges[1][1]!==-edges[3][1] ||
       (edges[0][0]===0)===(edges[1][0]===0))return path;
    r=Math.min(r,edges[0][2]/2,edges[1][2]/2);
    var incoming=[],outgoing=[],k=0.5522847498307936;
    for(var j=0;j<4;j++) {
        var p=path[j],prev=edges[(j+3)%4],next=edges[j];
        incoming.push([p.x-prev[0]/prev[2]*r,p.y-prev[1]/prev[2]*r]);
        outgoing.push([p.x+next[0]/next[2]*r,p.y+next[1]/next[2]*r]);
    }
    // AE's reversed Rectangle starts at the same right-edge anchor as its
    // forward version, then traverses the upper-right curve first.
    var reverseStart=edges[0][0]<0 && edges[3][1]<0;
    var origin=reverseStart?incoming[0]:outgoing[0];
    var result=[{type:'moveTo',x:origin[0],y:origin[1]}];
    function corner(index) {
        var c=path[index],start=incoming[index],end=outgoing[index];
        return {type:'cubicTo',c1x:start[0]+(c.x-start[0])*k,c1y:start[1]+(c.y-start[1])*k,
            c2x:end[0]+(c.x-end[0])*k,c2y:end[1]+(c.y-end[1])*k,x:end[0],y:end[1]};
    }
    if(reverseStart)result.push(corner(0));
    for(var n=1;n<=4;n++) {
        var index=n%4,start=incoming[index];
        result.push({type:'lineTo',x:start[0],y:start[1]});
        if(index!==0 || !reverseStart)result.push(corner(index));
    }
    result.push({type:'close'});
    return result;
}
function transport(path,m) {
    var result=[];
    for(var k=0;k<path.length;k++) {
        // Never mutate a shared producer's path while preparing another paint.
        var c=path[k], out={type:c.type};
        if(c.type!=='close') {
            var p=point(m,c.x,c.y); out.x=p[0];out.y=p[1];
            if(c.mirror!==undefined)out.mirror=c.mirror;
            if(c.cornerRadius!==undefined)out.cornerRadius=c.cornerRadius;
            if(c.type==='cubicTo') {
                p=point(m,c.c1x,c.c1y);out.c1x=p[0];out.c1y=p[1];
                p=point(m,c.c2x,c.c2y);out.c2x=p[0];out.c2y=p[1];
            }
        }
        result.push(out);
    }
    return result;
}
var commands=[],basePaths=[],remaining=0;
for(var b=0;b<sources.length;b++) {
    var base=roundedRectangle(value(sources[b].path).commands);
    remaining+=base.length;
    if(remaining>10000)throw new Error('AEP compound outline exceeds 10000 commands');
    basePaths.push(base);
}
for (var s=0;s<sources.length;s++) {
    var source=sources[s], path=basePaths[s];
    remaining-=path.length;
    var stages=source.rounds||[];
    if(stages.some(function(stage){return stage.length>0;})) {
        for(var stage=0;stage<stages.length;stage++) {
            for(var r=0;r<stages[stage].length;r++) {
                var rounded=roundOutline(path,value(stages[stage][r]));
                if(commands.length+rounded.length+remaining<=10000)path=rounded;
            }
            if(stage<source.transforms.length)path=transport(path,matrix(source.transforms[stage]));
        }
    } else {
        var m=[1,0,0,1,0,0];
        for(var j=0;j<source.transforms.length;j++)m=multiply(matrix(source.transforms[j]),m);
        path=transport(path,m);
    }
    if(commands.length+path.length>10000)throw new Error('AEP compound outline exceeds 10000 commands');
    for(var k=0;k<path.length;k++)commands.push(path[k]);
}
return {commands:commands};
