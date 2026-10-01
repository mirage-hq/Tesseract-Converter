// Optional CPU valueAtTime proof, invoked only with AEP_PATH_NODE explicitly set.
// The oracle is immutable upstream AE output, not a converter-generated baseline.
const assert = require('node:assert/strict');
const { code, property } = JSON.parse(require('node:fs').readFileSync(0, 'utf8'));
const evaluate = new Function('input', code);
function close(actual, expected, label) {
    assert.ok(Number.isFinite(actual) && Math.abs(actual - expected) <= 0.0002,
        `${label}: actual=${actual}, native=${expected}`);
}
for (const frame of property.frames) {
    const actual = evaluate({ time: { seconds: frame.time } }).commands;
    const shape = frame.value, vertices = shape.vertices;
    close(actual[0].x, vertices[0][0], `t=${frame.time} start.x`);
    close(actual[0].y, vertices[0][1], `t=${frame.time} start.y`);
    const segments = shape.closed ? vertices.length : vertices.length - 1;
    assert.equal(actual.length, 1 + segments + (shape.closed ? 1 : 0));
    for (let i = 0; i < segments; i++) {
        const next = (i + 1) % vertices.length, c = actual[i + 1];
        assert.equal(c.type, 'cubicTo');
        for (const [axis, coordinate] of [[0, 'x'], [1, 'y']]) {
            close(c[coordinate], vertices[next][axis], `t=${frame.time} end.${coordinate}`);
            close(c['c1' + coordinate], vertices[i][axis] + shape.outTangents[i][axis], `t=${frame.time} c1.${coordinate}`);
            close(c['c2' + coordinate], vertices[next][axis] + shape.inTangents[next][axis], `t=${frame.time} c2.${coordinate}`);
        }
    }
}
