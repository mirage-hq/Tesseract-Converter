/* Independent Adobe readback of fresh exports. No converter reader is used.
 * PATH_KEY_READBACK_CONFIG = {inputs:[{name,path}], receipt}. Requires empty AE.
 */
(function () {
    var config=PATH_KEY_READBACK_CONFIG;
    function encode(v) {
        if(v===null || v===undefined) return 'null';
        if(typeof v==='string') return '"'+v.replace(/\\/g,'\\\\').replace(/"/g,'\\"').replace(/\r/g,'\\r').replace(/\n/g,'\\n').replace(/\t/g,'\\t')+'"';
        if(typeof v==='number' || typeof v==='boolean') return String(v);
        var a=[],k;
        if(v instanceof Array) {for(k=0;k<v.length;k++) a.push(encode(v[k]));return '['+a.join(',')+']';}
        for(k in v) if(v.hasOwnProperty(k)) a.push(encode(k)+':'+encode(v[k]));
        return '{'+a.join(',')+'}';
    }
    function write(result) {
        var file=new File(config.receipt);file.encoding='UTF-8';
        if(!file.open('w')) throw new Error('Cannot write readback receipt');
        file.write(encode(result));file.close();
    }
    function clean() {var p=app.project;if(p && (p.file!==null || p.dirty || p.numItems!==0)) throw new Error('Existing session is not owned/empty');}
    function value(s) {return {vertices:s.vertices,inTangents:s.inTangents,outTangents:s.outTangents,closed:s.closed};}
    function interp(v) {return v===KeyframeInterpolationType.LINEAR?'linear':v===KeyframeInterpolationType.HOLD?'hold':v===KeyframeInterpolationType.BEZIER?'bezier':String(v);}
    function visit(group,trail,out,layer) {
        for(var i=1;i<=group.numProperties;i++) {
            var p=group.property(i), path=trail+'/'+p.matchName;
            if(p.matchName==='ADBE Vector Shape' || p.matchName==='ADBE Mask Shape') {
                var keys=[];
                for(var k=1;k<=p.numKeys;k++) keys.push({time:p.keyTime(k),value:value(p.keyValue(k)),incoming:interp(p.keyInInterpolationType(k)),outgoing:interp(p.keyOutInterpolationType(k)),
                    inEase:{speed:p.keyInTemporalEase(k)[0].speed,influence:p.keyInTemporalEase(k)[0].influence},outEase:{speed:p.keyOutTemporalEase(k)[0].speed,influence:p.keyOutTemporalEase(k)[0].influence}});
                out.push({path:path,layer:layer.name,property:p.matchName,keys:keys,staticValue:p.numKeys?null:value(p.value)});
            } else if(p.propertyType!==PropertyType.PROPERTY) visit(p,path,out,layer);
        }
    }
    var result={adobeVersion:app.version,adobeBuild:app.buildNumber,completed:false,projects:[]};
    if(new File(config.receipt).exists) return;
    try {
        clean();
        for(var n=0;n<config.inputs.length;n++) {
            clean();var spec=config.inputs[n], owned=app.open(new File(spec.path));
            if(!owned || app.project!==owned) throw new Error('Open did not establish owned project');
            var item={name:spec.name,compositions:[]};result.projects.push(item);write(result);
            for(var i=1;i<=owned.numItems;i++) {
                var c=owned.item(i);if(!(c instanceof CompItem)) continue;
                var row={id:c.id,name:c.name,width:c.width,height:c.height,duration:c.duration,fps:c.frameRate,paths:[]};
                for(var l=1;l<=c.numLayers;l++) visit(c.layer(l),c.layer(l).name,row.paths,c.layer(l));
                item.compositions.push(row);
            }
            write(result);
            if(app.project!==owned) throw new Error('Project ownership changed');
            owned.close(CloseOptions.DO_NOT_SAVE_CHANGES);
        }
        result.completed=true;
    } catch(error) {result.error=error.toString();result.line=error.line;}
    write(result);
})();
