var t = input.time.seconds;
var w = scalar(C[0], t) / 2, h = scalar(C[1], t) / 2;
var x = scalar(C[2], t), y = scalar(C[3], t);
var r = Math.max(0, scalar(C[4], t));
var commands = [
    { type: 'moveTo', x: x+w, y: y-h, cornerRadius: r },
    { type: 'lineTo', x: x+w, y: y+h, cornerRadius: r },
    { type: 'lineTo', x: x-w, y: y+h, cornerRadius: r },
    { type: 'lineTo', x: x-w, y: y-h, cornerRadius: r },
    { type: 'close' }
];
if (r === 0) {
    for (var i = 0; i < 4; i++) delete commands[i].cornerRadius;
}
return { commands: commands };
