import { readFileSync } from 'node:fs';
import assert from 'node:assert/strict';
import test from 'node:test';

const root = new URL('../crates/aftereffects_file/src/structure_document/', import.meta.url);
const sampler = readFileSync(new URL('sample-scalar.js', root), 'utf8');
const rectangle = readFileSync(new URL('shapes/rectangle.js', root), 'utf8');
const constant = base => ({ base, keys: [] });
const key = (t, v, e = { type: 'linear' }, si = null, so = null) => ({ t, v, e, si, so });
const channel = keys => ({ base: 999, keys });
const sample = new Function('channel', 'time', `${sampler}; return scalar(channel, time);`);
const rect = new Function('C', 'input', `${sampler}\n${rectangle}`);

test('size, position and roundness are independent source-local channels', () => {
    const c = [channel([key(0, 100), key(2, 200)]), constant(80),
        channel([key(0, 0), key(2, 20)]), constant(-5), channel([key(0, 0), key(2, 12)])];
    const path = rect(c, { time: { seconds: 1 } });
    assert.deepEqual(path.commands[0], { type: 'moveTo', x: 85, y: -45, cornerRadius: 6 });
    assert.deepEqual(path.commands[2], { type: 'lineTo', x: -65, y: 35, cornerRadius: 6 });
    assert.equal(path.commands.at(-1).type, 'close');
});

test('Hold changes exactly at keys, including descending local-time queries', () => {
    const c = channel([key(-1, 10), key(1, 20, { type: 'hold' }), key(2, 30)]);
    assert.equal(sample(c, 1), 20);
    assert.equal(sample(c, 1 - 1e-8), 10);
    assert.equal(sample(c, -2), 10);
    assert.equal(sample(c, 3), 30);
});

test('asymmetric Bezier handles are used, not linearized', () => {
    const c = channel([key(0, 0), key(1, 100, { type: 'cubicBezier', x1: 1/3, y1: 0, x2: 2/3, y2: 0 })]);
    assert.ok(Math.abs(sample(c, 0.5) - 12.5) < 1e-10);
});

test('scalar spatial controls remain cubic offsets from endpoints', () => {
    const c = channel([key(0, 0, undefined, null, 20), key(1, 100, undefined, -40, null)]);
    assert.equal(sample(c, 0.5), 42.5);
});

test('empty channels retain static values and negative roundness is clamped', () => {
    assert.equal(sample(constant(37), 1e6), 37);
    const path = rect([100, 100, 0, 0, -4].map(constant), { time: { seconds: 0 } });
    assert.ok(path.commands.slice(0, 4).every(command => !Object.hasOwn(command, 'cornerRadius')),
        'zero roundness must inherit a later Round Corners modifier');
});
