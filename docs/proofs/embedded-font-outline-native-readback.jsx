function require(ok, message) { if (!ok) throw new Error(message); }
function finite(x) { require(typeof x === 'number' && isFinite(x), 'Non-numeric control'); return x; }
function vector(x) { var r=[]; for(var i=0;i<x.length;i++) r.push(finite(x[i])); return r; }
var results=[];
for (var v=0;v<context.data.projects.length;v++) {
    var spec=context.data.projects[v];
    if (app.project) app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES);
    app.open(new File(context.assets[spec.asset]));
    for(var i=1;i<=app.project.numItems;i++) {
        var item=app.project.item(i);
        if(item instanceof FootageItem && item.file) {
            var path=item.file.fsName, key=spec.media[path];
            require(key!==undefined, 'Unpinned footage');
            item.replace(new File(context.assets[key]));
            require(!item.footageMissing,'Offline footage');
        }
    }
    var cards=[];
    for(var j=0;j<context.data.cards.length;j++) {
        var expected=context.data.cards[j], comp=null;
        for(var k=1;k<=app.project.numItems;k++) {
            var candidate=app.project.item(k);
            if(candidate instanceof CompItem && candidate.name===expected.comp) {
                require(comp===null,'Ambiguous composition'); comp=candidate;
            }
        }
        require(comp!==null && comp.numLayers===3,'Missing editable card');
        var label=null;
        for(var k=1;k<=comp.numLayers;k++) if(comp.layer(k).name===expected.label) {
            require(label===null,'Ambiguous label');label=comp.layer(k);
        }
        require(label!==null && label.property('ADBE Text Properties')!==null,'Missing native editable Text');
        // No CUSTOM_VALUE or Source Text .value read. All observations below
        // are native numeric geometry, Transform and key controls.
        var rect=label.sourceRectAtTime(0,false);
        var p=vector(label.property('ADBE Transform Group').property('ADBE Position').value);
        var a=vector(label.property('ADBE Transform Group').property('ADBE Anchor Point').value);
        var scale=vector(label.property('ADBE Transform Group').property('ADBE Scale').value);
        var opacity=label.property('ADBE Transform Group').property('ADBE Opacity');
        require(scale[0]===100 && scale[1]===100,'Unexpected Text scale');
        require(rect.width>0 && rect.height>0 && opacity.numKeys===3,'Invalid glyph geometry or lost opacity keys');
        var left=finite(rect.left)+p[0]-a[0],top=finite(rect.top)+p[1]-a[1];
        require(left>=-0.02 && top>=-0.02 && left+rect.width<=comp.width+0.02 && top+rect.height<=comp.height+0.02,'Glyphs escape enclosure');
        var values=[];
        for(var n=1;n<=opacity.numKeys;n++) values.push({time:finite(opacity.keyTime(n)),value:finite(opacity.keyValue(n))});
        cards.push({width:finite(comp.width),height:finite(comp.height),duration:finite(comp.duration),
            glyphLeft:finite(rect.left),glyphTop:finite(rect.top),glyphWidth:finite(rect.width),glyphHeight:finite(rect.height),
            position:p,anchor:a,opacityKeys:values});
    }
    results.push(cards);
}
require(results.length===2,'Missing edit-response pair');
require(Math.abs(results[1][0].glyphWidth/results[0][0].glyphWidth-10)<0.001,'Font-size edit width not preserved');
require(Math.abs(results[1][0].glyphHeight/results[0][0].glyphHeight-10)<0.001,'Font-size edit height not preserved');
require(results[1][0].width>results[0][0].width || results[1][0].height>results[0][0].height,'Bounds did not recompute after FX edit');
for(var j=1;j<4;j++) require(Math.abs(results[1][j].glyphWidth-results[0][j].glyphWidth)<0.001,'Unedited label changed');
return {result:{projects:results,numericOnly:true,editableTextCount:8,enclosureChecks:8,editResponse:true},targets:[]};
