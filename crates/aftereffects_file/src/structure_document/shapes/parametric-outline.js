// Existing FX ellipse/integer-poly-star construction with editable dependencies.
// Fractional point counts are intentionally floored after the existing bounds.
// This is a generated geometry evaluator, not an AE expression interpreter.
function v(i) { return input.deps[i].value; }
var commands=[];
function move(x,y) { commands.push({type:'moveTo',x:x,y:y}); }
function line(x,y) { commands.push({type:'lineTo',x:x,y:y}); }
function cubic(a,b,c,d,x,y) { commands.push({type:'cubicTo',c1x:a,c1y:b,c2x:c,c2y:d,x:x,y:y}); }
if(kind==='ellipse') {
    var size=v(0), p=v(1), x=p[0], y=p[1], rx=size[0]/2, ry=size[1]/2;
    var k=0.5522847498307936, kx=rx*k, ky=ry*k;
    move(x,y-ry);
    cubic(x+kx,y-ry,x+rx,y-ky,x+rx,y);
    cubic(x+rx,y+ky,x+kx,y+ry,x,y+ry);
    cubic(x-kx,y+ry,x-rx,y+ky,x-rx,y);
    cubic(x-rx,y-ky,x-kx,y-ry,x,y-ry);
} else {
    var p=v(0), x=p[0], y=p[1], n=Math.floor(Math.max(3,Math.min(1000,v(1))));
    var angle=(v(2)-90)*Math.PI/180, outer=v(3), inner=v(4), ro=v(5)/100, ri=v(6)/100;
    var star=kind==='star', count=star?n*2:n, step=2*Math.PI/count;
    var tangent=step*(star?0.5:0.25), rounded=Math.abs(ro)>1e-6||(star&&Math.abs(ri)>1e-6), corners=[];
    for(var i=0;i<count;i++) {
        var isInner=star&&i%2===1, r=isInner?inner:outer, a=angle+i*step;
        corners.push({x:r*Math.cos(a),y:r*Math.sin(a),r:isInner?ri:ro});
    }
    function edge(a,b) {
        cubic(x+a.x-a.y*a.r*tangent,y+a.y+a.x*a.r*tangent,
            x+b.x+b.y*b.r*tangent,y+b.y-b.x*b.r*tangent,x+b.x,y+b.y);
    }
    move(x+corners[0].x,y+corners[0].y);
    for(var i=1;i<count;i++) {
        if(rounded)edge(corners[i-1],corners[i]);
        else line(x+corners[i].x,y+corners[i].y);
    }
    if(rounded)edge(corners[count-1],corners[0]);
}
if(reversed) {
    var first=commands[0], previous=first, edges=[];
    for(var i=1;i<commands.length;i++) {
        edges.push({start:previous,command:commands[i]});previous=commands[i];
    }
    if(previous.x!==first.x||previous.y!==first.y)edges.push({start:previous,command:{type:'lineTo',x:first.x,y:first.y}});
    commands=[first];
    for(var i=edges.length-1;i>=0;i--) {
        var e=edges[i], c=e.command, p=e.start;
        if(c.type==='cubicTo')cubic(c.c2x,c.c2y,c.c1x,c.c1y,p.x,p.y);
        else line(p.x,p.y);
    }
}
commands.push({type:'close'});
return {commands:commands};
