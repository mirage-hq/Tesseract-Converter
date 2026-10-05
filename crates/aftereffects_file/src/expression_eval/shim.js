// Numeric AE subset. Each invocation has fresh dependency/cache/clock state.
const model = input.model;
const active = new Set();
const memo = new Map();
const approximated = new Set();
let failure = null;
function __aeFail(message) { failure = Error(message); throw failure; }
const __aeToken = Symbol();
function __aeUnwrap(v) { return v && v[__aeToken] ? v[__aeToken]() : v; }
function finite(v) {
    v = __aeUnwrap(v);
    if (typeof v === 'number' && Number.isFinite(v)) return v;
    if (Array.isArray(v) && v.length > 0 && v.length <= 4 && v.every(x => typeof x === 'number' && Number.isFinite(x))) return v;
    __aeFail('nonfinite or nonnumeric expression value');
}
function binary(a, b, op, scalarVector) {
    a = __aeUnwrap(a); b = __aeUnwrap(b);
    if (typeof a === 'number' && typeof b === 'number') return finite(op(a,b));
    finite(a); finite(b);
    if (Array.isArray(a) && Array.isArray(b)) {
        if (scalarVector) __aeFail('vector/vector multiplication or division is not admitted');
        if (a.length !== b.length) __aeFail('unequal vector dimensions are not admitted');
        return finite(a.map((v,i) => op(v,b[i])));
    }
    if (!scalarVector) __aeFail('scalar/vector addition or subtraction is not admitted');
    return finite(Array.isArray(a) ? a.map(v=>op(v,b)) : b.map(v=>op(a,v)));
}
function __aeAdd(a,b) {
    a=__aeUnwrap(a); b=__aeUnwrap(b);
    if (typeof a==='string' || typeof b==='string') {
        if (Array.isArray(a) || Array.isArray(b)) __aeFail('vector string coercion is not admitted');
        return a+b;
    }
    return binary(a,b,(x,y)=>x+y,false);
}
function __aeSub(a,b) { return binary(a,b,(x,y)=>x-y,false); }
function __aeMul(a,b) { return binary(a,b,(x,y)=>x*y,true); }
function __aeDiv(a,b) { return binary(a,b,(x,y)=>x/y,true); }
function __aeMod(a,b) { return binary(a,b,(x,y)=>x%y,true); }
function __aeNeg(a) { a=finite(a); return Array.isArray(a) ? a.map(x=>-x) : -a; }
function scalar(v) { v=finite(v); if(Array.isArray(v)) __aeFail('expected a scalar'); return v; }
// Same arithmetic as the converter's Rust cubic_bezier_progress: 24-step
// bisection for the curve parameter, then the y polynomial.
function __aeCubic(t,p0,p1,p2,p3) {
    const s=1-t;
    return s*s*s*p0+3*s*s*t*p1+3*s*t*t*p2+t*t*t*p3;
}
function __aeEase(progress,e) {
    if(e===null) return progress;
    let low=0,high=1;
    for(let i=0;i<24;++i){const middle=(low+high)*0.5;if(__aeCubic(middle,0,e[0],e[2],1)<progress)low=middle;else high=middle;}
    return __aeCubic((low+high)*0.5,0,e[1],e[3],1);
}
function __aeDerivative(read,t) {
    const h=1e-5, a=finite(read(t-h)), b=finite(read(t+h));
    return Array.isArray(a) ? a.map((v,i)=>(b[i]-v)/(2*h)) : (b-a)/(2*h);
}
function __aeSpeed(v) { return Array.isArray(v) ? Math.sqrt(v.reduce((s,x)=>s+x*x,0)) : Math.abs(v); }
function __aeSmooth(read,width,samples,t) {
    if(!Number.isInteger(samples)||samples<1||samples>256||!(width>=0))__aeFail('smooth requires 1..256 samples and nonnegative width');
    let sum=null;
    for(let i=0;i<samples;i++){
        const u=samples===1?0:i/(samples-1)-0.5, v=finite(read(t+u*width));
        sum=sum===null?(Array.isArray(v)?v.slice():v):(Array.isArray(v)?sum.map((x,j)=>x+v[j]):sum+v);
    }
    return Array.isArray(sum)?sum.map(x=>x/samples):sum/samples;
}
// Column-major 4x4 affine matrices (12 numbers: 3x3 linear + translation).
function __aeCompose(m,n) {
    const r=new Array(12);
    for(let c=0;c<4;c++) for(let row=0;row<3;row++) {
        r[c*3+row]=m[row]*n[c*3]+m[3+row]*n[c*3+1]+m[6+row]*n[c*3+2]+(c===3?m[9+row]:0);
    }
    return r;
}
function __aeInvert(m) {
    const [a,b,c,d,e,f,g,h,i]=m;
    const det=a*(e*i-f*h)-d*(b*i-c*h)+g*(b*f-c*e);
    if(!(Math.abs(det)>1e-12))__aeFail('singular layer transform');
    const inv=[(e*i-f*h)/det,(c*h-b*i)/det,(b*f-c*e)/det,
        (f*g-d*i)/det,(a*i-c*g)/det,(c*d-a*f)/det,
        (d*h-e*g)/det,(b*g-a*h)/det,(a*e-b*d)/det];
    const t=[m[9],m[10],m[11]];
    return inv.concat([0,1,2].map(row=>-(inv[row]*t[0]+inv[3+row]*t[1]+inv[6+row]*t[2])));
}
function __aeApply(m,p) {
    p=finite(p);if(!Array.isArray(p)||p.length<2)__aeFail('layer space transforms require a 2D/3D point');
    const z=p.length>2?p[2]:0;
    const out=[0,1,2].map(row=>m[row]*p[0]+m[3+row]*p[1]+m[6+row]*z+m[9+row]);
    return p.length>2?out:[out[0],out[1]];
}
function __aeRotation(axis,degrees) {
    const r=degrees*Math.PI/180,c=Math.cos(r),s=Math.sin(r);
    if(axis===0) return [1,0,0, 0,c,s, 0,-s,c, 0,0,0];
    if(axis===1) return [c,0,-s, 0,1,0, s,0,c, 0,0,0];
    return [c,s,0, -s,c,0, 0,0,1, 0,0,0];
}
function authored(slot,t) {
    const p=model.properties[slot];
    if(p.error) __aeFail(p.error);
    if(p.text!==null) return p.text;
    const keys=p.keys;
    let value;
    if(keys.length===0) value=p.initial;
    else if(t<=keys[0].time) value=keys[0].value;
    else if(t>=keys[keys.length-1].time) value=keys[keys.length-1].value;
    else {
        let i=1; while(keys[i].time<t) ++i;
        const a=keys[i-1], b=keys[i];
        const u=(t-a.time)/(b.time-a.time);
        value = t===b.time ? b.value : a.hold ? a.value : a.value.map((v,j)=>v+(b.value[j]-v)*__aeEase(u,b.ease[j]));
    }
    return value.length===1 ? finite(value[0]) : finite(value.slice());
}
function nativeKey(slot,index) {
    const property=model.properties[slot];
    if(property.error!==null) __aeFail(property.error);
    const keys=property.keys;
    if (!Number.isInteger(index) || index<1 || index>keys.length) throw RangeError('invalid native key index');
    const key=keys[index-1];
    const result={index,time:key.time,value:key.value.length===1?key.value[0]:key.value.slice()};
    for(let i=0;i<4;++i) Object.defineProperty(result,String(i),{get:()=>{
        if(i>=key.value.length) throw RangeError('invalid native key component');
        return key.value[i];
    }});
    return Object.freeze(result);
}
function nearestNativeKey(slot,time) {
    const property=model.properties[slot];
    if(property.error!==null) __aeFail(property.error);
    const keys=property.keys;
    if(!keys.length) throw RangeError('native property has no keys');
    let nearest=0;
    for(let i=1;i<keys.length;++i) if(Math.abs(keys[i].time-time)<Math.abs(keys[nearest].time-time)) nearest=i;
    return nativeKey(slot,nearest+1);
}
function unique(list,test,kind) {
    const found=list.filter(test);
    if(found.length!==1) __aeFail('missing/ambiguous '+kind);
    return found[0];
}
function evaluate(slot,requestedTime) {
    const cacheKey=slot+':'+requestedTime;
    // A temporal self-reference is also a cycle, never a pre-expression fallback.
    if(active.has(slot)) __aeFail('expression dependency cycle');
    if(memo.has(cacheKey)) return memo.get(cacheKey);
    active.add(slot);
    try {
        const p=model.properties[slot];
        if(p.error) __aeFail(p.error);
        const c=unique(model.comps,c=>c.id===p.comp_id,'composition');
        const l=unique(c.layers,l=>l.id===p.layer_id,'layer');
        let time=scalar(requestedTime);
        let value=authored(slot,time);
        function property(other,preExpression=false) {
            const read=t=>preExpression ? authored(other,t) : evaluate(other,t);
            const object={
                [__aeToken]:()=>read(time),
                get value(){return read(time);},
                valueAtTime:t=>read(scalar(t)),
                get numKeys(){
                    const dependency=model.properties[other];
                    if(dependency.error!==null) __aeFail(dependency.error);
                    return dependency.keys.length;
                },
                key:index=>nativeKey(other,scalar(index)),
                nearestKey:t=>nearestNativeKey(other,scalar(t)),
                get length(){const v=read(time);return Array.isArray(v)?v.length:1;},
                velocityAtTime:t=>__aeDerivative(read,scalar(t)),
                speedAtTime:t=>__aeSpeed(__aeDerivative(read,scalar(t))),
                get velocity(){return __aeDerivative(read,time);},
                get speed(){return __aeSpeed(__aeDerivative(read,time));},
                smooth:(width=0.2,samples=5,t=time)=>__aeSmooth(read,scalar(width),scalar(samples),scalar(t)),
                [Symbol.toPrimitive]:()=>{const v=read(time);if(Array.isArray(v))__aeFail('vector coercion requires an admitted operator');return v;}
            };
            for(let i=0;i<4;++i) Object.defineProperty(object,String(i),{get:()=>{
                const v=read(time); if(!Array.isArray(v)||i>=v.length) __aeFail('invalid vector index'); return v[i];
            }});
            return object;
        }
        // AE exposes a separated Position as the vector of its dimension values.
        function composite(slots) {
            const read=t=>finite(slots.map(other=>scalar(evaluate(other,t))));
            const object={
                [__aeToken]:()=>read(time),
                get value(){return read(time);},
                valueAtTime:t=>read(scalar(t)),
                get length(){return slots.length;},
                [Symbol.toPrimitive]:()=>__aeFail('vector coercion requires an admitted operator')
            };
            for(let i=0;i<4;++i) Object.defineProperty(object,String(i),{get:()=>{
                if(i>=slots.length) __aeFail('invalid vector index'); return scalar(evaluate(slots[i],time));
            }});
            return object;
        }
        function layer(nativeLayer) {
            const transform={};
            const separated=nativeLayer.separated_position;
            for(const name of Object.keys(nativeLayer.transform)) Object.defineProperty(transform,name,{get:()=>
                name==='position'&&separated!==null ? composite(separated) : property(nativeLayer.transform[name])});
            const effect=selector=>{
                const e=unique(nativeLayer.effects,e=>typeof selector==='number'?e.index===selector:e.name===selector||e.match_name===selector,'effect');
                return selector=>{
                    const p=unique(e.parameters,p=>typeof selector==='number'?p.index===selector:p.name===selector||p.match_name===selector,'effect parameter');
                    return property(p.slot);
                };
            };
            const result={transform,effect,index:nativeLayer.index,inPoint:nativeLayer.in_point,outPoint:nativeLayer.out_point,
                width:nativeLayer.width,height:nativeLayer.height,name:nativeLayer.name,
                hasParent:nativeLayer.parent!==null,
                toComp:(p,t=time)=>{
                    const world=__aeApply(layerMatrix(nativeLayer,scalar(t)),finite(p).length>2?p:[p[0],p[1],0]);
                    const out=nativeLayer.three_d?project(nativeLayer,world):world;
                    return finite(p).length>2?out:[out[0],out[1]];
                },
                fromComp:(p,t=time)=>{
                    p=finite(p);
                    const world=nativeLayer.three_d?unproject(nativeLayer,p):[p[0],p[1],p.length>2?p[2]:0];
                    const out=__aeApply(__aeInvert(layerMatrix(nativeLayer,scalar(t))),world);
                    return p.length>2?out:[out[0],out[1]];
                },
                content:selector=>contentItem(nativeLayer.content,selector),
                sourceRectAtTime:(t=time,includeExtents=false)=>shapeRect(nativeLayer,scalar(t),includeExtents===true),
                text:{get sourceText(){
                    if(nativeLayer.source_text===null) __aeFail('layer has no evaluable Source Text');
                    return property(nativeLayer.source_text);
                }},
                mask:selector=>maskItem(nativeLayer,selector)};
            result.toWorld=result.toComp; result.fromWorld=result.fromComp;
            Object.defineProperty(result,'active',{get:()=>nativeLayer.enabled
                && (nativeLayer.in_point===null || time>=nativeLayer.in_point)
                && (nativeLayer.out_point===null || time<nativeLayer.out_point)});
            Object.defineProperty(result,'parent',{get:()=>{
                if(nativeLayer.parent===null) __aeFail('layer has no parent');
                return layer(parentOf(nativeLayer));
            }});
            for(const name of Object.keys(nativeLayer.transform)) Object.defineProperty(result,name,{get:()=>transform[name]});
            return result;
        }
        // AE's default comp camera (50 mm on 36 mm film) when no camera layer
        // exists: perspective about the comp center, depth reported as
        // zoom - zoom/distance. Explicit camera layers are not modelled.
        function cameraOf(nativeLayer) {
            const owner=unique(model.comps,c=>c.layers.includes(nativeLayer),'layer composition');
            if(owner.has_camera) __aeFail('3D layer space through a camera layer is not admitted');
            return {zoom:owner.width*50/36,cx:owner.width/2,cy:owner.height/2};
        }
        function project(nativeLayer,w) {
            const {zoom,cx,cy}=cameraOf(nativeLayer), d=w[2]+zoom;
            if(!(d>0)) __aeFail('point behind the default camera');
            return [cx+(w[0]-cx)*zoom/d,cy+(w[1]-cy)*zoom/d,zoom-zoom/d];
        }
        function unproject(nativeLayer,q) {
            const {zoom,cx,cy}=cameraOf(nativeLayer), z=q.length>2?q[2]:0;
            if(!(zoom-z>0)) __aeFail('composition depth outside the default camera range');
            const d=zoom/(zoom-z);
            return [cx+(q[0]-cx)*d/zoom,cy+(q[1]-cy)*d/zoom,d-zoom];
        }
        function parentOf(nativeLayer) {
            const owner=unique(model.comps,c=>c.layers.includes(nativeLayer),'layer composition');
            return unique(owner.layers,l=>l.id===nativeLayer.parent,'parent layer');
        }
        // Planar layer-to-composition affine [a,b,c,d,tx,ty] including parents.
        // Layer -> composition affine: T(position)·Orientation(X,Y,Z)·Rx·Ry·Rz·S·T(-anchor).
        function layerMatrix(nativeLayer,t) {
            const get=(alias,fallback)=>{
                const slot=nativeLayer.transform[alias];
                return slot===undefined ? fallback : evaluate(slot,t);
            };
            const vec3=v=>Array.isArray(v)?[v[0],v[1],v.length>2?v[2]:0]:[v,0,0];
            const position=vec3(nativeLayer.separated_position!==null
                ? nativeLayer.separated_position.map(slot=>scalar(evaluate(slot,t)))
                : get('position',[0,0,0]));
            const anchor=vec3(get('anchorPoint',nativeLayer.default_anchor));
            const scale=vec3(get('scale',[100,100,100]));
            if(!nativeLayer.three_d) scale[2]=100;
            const orientation=nativeLayer.three_d?vec3(get('orientation',[0,0,0])):[0,0,0];
            let m=[1,0,0, 0,1,0, 0,0,1, position[0],position[1],nativeLayer.three_d?position[2]:0];
            const rotations=nativeLayer.three_d
                ? [[0,orientation[0]],[1,orientation[1]],[2,orientation[2]],
                   [0,get('xRotation',0)],[1,get('yRotation',0)],[2,get('rotation',0)]]
                : [[2,get('rotation',0)]];
            for(const [axis,degrees] of rotations) if(degrees!==0) m=__aeCompose(m,__aeRotation(axis,degrees));
            m=__aeCompose(m,[scale[0]/100,0,0, 0,scale[1]/100,0, 0,0,scale[2]/100, 0,0,0]);
            m=__aeCompose(m,[1,0,0, 0,1,0, 0,0,1, -anchor[0],-anchor[1],nativeLayer.three_d?-anchor[2]:0]);
            return nativeLayer.parent===null ? m : __aeCompose(layerMatrix(parentOf(nativeLayer),t),m);
        }
        // Shape-layer source rectangle from Rect/Ellipse/Polystar geometry and
        // group transforms; text and free-form path bounds are not modelled.
        function shapeRect(nativeLayer,t,extents) {
            if(nativeLayer.content.length===0) __aeFail('sourceRectAtTime is only admitted for Shape layers');
            const read=(node,alias,fallback)=>{
                const child=node.children.find(c=>c.alias===alias);
                return child&&child.slot!==null ? finite(evaluate(child.slot,t)) : fallback;
            };
            let box=null;
            const add=(x,y)=>{ box=box===null?[x,y,x,y]:[Math.min(box[0],x),Math.min(box[1],y),Math.max(box[2],x),Math.max(box[3],y)]; };
            function visit(items,matrix) {
                let stroke=0;
                for(const node of items) if(node.match_name==='ADBE Vector Graphic - Stroke') stroke=Math.max(stroke,read(node,'strokeWidth',0));
                for(const node of items) {
                    const m=node.match_name;
                    if(m==='ADBE Vector Group') {
                        const tr=node.children.find(c=>c.alias==='transform');
                        let local=matrix;
                        if(tr) {
                            const pos=read(tr,'position',[0,0]),anc=read(tr,'anchorPoint',[0,0]),sc=read(tr,'scale',[100,100]),rot=read(tr,'rotation',0);
                            let g=[1,0,0, 0,1,0, 0,0,1, pos[0],pos[1],0];
                            if(rot!==0) g=__aeCompose(g,__aeRotation(2,rot));
                            g=__aeCompose(g,[sc[0]/100,0,0, 0,sc[1]/100,0, 0,0,1, 0,0,0]);
                            g=__aeCompose(g,[1,0,0, 0,1,0, 0,0,1, -anc[0],-anc[1],0]);
                            local=__aeCompose(matrix,g);
                        }
                        const list=node.children.find(c=>c.alias==='content');
                        if(list) visit(list.children,local);
                    } else if(m==='ADBE Vector Shape - Rect'||m==='ADBE Vector Shape - Ellipse'||m==='ADBE Vector Shape - Star') {
                        const c=read(node,'position',[0,0]);
                        let half;
                        if(m==='ADBE Vector Shape - Star') { const r=read(node,'outerRadius',0); half=[r,r]; }
                        else { const size=read(node,'size',[0,0]); half=[size[0]/2,size[1]/2]; }
                        const e=extents?stroke/2:0;
                        for(const [dx,dy] of [[-1,-1],[1,-1],[1,1],[-1,1]]) {
                            const q=__aeApply(matrix,[c[0]+dx*(half[0]+e),c[1]+dy*(half[1]+e)]);
                            add(q[0],q[1]);
                        }
                    } else if(m==='ADBE Vector Shape - Group') {
                        __aeFail('sourceRectAtTime over free-form Shape paths is not admitted');
                    }
                }
            }
            visit(nativeLayer.content,[1,0,0, 0,1,0, 0,0,1, 0,0,0]);
            if(box===null) __aeFail('Shape layer has no measurable geometry');
            const rect={top:box[1],left:box[0],width:box[2]-box[0],height:box[3]-box[1]};
            return new Proxy(rect,{get:(target,name)=>{
                if(name===__aeToken||name===Symbol.toPrimitive) __aeFail('sourceRectAtTime() is not a value');
                if(!(name in target)) __aeFail('unsupported sourceRectAtTime member '+String(name));
                return target[name];
            }});
        }
        function contentItem(items,selector) {
            const node=typeof selector==='number'
                ? (Number.isInteger(selector) && selector>=1 && selector<=items.length ? items[selector-1] : null)
                : unique(items,item=>item.name===selector||item.match_name===selector,'Contents item '+selector);
            if(!node) __aeFail('missing Contents item '+selector);
            return contentObject(node);
        }
        // Named Contents members resolve against native children; unknown names fail.
        function contentObject(node) {
            if(node.slot!==null) return property(node.slot);
            const member=name=>{
                if(name==='name') return node.name;
                if(name==='content') {
                    const list=node.children.find(child=>child.alias==='content');
                    return selector=>contentItem(list ? list.children : node.children,selector);
                }
                const child=node.children.find(child=>child.alias===name);
                if(!child) __aeFail('unsupported Contents member '+String(name)+' of '+node.name);
                return contentObject(child);
            };
            return new Proxy({}, {get:(_,name)=>{
                if(name===__aeToken || name===Symbol.toPrimitive) __aeFail('Contents groups are not values');
                return member(name);
            }, has:()=>false});
        }
        function maskItem(nativeLayer,selector) {
            const mask=typeof selector==='number'
                ? nativeLayer.masks.find(m=>m.index===selector)
                : unique(nativeLayer.masks,m=>m.name===selector,'mask '+selector);
            if(!mask) __aeFail('missing mask '+selector);
            return new Proxy({}, {get:(_,name)=>{
                if(name==='name') return mask.name;
                const slot=mask.properties[name];
                if(slot===undefined) __aeFail('unsupported mask member '+String(name));
                return property(slot);
            }, has:()=>false});
        }
        function composition(nativeComp) {
            return {width:nativeComp.width,height:nativeComp.height,duration:nativeComp.duration,frameDuration:1/nativeComp.fps,
                displayStartTime:nativeComp.display_start,
                layer:selector=>layer(unique(nativeComp.layers,l=>typeof selector==='number'?l.index===selector:l.name===selector,'layer'))};
        }
        const thisComp=composition(c),thisLayer=layer(l),thisProperty=property(slot,true),transform=thisLayer.transform;
        const index=l.index,inPoint=l.in_point,outPoint=l.out_point;
        const effect=thisLayer.effect;
        const width=thisLayer.width,height=thisLayer.height,name=thisLayer.name,hasParent=thisLayer.hasParent;
        const toComp=thisLayer.toComp,fromComp=thisLayer.fromComp,toWorld=thisLayer.toWorld,fromWorld=thisLayer.fromWorld;
        const content=thisLayer.content,mask=thisLayer.mask,text=thisLayer.text;
        const anchorPoint=transform.anchorPoint,position=transform.position,scale=transform.scale,rotation=transform.rotation,opacity=transform.opacity;
        const active=thisLayer.active;
        const parent=hasParent ? thisLayer.parent : undefined;
        const pre=t=>authored(slot,t);
        const velocity=p.text===null?__aeDerivative(pre,time):0, speed=p.text===null?__aeSpeed(velocity):0;
        function smooth(width=0.2,samples=5,t=time){return __aeSmooth(pre,scalar(width),scalar(samples),scalar(t));}
        // AE's ease family is a cubic Hermite: ease has zero end slopes, easeIn a
        // unit slope at its end and easeOut a unit slope at its start.
        function ease(t,...args){return eased(t,args,u=>u*u*(3-2*u));}
        function easeIn(t,...args){return eased(t,args,u=>u*u*(3-2*u)+u*u*(u-1));}
        function easeOut(t,...args){return eased(t,args,u=>u*u*(3-2*u)+u*(1-u)*(1-u));}
        function eased(t,args,curve) {
            t=scalar(t);let a,b,start,end;
            if(args.length===2) [a,b,start,end]=[0,1,args[0],args[1]];
            else if(args.length===4) [a,b,start,end]=args;
            else __aeFail('ease requires three or five arguments');
            a=scalar(a);b=scalar(b);
            if(a>=b)__aeFail('nonincreasing ease bounds are not admitted');
            const u=curve(Math.max(0,Math.min(1,(t-a)/(b-a))));
            return __aeAdd(start,__aeMul(__aeSub(end,start),u));
        }
        function degreesToRadians(d){return __aeMul(d,Math.PI/180);}
        function radiansToDegrees(r){return __aeMul(r,180/Math.PI);}
        function dot(a,b){a=finite(a);b=finite(b);if(!Array.isArray(a)||!Array.isArray(b)||a.length!==b.length)__aeFail('dot requires equal vectors');return a.reduce((s,v,i)=>s+v*b[i],0);}
        function cross(a,b){a=finite(a);b=finite(b);if(!Array.isArray(a)||!Array.isArray(b)||a.length!==3||b.length!==3)__aeFail('cross requires 3D vectors');return [a[1]*b[2]-a[2]*b[1],a[2]*b[0]-a[0]*b[2],a[0]*b[1]-a[1]*b[0]];}
        function lookAt(from,at) {
            from=finite(from);at=finite(at);
            if(!Array.isArray(from)||!Array.isArray(at)||from.length!==3||at.length!==3)__aeFail('lookAt requires 3D points');
            const d=[at[0]-from[0],at[1]-from[1],at[2]-from[2]];
            // Orientation X/Y aiming the layer's Z axis at the target; roll is 0.
            const x=-Math.atan2(d[1],d[2]), y=Math.atan2(d[0],Math.hypot(d[1],d[2]));
            return [x*180/Math.PI,y*180/Math.PI,0];
        }
        function rgbToHsl(c) {
            c=finite(c);if(!Array.isArray(c)||c.length<3)__aeFail('rgbToHsl requires RGB(A)');
            const [r,g,b]=c,max=Math.max(r,g,b),min=Math.min(r,g,b),l=(max+min)/2;
            let h=0,s=0;
            if(max!==min){const d=max-min;s=l>0.5?d/(2-max-min):d/(max+min);
                h=max===r?(g-b)/d+(g<b?6:0):max===g?(b-r)/d+2:(r-g)/d+4;h/=6;}
            return [h,s,l,c.length>3?c[3]:1];
        }
        function hslToRgb(c) {
            c=finite(c);if(!Array.isArray(c)||c.length<3)__aeFail('hslToRgb requires HSL(A)');
            const [h,s,l]=c;
            const f=(p,q,t)=>{t=((t%1)+1)%1;return t<1/6?p+(q-p)*6*t:t<1/2?q:t<2/3?p+(q-p)*(2/3-t)*6:p;};
            if(s===0)return [l,l,l,c.length>3?c[3]:1];
            const q=l<0.5?l*(1+s):l+s-l*s,p=2*l-q;
            return [f(p,q,h+1/3),f(p,q,h),f(p,q,h-1/3),c.length>3?c[3]:1];
        }
        function comp(name) {
            const other=unique(model.comps,c=>c.name===name,'foreign composition');
            if(!model.allow_foreign) __aeFail('comp(name) references with occurrence overrides are not admitted');
            return composition(other);
        }
        function valueAtTime(t) { return authored(slot,scalar(t)); }
        const numKeys=p.keys.length;
        const key=index=>nativeKey(slot,scalar(index));
        const nearestKey=t=>nearestNativeKey(slot,scalar(t));
        const add=__aeAdd,sub=__aeSub,mul=__aeMul,div=__aeDiv;
        function clamp(v,a,b) {
            a=scalar(a);b=scalar(b);if(a>b)__aeFail('reversed clamp bounds are not admitted');
            v=finite(v);const f=x=>Math.min(b,Math.max(a,x));return Array.isArray(v)?v.map(f):f(v);
        }
        function linear(t,...args) {
            t=scalar(t);let a,b,start,end;
            if(args.length===2) [a,b,start,end]=[0,1,args[0],args[1]];
            else if(args.length===4) [a,b,start,end]=args;
            else __aeFail('linear requires three or five arguments');
            a=scalar(a);b=scalar(b);
            if(a>=b)__aeFail('nonincreasing linear bounds are not admitted');
            const u=Math.max(0,Math.min(1,(t-a)/(b-a)));
            return __aeAdd(start,__aeMul(__aeSub(end,start),u));
        }
        function posterizeTime(fps) {
            fps=scalar(fps);if(fps<=0)__aeFail('nonpositive posterizeTime rate is not admitted');
            time=Math.floor(requestedTime*fps)/fps;value=authored(slot,time);
        }
        function length(a,b) {
            a=finite(a);if(!Array.isArray(a))__aeFail('length requires vectors');
            if(b!==undefined) a=__aeSub(a,b);
            return Math.sqrt(a.reduce((sum,x)=>sum+x*x,0));
        }
        function normalize(a) { a=finite(a);const n=length(a);if(n===0)__aeFail('zero-vector normalization is not admitted');return __aeDiv(a,n); }
        function framesToTime(frames,fps=c.fps) { fps=scalar(fps);if(fps<=0)__aeFail('nonpositive framesToTime rate');return scalar(frames)/fps; }
        function timeToFrames(t=time+c.display_start,fps=c.fps,isDuration=false) {
            const frames=scalar(t)*scalar(fps);if(fps<=0)__aeFail('nonpositive frame rate');
            return isDuration?(frames<0?Math.floor(frames):Math.ceil(frames)):Math.floor(frames);
        }
        function loop(direction,type='cycle',count=0,duration=null) {
            const keys=p.keys;
            if(keys.length<2)__aeFail('loop requires at least two keys');
            if(!Number.isInteger(count)||count<0||count>=keys.length)__aeFail('unsupported loop key count');
            if(!['cycle','pingpong','offset','continue'].includes(type))__aeFail('unsupported loop mode');
            const first=direction==='out'?(count===0?0:keys.length-1-count):0;
            const last=direction==='in'?(count===0?keys.length-1:count):keys.length-1;
            let a=keys[first],b=keys[last];
            if(duration!==null) {
                duration=scalar(duration);
                const whole=keys[keys.length-1].time-keys[0].time;
                if(duration<=0||duration>whole) duration=whole;
                a=direction==='out'?{time:keys[keys.length-1].time-duration}:keys[0];
                b=direction==='out'?keys[keys.length-1]:{time:keys[0].time+duration};
            }
            const span=b.time-a.time;
            const outside=direction==='out'?time>b.time:time<a.time;
            if(!outside)return authored(slot,time);
            if(type==='continue') {
                // A Linear derivative exists; Hold/endpoint discontinuities are declined.
                const left=direction==='out'?keys[keys.length-2]:keys[0],right=direction==='out'?keys[keys.length-1]:keys[1];
                if(left.hold)__aeFail('Hold loop continue derivative is not admitted');
                const slope=__aeDiv(__aeSub(right.value.length===1?right.value[0]:right.value,left.value.length===1?left.value[0]:left.value),right.time-left.time);
                const boundary=direction==='out'?b.time:a.time;
                return __aeAdd(authored(slot,boundary),__aeMul(slope,time-boundary));
            }
            const cycles=Math.floor((time-a.time)/span);
            let local=(time-a.time)-cycles*span;
            if(type==='pingpong' && ((cycles%2)+2)%2===1)local=span-local;
            let result=authored(slot,a.time+local);
            if(type==='offset') result=__aeAdd(result,__aeMul(__aeSub(authored(slot,b.time),authored(slot,a.time)),cycles));
            return result;
        }
        function loopOut(type,count) { return loop('out',type,count); }
        function loopIn(type,count) { return loop('in',type,count); }
        function loopOutDuration(type='cycle',duration=0) { return loop('out',type,0,duration); }
        function loopInDuration(type='cycle',duration=0) { return loop('in',type,0,duration); }
        // Each property owns a fresh seeded stream; dependency calls mark the same
        // sample report, including dependencies satisfied through the memo table.
        const planarPosition=!l.three_d && p.identity.kind==='transform'
            && p.identity.match_name==='ADBE Position' && p.initial.length===3
            && p.initial[2]===0 && p.keys.every(k=>k.value[2]===0);
        // Only admitted unshadowed global calls are rewritten to these private names.
        // Do not reserve native API names in the direct-eval lexical environment:
        // deterministic user locals may legitimately use those names.
        const {seedRandom:__aeSeedRandom,random:__aeRandomValue,
            gaussRandom:__aeGaussRandom,wiggle:__aeWiggle,noise:__aeNoise}=__aeRandom(
            p.comp_id+':'+p.layer_id+':'+JSON.stringify(p.identity),
            ()=>time,t=>{const v=authored(slot,t);return planarPosition?v.slice(0,2):v;},
            api=>approximated.add(api),scalar,finite,__aeFail);
        // Strict direct eval keeps source declarations in a fresh eval environment:
        // a caught failure cannot shadow or overwrite the trusted sticky failure.
        const raw=p.expression===null?value:eval('"use strict";\n'+p.expression);
        // Source Text results are strings (AE coerces numbers and documents).
        if(p.text!==null) {
            if(failure!==null) throw failure;
            const text=String(__aeUnwrap(raw));
            memo.set(cacheKey,text);return text;
        }
        let result=finite(raw);
        // Independently captured 2D Position accepts XY and exposes native XYZ.
        // Do not generalize this to 3D layers, Scale, or other vector properties.
        if(planarPosition && Array.isArray(result) && result.length===2) result=[result[0],result[1],0];
        // AE fills components an expression omits from the pre-expression value
        // (e.g. a 2D Scale result on a 3D Scale keeps its Z).
        else if(Array.isArray(result) && Array.isArray(value) && result.length<value.length) result=result.concat(value.slice(result.length));
        if(p.range!==null) {
            const [low,high]=p.range;
            const clamp=x=>Math.min(high===null?Infinity:high,Math.max(low===null?-Infinity:low,x));
            result=Array.isArray(result)?result.map(clamp):(typeof result==='number'?clamp(result):result);
        }
        if(failure!==null) throw failure;
        memo.set(cacheKey,result);return result;
    } catch(error) { failure=error;throw error; } finally { active.delete(slot); }
}
const value=evaluate(input.slot,input.seconds);
return {value,approximated:Array.from(approximated).sort()};
