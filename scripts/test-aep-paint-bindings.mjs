import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import assert from 'node:assert/strict';
const root = new URL('../crates/aftereffects_file/src/structure_document/shapes/', import.meta.url);
const appendCode = readFileSync(new URL('append-outlines.js', root), 'utf8');
const parametricCode = readFileSync(new URL('parametric-outline.js', root), 'utf8');
const roundCode = readFileSync(new URL('round-outline.js', root), 'utf8');
const dep = value => ({ value });
const path = x => ({ commands: [{ type: 'moveTo', x, y: 0 }, { type: 'lineTo', x: x + 10, y: 10 }, { type: 'close' }] });
const append = (sources, values) => new Function('input', `var sources=${JSON.stringify(sources)};\n${roundCode}\n${appendCode}`)({ deps: values.map(dep) });
const primitive = (kind, values, reversed = false) => new Function('input', `var kind=${JSON.stringify(kind)},reversed=${reversed};\n${parametricCode}`)({ deps: values.map(dep) });

test('compound paints append contours rather than removing overlap boundaries', () => {
  const result = append([{path:0, transforms:[]},{path:1, transforms:[]}], [path(0), path(5)]);
  assert.equal(result.commands.length, 6);
  assert.equal(result.commands.filter(c => c.type === 'moveTo').length, 2);
});
test('independent consumers observe producer edits without mutating each other', () => {
  const source = path(0), config = [{path:0, transforms:[]}];
  const first = append(config,[source]); first.commands[0].x = 100;
  assert.equal(source.commands[0].x, 0);
  source.commands[0].x = 20;
  assert.equal(append(config,[source]).commands[0].x, 20);
  assert.equal(append(config,[source]).commands[0].x, 20);
});
test('nested vector transforms apply inner-to-outer, excluding opacity', () => {
  const source = {commands:[{type:'moveTo',x:4,y:5}]};
  const inner = [0,0,10,0,100,100,0,0,0], outer = [0,0,0,0,200,200,0,0,0];
  assert.deepEqual(append([{path:0,transforms:[1,10]}],[source,...inner,...outer]).commands[0], {type:'moveTo',x:28,y:10});
});
test('skew uses the existing negative-tangent convention and finite clamp', () => {
  const result=append([{path:0,transforms:[1]}],[{commands:[{type:'moveTo',x:0,y:1}]},0,0,0,0,100,100,0,45,0]);
  assert.ok(Math.abs(result.commands[0].x+1)<1e-12);
  const clamped=append([{path:0,transforms:[1]}],[path(0),0,0,0,0,100,100,0,90,0]);
  assert.ok(Number.isFinite(clamped.commands[1].x));
});
test('ellipse outlines start at the top and expose four cubic edges', () => {
  const result=primitive('ellipse',[[100,40],[10,20]]);
  assert.deepEqual(result.commands[0],{type:'moveTo',x:10,y:0});
  assert.equal(result.commands.filter(c=>c.type==='cubicTo').length,4);
  assert.equal(result.commands[1].x,60);
});
test('reversing an ellipse swaps handles and preserves the starting anchor', () => {
  const forward=primitive('ellipse',[[100,40],[10,20]]), reverse=primitive('ellipse',[[100,40],[10,20]],true);
  assert.deepEqual(reverse.commands[0],forward.commands[0]);
  assert.equal(reverse.commands[1].x,-40);
  assert.equal(reverse.commands[1].c1x,forward.commands[4].c2x);
});
test('rounded stars share parametric controls and close with a cubic', () => {
  const result=primitive('star',[[0,0],5,0,100,50,30,20]);
  assert.equal(result.commands.length,12);
  assert.equal(result.commands.filter(c=>c.type==='cubicTo').length,10);
  assert.equal(result.commands.at(-1).type,'close');
  assert.equal(primitive('ellipse',[[200,40],[10,20]]).commands[1].x,110);
});
test('reversed rounded stars preserve the first anchor and swap closing cubic handles', () => {
  const controls=[[0,0],5,0,100,50,30,20];
  const forward=primitive('star',controls), reverse=primitive('star',controls,true);
  assert.deepEqual(reverse.commands[0],forward.commands[0]);
  assert.equal(reverse.commands[1].x,forward.commands[9].x);
  assert.equal(reverse.commands[1].y,forward.commands[9].y);
  assert.equal(reverse.commands[1].c1x,forward.commands[10].c2x);
  assert.equal(reverse.commands[1].c2y,forward.commands[10].c1y);
});

test('noninteger point values use the integer fallback, including between keys', () => {
  for (const kind of ['star','polygon']) {
    for (const reversed of [false,true]) {
      const controls=[[10,20],4,0,100,50,30,20];
      const integer=primitive(kind,controls,reversed);
      controls[1]=4.5;
      assert.deepEqual(primitive(kind,controls,reversed),integer);
    }
  }
});

test('rounded rectangle corners are resolved before nonuniform scaling', () => {
  const source={commands:[{type:'moveTo',x:0,y:0,cornerRadius:10},
    {type:'lineTo',x:100,y:0,cornerRadius:10},{type:'lineTo',x:100,y:50,cornerRadius:10},
    {type:'lineTo',x:0,y:50,cornerRadius:10},{type:'close'}]};
  const result=append([{path:0,transforms:[1]}],[source,0,0,0,0,200,100,0,0,0]);
  const corners=result.commands.filter(command=>command.type==='cubicTo');
  assert.equal(corners.length,4);
  assert.equal(corners[0].x,200);
  assert.equal(corners[0].y,10);
  assert.ok(Math.abs(corners[0].c1x-(180+20*0.5522847498307936))<1e-9);
  assert.ok(result.commands.every(command=>command.cornerRadius===undefined));
  assert.equal(source.commands[0].cornerRadius,10);
});

test('reversed rounded Rectangle preserves the native trim anchor under scale', () => {
  const source={commands:[{type:'moveTo',x:100,y:0,cornerRadius:10},
    {type:'lineTo',x:0,y:0,cornerRadius:10},{type:'lineTo',x:0,y:50,cornerRadius:10},
    {type:'lineTo',x:100,y:50,cornerRadius:10},{type:'close'}]};
  const result=append([{path:0,transforms:[1]}],[source,0,0,0,0,200,100,0,0,0]);
  assert.deepEqual(result.commands[0],{type:'moveTo',x:200,y:10});
  assert.equal(result.commands[1].type,'cubicTo');
  assert.equal(result.commands[1].x,180);
  assert.equal(result.commands[1].y,0);
  assert.equal(result.commands.filter(command=>command.type==='cubicTo').length,4);
});

test('partial and repeated rounds run in their owning coordinate space without leaking', () => {
  const rect={commands:[{type:'moveTo',x:0,y:0},{type:'lineTo',x:100,y:0},
    {type:'lineTo',x:100,y:50},{type:'lineTo',x:0,y:50},{type:'close'}]};
  const config=[{path:0,transforms:[2],rounds:[[1],[11]]},{path:12,transforms:[]}];
  const values=[rect,12,0,0,0,0,200,100,0,0,0,30,rect];
  const result=append(config,values);
  assert.equal(result.commands.filter(c=>c.type==='cubicTo').length,4);
  assert.equal(result.commands[1].x,24,'inner radius12 is rounded before x2 scale; second round must not overwrite curves');
  assert.deepEqual(result.commands.slice(-5),rect.commands,'unaffected second contour stays sharp');
  values[1]=0;
  assert.equal(append(config,values).commands[1].x,25,'parent radius30 is clamped in scaled coordinates');
});

test('compound command budget rejects oversized output instead of truncation', () => {
  const source={commands:Array.from({length:10001},()=>({type:'close'}))};
  assert.throws(()=>append([{path:0,transforms:[]}],[source]),/10000 commands/);
});
