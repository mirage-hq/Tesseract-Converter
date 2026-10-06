function value(group, name) {
    var p = group.property(name);
    if (!p) return null;
    return {matchName:p.matchName, value:p.value, keys:p.numKeys, expressionEnabled:p.expressionEnabled};
}
function transform(layer) {
    var t = layer.property("ADBE Transform Group"), names=[];
    for (var i=1;i<=t.numProperties;i++) names.push(t.property(i).matchName);
    return {inventory:names, scale:value(t,"ADBE Scale"), rotation:value(t,"ADBE Rotate Z"), position:value(t,"ADBE Position"), opacity:value(t,"ADBE Opacity"), skewPresent:!!t.property("ADBE Skew"), skewAxisPresent:!!t.property("ADBE Skew Axis"), autoOrient:Number(layer.autoOrient), autoOrientNone:layer.autoOrient===AutoOrientType.NO_AUTO_ORIENT};
}
function styles(layer) {
    var root=layer.property("ADBE Layer Styles"), rows=[];
    function walk(group, depth) {
        if (depth>5) throw new Error("style nesting limit");
        for (var i=1;i<=group.numProperties;i++) {
            var p=group.property(i);
            if (p.matchName==="dropShadow/enabled") rows.push({matchName:p.matchName, enabled:p.canSetEnabled?p.enabled:null});
            if (p.matchName==="dropShadow/distance" || p.matchName==="dropShadow/localLightingAngle" || p.matchName==="dropShadow/blur" || p.matchName==="dropShadow/opacity" || p.matchName==="dropShadow/useGlobalAngle") rows.push({matchName:p.matchName,value:p.value,keys:p.numKeys});
            if (p.propertyType!==PropertyType.PROPERTY) walk(p,depth+1);
        }
    }
    if (root) walk(root,0);
    return rows;
}
function open(key) {
    if (app.project) app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES);
    app.open(new File(context.assets[key]));
}
open("source");
var sourceComp=null;
for (var i=1;i<=app.project.numItems;i++) if (app.project.item(i) instanceof CompItem && app.project.item(i).id===1) sourceComp=app.project.item(i);
if (!sourceComp || sourceComp.numLayers!==1) throw new Error("independent source identity");
var l=sourceComp.layer(1), before=transform(l), e=l.property("ADBE Effect Parade").property("ADBE Drop Shadow");
if (!(l instanceof AVLayer) || !(l.source.mainSource instanceof SolidSource)) throw new Error("native AV Solid required");
var source={name:sourceComp.name,width:sourceComp.width,height:sourceComp.height,fps:sourceComp.frameRate,duration:sourceComp.duration,layerName:l.name,transform:before,direction:value(e,"ADBE Drop Shadow-0003"),distance:value(e,"ADBE Drop Shadow-0004"),softness:value(e,"ADBE Drop Shadow-0005")};
l.property("ADBE Transform Group").property("ADBE Scale").setValue([300,300,100]);
l.property("ADBE Transform Group").property("ADBE Rotate Z").setValue(0);
source.editedTransform=transform(l);
var generated={};
for (var k=0;k<2;k++) {
    var key=k===0?"fresh":"edited";open(key);var comps=[];
    if (app.project.numItems>64) throw new Error("item limit");
    for (var i=1;i<=app.project.numItems;i++) {
        var c=app.project.item(i);if (!(c instanceof CompItem)) continue;
        if (c.numLayers>64) throw new Error("layer limit");
        var layers=[];
        for (var j=1;j<=c.numLayers;j++) {var layer=c.layer(j);layers.push({name:layer.name,index:j,transform:transform(layer),styles:styles(layer),effects:layer.property("ADBE Effect Parade").numProperties});}
        comps.push({id:c.id,name:c.name,width:c.width,height:c.height,fps:c.frameRate,duration:c.duration,layers:layers});
    }
    generated[key]=comps;
}
return {result:{source:source,generated:generated},targets:[]};
