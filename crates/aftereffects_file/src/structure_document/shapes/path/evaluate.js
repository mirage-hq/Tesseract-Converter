// K is validated native path data, not native AE expression source.
if (t <= K[0].t) return K[0].p;
if (t >= K[K.length - 1].t) return K[K.length - 1].p;
var lo = 0, hi = K.length - 1;
while (hi - lo > 1) {
    var mid = Math.floor((lo + hi) / 2);
    if (K[mid].t <= t) lo = mid; else hi = mid;
}
var a = K[lo], b = K[hi], e = b.e;
if (e === "hold") return a.p;
var u = (t - a.t) / (b.t - a.t);
function cubic(s, p, q) {
    var v = 1 - s;
    return 3 * v * v * s * p + 3 * v * s * s * q + s * s * s;
}
if (e !== null) {
    var l = 0, r = 1;
    for (var i = 0; i < 48; i++) {
        var s = (l + r) / 2;
        if (cubic(s, e[0], e[2]) < u) l = s; else r = s;
    }
    u = cubic((l + r) / 2, e[1], e[3]);
}
var result = [];
for (var j = 0; j < a.p.commands.length; j++) {
    var from = a.p.commands[j], to = b.p.commands[j], command = {};
    for (var field in from) {
        command[field] = typeof from[field] === "number"
            ? from[field] + (to[field] - from[field]) * u : from[field];
    }
    result.push(command);
}
return { commands: result };
