// Converter-private Path values. Only this helper creates branded values; numeric
// finite() must not accept them. Native snapshots retain their unused handles.
function __aePathRuntime(fail, approximation, unwrap) {
    const owned = new WeakSet();
    const arrays = new WeakMap();
    const slots = new WeakSet();
    function copied(values) {
        const result = values.map(value => {
            const row = vector(value);
            arrays.set(row, 'row');
            return row;
        });
        arrays.set(result, 'outer');
        return result;
    }
    function index(value, key) {
        if (!Array.isArray(value) || !arrays.has(value)) fail('Path array access requires a copied Path array');
        if (typeof key !== 'number' || !Number.isInteger(key) || key < 0 || key >= value.length) fail('invalid Path array index');
    }
    function get(value, key) {
        index(value, key);
        return value[key];
    }
    // Capture the reference and old value before evaluating a compound RHS.
    function slot(value, key) {
        index(value, key);
        const result = {value, key, old:value[key]};
        slots.add(result);
        return result;
    }
    function set(reference, value, operation) {
        if (!slots.has(reference)) fail('invalid Path array write reference');
        slots.delete(reference);
        const {value:target, key, old} = reference;
        if (operation) value = operation(old, value);
        if (arrays.get(target) === 'outer') {
            value = vector(value);
            arrays.set(value, 'row');
        } else if (typeof value !== 'number' || !Number.isFinite(value)) {
            fail('invalid Path coordinate');
        }
        target[key] = value;
        return value;
    }
    function vector(value) {
        if (!Array.isArray(value) || value.length !== 2 || [0,1].some(i => typeof value[i] !== 'number' || !Number.isFinite(value[i]))) fail('invalid Path coordinate');
        return value.slice();
    }
    function native(value) {
        if (!value || !Array.isArray(value.vertices) || value.vertices.length === 0 || typeof value.closed !== 'boolean') fail('invalid single-contour Path');
        if(value.vertices.length>8190)fail('Path vertex budget exceeded');
        const vertices = Array.from(value.vertices,vector);
        const incoming = value.in_tangents, outgoing = value.out_tangents;
        if (!Array.isArray(incoming) || !Array.isArray(outgoing) || incoming.length !== vertices.length || outgoing.length !== vertices.length) fail('Path handle counts disagree');
        const result = {vertices, in_tangents:Array.from(incoming,vector), out_tangents:Array.from(outgoing,vector), closed:value.closed};
        for (let i=0;i<vertices.length;++i) for (const handles of [result.in_tangents,result.out_tangents]) {
            if (vertices[i].some((x,j)=>!Number.isFinite(x+handles[i][j]))) fail('overflowing Path control point');
        }
        for (const values of [vertices,result.in_tangents,result.out_tangents]) {
            values.forEach(Object.freeze); Object.freeze(values);
        }
        owned.add(result);
        return Object.freeze(result);
    }
    function requirePath(value) {
        value=unwrap(value);
        if (!owned.has(value)) fail('Path API requires a native Path value');
        return value;
    }
    function createPath(points, incoming, outgoing, closed=true) {
        if (!Array.isArray(points) || points.length===0) fail('createPath requires nonempty points');
        if (typeof closed!=='boolean') fail('createPath closure must be boolean');
        const zeros=()=>points.map(()=>[0,0]);
        incoming=incoming===undefined || (Array.isArray(incoming) && incoming.length===0)?zeros():incoming;
        outgoing=outgoing===undefined || (Array.isArray(outgoing) && outgoing.length===0)?zeros():outgoing;
        // Adobe zeros unused handles only for newly created OPEN Paths.
        if (!closed) {
            if (!Array.isArray(incoming) || !Array.isArray(outgoing) || incoming.length!==points.length || outgoing.length!==points.length) fail('Path handle counts disagree');
            incoming=incoming.map(vector); outgoing=outgoing.map(vector);
            incoming[0]=[0,0]; outgoing[outgoing.length-1]=[0,0];
        }
        return native({vertices:points,in_tangents:incoming,out_tangents:outgoing,closed});
    }
    function interpolate(a,b,u) {
        a=requirePath(a); b=requirePath(b);
        if (!Number.isFinite(u) || a.closed!==b.closed || a.vertices.length!==b.vertices.length) fail('unsupported Path morph topology');
        const blend=(left,right)=>left.map((v,i)=>v.map((x,j)=>x+(right[i][j]-x)*u));
        return native({vertices:blend(a.vertices,b.vertices),in_tangents:blend(a.in_tangents,b.in_tangents),out_tangents:blend(a.out_tangents,b.out_tangents),closed:a.closed});
    }
    const add=(a,b)=>[a[0]+b[0],a[1]+b[1]];
    function cubic(c,t) {
        const s=1-t;
        return [0,1].map(j=>s*s*s*c[0][j]+3*s*s*t*c[1][j]+3*s*t*t*c[2][j]+t*t*t*c[3][j]);
    }
    function derivative(c,t) {
        const s=1-t;
        return [0,1].map(j=>3*(s*s*(c[1][j]-c[0][j])+2*s*t*(c[2][j]-c[1][j])+t*t*(c[3][j]-c[2][j])));
    }
    const speed=(c,t)=>Math.hypot(...derivative(c,t));
    // Adaptive Simpson integration and monotone inversion use arc length, not
    // cubic parameter. Tables are recomputed for the actual requested geometry.
    function length(c,end) {
        const simpson=(a,b,fa,fm,fb)=>(b-a)*(fa+4*fm+fb)/6;
        function integrate(a,b,fa,fm,fb,whole,tolerance,depth) {
            const middle=(a+b)/2, fl=speed(c,(a+middle)/2), fr=speed(c,(middle+b)/2);
            const left=simpson(a,middle,fa,fl,fm),right=simpson(middle,b,fm,fr,fb);
            if (depth===0 || Math.abs(left+right-whole)<=15*tolerance) return left+right+(left+right-whole)/15;
            return integrate(a,middle,fa,fl,fm,left,tolerance/2,depth-1)+integrate(middle,b,fm,fr,fb,right,tolerance/2,depth-1);
        }
        if (end===0) return 0;
        const fa=speed(c,0),fm=speed(c,end/2),fb=speed(c,end);
        const result=integrate(0,end,fa,fm,fb,simpson(0,end,fa,fm,fb),1e-8,20);
        if (!Number.isFinite(result) || result<0) fail('invalid Path arc length');
        return result;
    }
    function traverse(value,percentage,kind) {
        approximation('path:arc-length-analytic');
        const p=requirePath(value);
        if (typeof percentage!=='number' || !Number.isFinite(percentage)) fail('invalid Path percentage');
        percentage=Math.max(0,Math.min(1,percentage));
        const segments=[];
        let total=0;
        const count=p.closed?p.vertices.length:p.vertices.length-1;
        for(let i=0;i<count;++i) {
            const j=(i+1)%p.vertices.length;
            const controls=[p.vertices[i],add(p.vertices[i],p.out_tangents[i]),add(p.vertices[j],p.in_tangents[j]),p.vertices[j]];
            const size=length(controls,1);
            segments.push({controls,size}); total+=size;
        }
        if (!Number.isFinite(total)) fail('overflowing Path arc length');
        if (total===0) return kind==='point'?p.vertices[0].slice():[0,0];
        let remaining=percentage*total,selected=segments[segments.length-1];
        for (const segment of segments) {
            if (segment.size>0 && remaining<=segment.size) {selected=segment;break;}
            remaining-=segment.size;
        }
        let t;
        if (remaining<=0) t=0;
        else if (remaining>=selected.size) t=1;
        else {
            // Native traversal uses 100 uniform parameter samples to invert the
            // absolute distance from accurate segment lengths, not a fraction
            // normalized by the sampled chord total.
            const distances=[0];
            let previous=cubic(selected.controls,0),distance=0;
            for(let i=1;i<100;++i) {
                const point=cubic(selected.controls,i/99);
                distance+=Math.hypot(point[0]-previous[0],point[1]-previous[1]);
                if (!Number.isFinite(distance)) fail('overflowing Path arc length');
                distances.push(distance);previous=point;
            }
            t=1;
            for(let i=1;i<100;++i) {
                const chord=distances[i]-distances[i-1];
                if(chord>0 && remaining<=distances[i]) {
                    t=(i-1+(remaining-distances[i-1])/chord)/99;
                    break;
                }
            }
        }
        if(kind==='point') return cubic(selected.controls,t);
        let direction=derivative(selected.controls,t),magnitude=Math.hypot(...direction);
        // A repeated endpoint control has zero derivative but a well-defined
        // one-sided tangent. Fully degenerate geometry remains the zero vector.
        if(magnitude===0) {
            const nearby=t===1?1-1e-7:t+1e-7;
            direction=derivative(selected.controls,nearby);magnitude=Math.hypot(...direction);
        }
        if (!Number.isFinite(magnitude)) fail('invalid Path tangent');
        if(magnitude===0)return [0,0];
        direction=direction.map(x=>x/magnitude);
        return kind==='normal'?[-direction[1],direction[0]]:direction;
    }
    function methods(read, currentTime, scalar) {
        const at=t=>requirePath(read(t===undefined?currentTime():scalar(t)));
        return {
            points:t=>{approximation('path:points-1/16-pixel');return copied(at(t).vertices.map(v=>v.map(x=>Math.round(x*16)/16)));},
            inTangents:t=>copied(at(t).in_tangents),
            outTangents:t=>copied(at(t).out_tangents),
            isClosed:t=>at(t).closed,
            pointOnPath:(percentage=0.5,t)=>traverse(at(t),scalar(percentage),'point'),
            tangentOnPath:(percentage=0.5,t)=>traverse(at(t),scalar(percentage),'tangent'),
            normalOnPath:(percentage=0.5,t)=>traverse(at(t),scalar(percentage),'normal')
        };
    }
    return {native,requirePath,createPath,interpolate,methods,get,slot,set};
}
