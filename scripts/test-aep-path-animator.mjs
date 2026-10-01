// CPU-only tests of the exact generated path evaluator. This is not Adobe or
// Boa rendering proof; Rust tests separately pin native decoding and graph IDs.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';

const body = readFileSync(new URL('../crates/aftereffects_file/src/structure_document/shapes/path/evaluate.js', import.meta.url), 'utf8');
const evaluate = new Function('K', 't', body);
const path = x => ({ commands: [
  { type: 'moveTo', x, y: 0 },
  { type: 'cubicTo', c1x: x + 10, c1y: 20, c2x: x + 30, c2y: 40, x: x + 50, y: 60 },
  { type: 'close' },
] });
const keys = ease => [{ t: 0, p: path(100), e: null }, { t: 5, p: path(150), e: ease }];

test('linear path morph interpolates every control and preserves topology', () => {
  assert.deepEqual(evaluate(keys(null), 2.5), path(125));
  assert.deepEqual(evaluate(keys(null), -1), path(100));
  assert.deepEqual(evaluate(keys(null), 6), path(150));
  assert.deepEqual(evaluate(keys(null), 5), path(150));
});

test('native dimensionless temporal ease is not rescaled by path distance or duration', () => {
  const p = evaluate(keys([0, 0, 1, 0]), 2.5);
  assert.ok(Math.abs(p.commands[0].x - 106.25) < 1e-9);
  assert.equal(p.commands[1].c1y, 20);
});

test('inverse parent clock preserves reverse Hold boundary values', () => {
  const atParent = time => evaluate(keys('hold'), (time - 10) / -2);
  assert.deepEqual(atParent(0), path(150));
  assert.deepEqual(atParent(0.000001), path(100));
  assert.deepEqual(atParent(10), path(100));
});

test('interior key boundaries select the authored value rather than previous Hold', () => {
  const k = [...keys('hold'), { t: 10, p: path(200), e: 'hold' }];
  assert.deepEqual(evaluate(k, 5), path(150));
  assert.deepEqual(evaluate(k, 4.999), path(100));
  assert.deepEqual(evaluate(k, 5.001), path(150));
});
