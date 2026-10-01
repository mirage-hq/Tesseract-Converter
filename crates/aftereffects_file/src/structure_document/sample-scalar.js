// Trusted decoded numeric data only. Time is the destination layer's local clock.
function cubic(a, b, c, d, t) {
    var s = 1 - t;
    return s*s*s*a + 3*s*s*t*b + 3*s*t*t*c + t*t*t*d;
}
function scalar(channel, time) {
    var keys = channel.keys;
    if (!keys.length) return channel.base;
    if (time <= keys[0].t) return keys[0].v;
    if (time >= keys[keys.length - 1].t) return keys[keys.length - 1].v;
    var lo = 0, hi = keys.length - 1;
    while (hi - lo > 1) {
        var middle = Math.floor((lo + hi) / 2);
        if (keys[middle].t <= time) lo = middle; else hi = middle;
    }
    var a = keys[lo], b = keys[hi];
    if (time === a.t || b.e.type === 'hold') return a.v;
    var p = (time - a.t) / (b.t - a.t);
    if (b.e.type === 'cubicBezier') {
        var left = 0, right = 1;
        for (var i = 0; i < 48; i++) {
            var u = (left + right) / 2;
            if (cubic(0, b.e.x1, b.e.x2, 1, u) < p) left = u; else right = u;
        }
        p = cubic(0, b.e.y1, b.e.y2, 1, (left + right) / 2);
    }
    if (a.so !== null || b.si !== null) {
        return cubic(a.v, a.v + (a.so || 0), b.v + (b.si || 0), b.v, p);
    }
    return a.v + (b.v - a.v) * p;
}
