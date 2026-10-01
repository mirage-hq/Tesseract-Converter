/* Independently Adobe-authored Path proof. No converter output is consumed.
 * Caller supplies PATH_KEY_PROOF_CONFIG {directory, receipt}. Native 24fps stays
 * unchanged; 30fps is applied separately by the native render settings.
 */
(function () {
    var config = PATH_KEY_PROOF_CONFIG;
    function encode(v) {
        if (v === null || v === undefined) return 'null';
        if (typeof v === 'string') return '"' + v.replace(/\\/g,'\\\\').replace(/"/g,'\\"').replace(/\r/g,'\\r').replace(/\n/g,'\\n').replace(/\t/g,'\\t') + '"';
        if (typeof v === 'number' || typeof v === 'boolean') return String(v);
        var parts=[], k;
        if (v instanceof Array) { for(k=0;k<v.length;k++) parts.push(encode(v[k])); return '['+parts.join(',')+']'; }
        for(k in v) if(v.hasOwnProperty(k)) parts.push(encode(k)+':'+encode(v[k]));
        return '{'+parts.join(',')+'}';
    }
    function write(file, value) {
        file.encoding='UTF-8';
        if(!file.open('w')) throw new Error('Cannot write owned receipt');
        file.write(encode(value)); file.close();
    }
    function emptySession() {
        var p=app.project;
        if(p && (p.file !== null || p.dirty || p.numItems !== 0)) throw new Error('Existing project is not an owned empty session');
    }
    function outline(index,mask,edited) {
        var sets=[
            [[-130,-65],[25,-85],[125,55]],
            [[-115,-45],[55,-65],[145,45]],
            [[-105,-75],[55,-90],[130,20],[-45,85]],
            [[-135,-30],[10,-100],[145,55],[-65,100]],
            [[-80,-100],[90,-55],[110,85],[-110,55]]
        ];
        var s=new Shape(), points=[], incoming=[], outgoing=[], base=sets[index];
        for(var v=0;v<base.length;v++) {
            points.push([base[v][0]+(mask?320:0)+(edited?17:0),base[v][1]+(mask?180:0)-(edited?9:0)]);
            incoming.push(v===1?[-25,12]:[0,0]);
            outgoing.push(v===0?[35,-18]:[0,0]);
        }
        s.vertices=points; s.inTangents=incoming; s.outTangents=outgoing;
        s.closed=mask || index>=2;
        return s;
    }
    function interp(value) {
        if(value===KeyframeInterpolationType.LINEAR) return 'linear';
        if(value===KeyframeInterpolationType.HOLD) return 'hold';
        if(value===KeyframeInterpolationType.BEZIER) return 'bezier';
        return String(value);
    }
    function shapeValue(s) {return {vertices:s.vertices,inTangents:s.inTangents,outTangents:s.outTangents,closed:s.closed};}
    function readKeys(p) {
        var keys=[];
        for(var k=1;k<=p.numKeys;k++) keys.push({time:p.keyTime(k),value:shapeValue(p.keyValue(k)),
            incoming:interp(p.keyInInterpolationType(k)),outgoing:interp(p.keyOutInterpolationType(k)),
            inEase:{speed:p.keyInTemporalEase(k)[0].speed,influence:p.keyInTemporalEase(k)[0].influence},
            outEase:{speed:p.keyOutTemporalEase(k)[0].speed,influence:p.keyOutTemporalEase(k)[0].influence}});
        return keys;
    }
    var receipt=new File(config.receipt);
    if(receipt.exists) return;
    var result={purpose:'independent Adobe Path source and edited-oracle authoring',adobeVersion:app.version,adobeBuild:app.buildNumber,cases:[],completed:false};
    try {
        emptySession();
        for(var index=0;index<4;index++) {
            var mask=index%2===1, edited=index>=2;
            var name=(mask?'mask':'shape')+(edited?'-edited-oracle':'')+'-v2';
            var source=new File(config.directory+'/'+name+'.aep');
            if(source.exists) throw new Error('Refusing immutable source overwrite: '+name);
            emptySession();
            var owned=app.newProject();
            owned.bitsPerChannel=8;
            var comp=owned.items.addComp('PATH_PROOF_'+name,640,360,1,5,24);
            comp.bgColor=[0,0,0];
            var layer, p;
            if(mask) {
                layer=comp.layers.addSolid([0.95,0.35,0.15],'MASK_CONTENT',640,360,1,5);
                var atom=layer.property('ADBE Mask Parade').addProperty('ADBE Mask Atom');
                atom.name='Authored Path'; atom.maskMode=MaskMode.ADD;
                p=atom.property('ADBE Mask Shape');
            } else {
                layer=comp.layers.addShape(); layer.name='SHAPE_CONTENT';
                var group=layer.property('ADBE Root Vectors Group').addProperty('ADBE Vector Group');
                group.name='Authored Path';
                var contents=group.property('ADBE Vectors Group');
                var pathGroup=contents.addProperty('ADBE Vector Shape - Group');
                // Resolve again after adding a sibling: indexed-group handles can invalidate.
                var stroke=contents.addProperty('ADBE Vector Graphic - Stroke');
                stroke.property('ADBE Vector Stroke Color').setValue([0.2,0.8,1]);
                stroke.property('ADBE Vector Stroke Width').setValue(8);
                p=contents.property(1).property('ADBE Vector Shape');
                layer.property('ADBE Transform Group').property('ADBE Position').setValue([320,180]);
            }
            var times=[edited?0.125:0,0.5,1.5,2.5,4.5];
            for(var k=0;k<times.length;k++) {
                p.setValueAtTime(times[k],outline(k,mask,edited));
                p.setInterpolationTypeAtKey(k+1,KeyframeInterpolationType.LINEAR,KeyframeInterpolationType.LINEAR);
            }
            p.setInterpolationTypeAtKey(2,KeyframeInterpolationType.LINEAR,KeyframeInterpolationType.HOLD);
            p.setInterpolationTypeAtKey(3,KeyframeInterpolationType.HOLD,KeyframeInterpolationType.BEZIER);
            p.setTemporalEaseAtKey(3,[new KeyframeEase(0,35)],[new KeyframeEase(0,70)]);
            p.setInterpolationTypeAtKey(4,KeyframeInterpolationType.BEZIER,KeyframeInterpolationType.LINEAR);
            p.setTemporalEaseAtKey(4,[new KeyframeEase(0,35)],[new KeyframeEase(0,16.6666667)]);
            p.setInterpolationTypeAtKey(5,KeyframeInterpolationType.BEZIER,KeyframeInterpolationType.LINEAR);
            p.setTemporalEaseAtKey(5,[new KeyframeEase(0,80)],[new KeyframeEase(0,16.6666667)]);
            // AE's ease setter promotes both sides to Bezier. Restore the
            // intended interpolation AFTER the ease values have been authored.
            p.setInterpolationTypeAtKey(3,KeyframeInterpolationType.HOLD,KeyframeInterpolationType.BEZIER);
            p.setInterpolationTypeAtKey(4,KeyframeInterpolationType.BEZIER,KeyframeInterpolationType.LINEAR);
            p.setInterpolationTypeAtKey(5,KeyframeInterpolationType.BEZIER,KeyframeInterpolationType.LINEAR);
            if(p.keyOutInterpolationType(4)!==KeyframeInterpolationType.LINEAR || p.keyInInterpolationType(5)!==KeyframeInterpolationType.BEZIER) throw new Error('Mixed ease was not authored');
            if(app.project !== owned) throw new Error('Project ownership changed');
            owned.save(source);
            result.cases.push({name:name,source:source.name,compositionId:comp.id,compositionName:comp.name,
                width:comp.width,height:comp.height,fps:comp.frameRate,duration:comp.duration,
                layerName:layer.name,property:p.matchName,keys:readKeys(p)});
            write(receipt,result);
            owned.close(CloseOptions.DO_NOT_SAVE_CHANGES);
        }
        result.completed=true;
    } catch(error) {
        result.error=error.toString(); result.line=error.line;
        // Leave any failed owned project intact for diagnosis; never discard it.
    }
    write(receipt,result);
})();
