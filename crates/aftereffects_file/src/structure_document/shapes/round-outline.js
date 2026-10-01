// Round-only source stages mirror scene::Path::round_corners before group
// coordinate transport. Unsupported custom corner data is never overwritten.
function roundOutline(path,radius) {
    if(!(radius>0)||path.length<3||path.length>4999) return path;
    if(path.some(function(c){return c.type==='cubicTo'||c.cornerRadius!==undefined;}))return path;
    var contours=[],points=[];
    function finish(closed) {
        if(closed&&points.length>=3) {
            var a=points[0],b=points[points.length-1];
            if(Math.abs(a.x-b.x)<1e-3&&Math.abs(a.y-b.y)<1e-3)points.pop();
        }
        if(points.length>=2)contours.push({points:points,closed:closed});
        points=[];
    }
    for(var i=0;i<path.length;i++) {
        var c=path[i];
        if(c.type==='moveTo'){finish(false);points.push(c);}
        else if(c.type==='lineTo')points.push(c);
        else if(c.type==='close')finish(true);
    }
    finish(false);
    var result=[],kappa=0.5522847498307936;
    for(var j=0;j<contours.length;j++) {
        var contour=contours[j],p=contour.points,n=p.length;
        for(var k=0;k<n;k++) {
            var c=p[k],type=k===0?'moveTo':'lineTo';
            if(n<3||(!contour.closed&&(k===0||k===n-1))) {
                result.push({type:type,x:c.x,y:c.y});continue;
            }
            var prev=p[(k+n-1)%n],next=p[(k+1)%n];
            var ux=prev.x-c.x,uy=prev.y-c.y,vx=next.x-c.x,vy=next.y-c.y;
            var a=Math.hypot(ux,uy),b=Math.hypot(vx,vy);
            if(a<1e-6||b<1e-6){result.push({type:type,x:c.x,y:c.y});continue;}
            var r=Math.min(radius,a/2,b/2),sx=c.x+ux/a*r,sy=c.y+uy/a*r,ex=c.x+vx/b*r,ey=c.y+vy/b*r;
            result.push({type:type,x:sx,y:sy});
            result.push({type:'cubicTo',c1x:sx+(c.x-sx)*kappa,c1y:sy+(c.y-sy)*kappa,
                c2x:ex+(c.x-ex)*kappa,c2y:ey+(c.y-ey)*kappa,x:ex,y:ey});
        }
        if(contour.closed)result.push({type:'close'});
    }
    return result.length<=10000?result:path;
}
