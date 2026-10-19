"""Deterministic owned ICC v4 profiles from published color standards only.

PQ uses ITU-R BT.2100/ST2084 EOTF with an AE-calibrated203nit relative
PCS reference (peak10000nit). HLG uses
BT.2100 inverse OETF and luminance-coupled1000nit OOTF (gamma1.2). No Adobe
profile bytes are used. HLG v2 uses65^3 floating-point mpet CLUTs and
unclipped matrices at the calibrated203nit PCS reference. The conventional
33^3 XYZ16 fallback clips highlights above approximately406nits and negative
PCS coordinates (8191/35937 nodes). Native CMM selection is independently
probed; serialized transforms alone are not converter fidelity proof.
"""
import argparse
import hashlib
import math
import struct
from pathlib import Path

D50 = (0.9642, 1.0, 0.8249)
D65 = (0.9504559270516716, 1.0, 1.0890577507598784)
BT2020 = ((0.6369580483012914, 0.1446169035862083, 0.1688809751641721),
          (0.2627002120112671, 0.6779980715188708, 0.0593017164698620),
          (0.0, 0.0280726930490874, 1.0609850577107910))
SRGB = ((0.4123907992659595, 0.3575843393838780, 0.1804807884018343),
        (0.2126390058715104, 0.7151686787677560, 0.0721923153607337),
        (0.0193308187155918, 0.1191947797946260, 0.9505321522496607))
BRADFORD = ((0.8951, 0.2664, -0.1614), (-0.7502, 1.7135, 0.0367),
            (0.0389, -0.0685, 1.0296))
BRADFORD_INV = ((0.9869929054667123, -0.1470542564209901, 0.1599626516637312),
                (0.4323052697233945, 0.5183602715367776, 0.0492912282128556),
                (-0.0085286645751773, 0.0400428216540849, 0.9684866957875500))


def mul(matrix, vector):
    return tuple(sum(a * b for a, b in zip(row, vector)) for row in matrix)


def adapt(xyz, source=D65, destination=D50):
    src, dst = mul(BRADFORD, source), mul(BRADFORD, destination)
    return mul(BRADFORD_INV, tuple(v * d / s for v, d, s in zip(mul(BRADFORD, xyz), dst, src)))


def pq(value):
    p = max(value, 0.0) ** (1.0 / (2523.0 / 32.0))
    return (max(p - 3424.0 / 4096.0, 0.0) / (2413.0 / 128.0 - 2392.0 / 128.0 * p)) ** (1.0 / (2610.0 / 16384.0))


def hlg_scene(value):
    return value * value / 3.0 if value <= 0.5 else (math.exp((value - 0.55991073) / 0.17883277) + 0.28466892) / 12.0


def hlg_display(rgb):
    scene = tuple(hlg_scene(v) for v in rgb)
    y = mul(BT2020, scene)[1]
    return tuple(v * y ** 0.2 for v in scene)


def fixed(value):
    return struct.pack('>i', round(value * 65536))


def xyz_tag(xyz):
    return b'XYZ \0\0\0\0' + b''.join(fixed(v) for v in xyz)


def text_tag(text):
    return b'text\0\0\0\0' + text.encode('ascii') + b'\0'


def description(text):
    data = text.encode('utf-16be')
    return b'mluc\0\0\0\0' + struct.pack('>II', 1, 12) + b'enUS' + struct.pack('>II', len(data), 28) + data


def curve(function):
    samples = [round(function(n / 4096) * 65535) for n in range(4097)]
    return b'curv\0\0\0\0' + struct.pack('>I', len(samples)) + struct.pack('>' + 'H' * len(samples), *samples)


def inverse(matrix):
    a, b, c = matrix[0]
    d, e, f = matrix[1]
    g, h, i = matrix[2]
    determinant = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g)
    return tuple(tuple(v / determinant for v in row) for row in
                 ((e*i-f*h, c*h-b*i, b*f-c*e),
                  (f*g-d*i, a*i-c*g, c*d-a*f),
                  (d*h-e*g, b*g-a*h, a*e-b*d)))


def hlg_signal(display):
    y = max(mul(BT2020, display)[1], 0.0)
    scene = tuple(max(v, 0.0) / y ** (1.0 / 6.0) if y else 0.0 for v in display)
    def encode(v):
        return math.sqrt(3 * v) if v <= 1.0 / 12 else 0.17883277 * math.log(12 * v - 0.28466892) + 0.55991073
    return tuple(min(max(encode(v), 0.0), 1.0) for v in scene)


def float_matrix(matrix):
    values = tuple(v for row in matrix for v in row) + (0.0,) * 3
    return b'matf\0\0\0\0' + struct.pack('>HH12f', 3, 3, *values)


def float_sqrt_curves():
    # Two formula segments: zero for X<=0, sqrt(X) for X>0. This allocates
    # more reverse-CLUT samples near black without undefined negative powers.
    curve_data = b'curf\0\0\0\0' + struct.pack('>HHf', 2, 0, 0)
    for parameters in ((1, 0, 0, 0), (.5, 1, 0, 0)):
        curve_data += b'parf\0\0\0\0' + struct.pack('>HH4f', 0, 0, *parameters)
    return (b'cvst\0\0\0\0' + struct.pack('>HH', 3, 3)
            + struct.pack('>II', 36, len(curve_data)) * 3 + curve_data)


def float_clut(function):
    grid = 65
    result = bytearray(b'clut\0\0\0\0' + struct.pack('>HH', 3, 3)
                       + bytes((grid,) * 3 + (0,) * 13))
    for r in range(grid):
        for g in range(grid):
            for b in range(grid):
                values = function((r / (grid - 1), g / (grid - 1), b / (grid - 1)))
                if not all(math.isfinite(v) for v in values):
                    raise ValueError('Nonfinite ICC float CLUT value')
                result.extend(struct.pack('>3f', *values))
    return bytes(result)


def float_hlg(reverse=False):
    # ICC.1:2022 10.16: float PCS values are actual XYZ, no XYZ16 scale.
    # Matrix elements do not clip. CLUT inputs alone are bounded to [0,1].
    columns = [adapt(tuple(row[n] for row in BT2020)) for n in range(3)]
    matrix = tuple(tuple(columns[c][r] * 1000 / 203 for c in range(3))
                   for r in range(3))
    elements = ([float_matrix(inverse(matrix)), float_sqrt_curves(),
                 float_clut(lambda rgb: hlg_signal(tuple(v * v for v in rgb)))] if reverse
                else [float_clut(hlg_display), float_matrix(matrix)])
    result = bytearray(b'mpet\0\0\0\0' + struct.pack('>HHI', 3, 3, len(elements)))
    offset = 16 + 8 * len(elements)
    for element in elements:
        result.extend(struct.pack('>II', offset, len(element)))
        offset += len(element) + (-len(element) % 4)
    for element in elements:
        result.extend(element)
        result.extend(b'\0' * (-len(element) % 4))
    return bytes(result)


def lut_hlg(reverse=False):
    grid = 33
    identity = tuple(1.0 if r == c else 0.0 for r in range(3) for c in range(3))
    header = b'mft2\0\0\0\0' + bytes((3, 3, grid, 0)) + b''.join(fixed(v) for v in identity) + struct.pack('>HH', 256, 256)
    table = b''.join(struct.pack('>H', n * 257) for n in range(256))
    clut = bytearray()
    for r in range(grid):
        for g in range(grid):
            for b in range(grid):
                position = (r / (grid - 1), g / (grid - 1), b / (grid - 1))
                if reverse:
                    xyz = tuple(v * 65535 / 32768 * 203 / 1000 for v in position)
                    values = hlg_signal(mul(inverse(BT2020), adapt(xyz, D50, D65)))
                    scale = 65535
                else:
                    values = tuple(v * 1000 / 203 for v in adapt(mul(BT2020, hlg_display(position))))
                    scale = 32768
                for value in values:
                    # ICC lut16 XYZ encoding:32768 represents1.0, not65535.
                    if not math.isfinite(value):
                        raise ValueError('Nonfinite ICC LUT value')
                    # Explicit finite PCS gamut approximation: Bradford-adapted
                    # saturated BT.2020 can have negative XYZ components. ICC
                    # XYZ16 cannot encode those; clamp only out-of-range values.
                    encoded = min(max(value * scale, 0.0), 65535.0)
                    clut.extend(struct.pack('>H', round(encoded)))
    return header + table * 3 + clut + table * 3


def profile(target):
    names = {'pq': 'Jerboa BT.2100 PQ BT.2020 D65 W203 v2', 'hlg': 'Jerboa BT.2100 HLG BT.2020 D65 W203 v2', 'srgb': 'Jerboa sRGB D65'}
    name = names[target]
    tags = {'desc': description(name), 'cprt': text_tag('Copyright Jerboa contributors. Standard-derived profile; CC0-1.0.'),
            'wtpt': xyz_tag(D50), 'lumi': xyz_tag((0, {'pq': 203, 'hlg': 203, 'srgb': 100}[target], 0))}
    if target == 'hlg':
        tags['A2B0'] = lut_hlg()
        tags['B2A0'] = lut_hlg(reverse=True)
        # Both perceptual and media-relative intent override integer fallbacks.
        # Unsupported mpet CMMs select A2B/B2A, with diagnosed highlight loss.
        for intent in ('0', '1'):
            tags['D2B' + intent] = float_hlg()
            tags['B2D' + intent] = float_hlg(reverse=True)
    else:
        matrix = BT2020 if target == 'pq' else SRGB
        # Native built-in/owned grey controls established PCS Y=1 ->203nits.
        # TRC remains normalized to10000; scale the actual matrix, not just lumi.
        # s15Fixed16 XYZ supports extended PCS; do not clamp HDR peak to1.
        scale = 10000 / 203 if target == 'pq' else 1
        for n, channel in enumerate('rgb'):
            tags[channel + 'XYZ'] = xyz_tag(adapt(tuple(row[n] * scale for row in matrix)))
        trc = curve(pq if target == 'pq' else lambda v: v / 12.92 if v <= 0.04045 else ((v + 0.055) / 1.055) ** 2.4)
        tags.update({channel + 'TRC': trc for channel in 'rgb'})
    # Explicit D65-to-D50 adaptation, independent of any vendor ICC profile.
    columns = [adapt(tuple(1.0 if i == n else 0.0 for i in range(3))) for n in range(3)]
    tags['chad'] = b'sf32\0\0\0\0' + b''.join(fixed(columns[c][r]) for r in range(3) for c in range(3))
    header = bytearray(128)
    header[8:12] = bytes((4, 0x30, 0, 0))
    header[12:24] = b'mntrRGB XYZ '
    header[24:36] = struct.pack('>6H', 2026, 1, 1, 0, 0, 0)
    header[36:40] = b'acsp'
    header[64:68] = struct.pack('>I', 1)
    header[68:80] = b''.join(fixed(v) for v in D50)
    header[80:84] = b'JRBA'
    directory = bytearray(struct.pack('>I', len(tags)))
    payload = bytearray()
    offset = 128 + 4 + 12 * len(tags)
    for signature, data in sorted(tags.items()):
        directory.extend(signature.encode('ascii') + struct.pack('>II', offset + len(payload), len(data)))
        payload.extend(data)
        payload.extend(b'\0' * (-len(payload) % 4))
    result = header + directory + payload
    result[:4] = struct.pack('>I', len(result))
    # ICCv4 ID hashes zero flags, rendering intent and ID fields.
    identity = bytearray(result)
    identity[44:48] = b'\0' * 4
    identity[64:68] = b'\0' * 4
    identity[84:100] = b'\0' * 16
    result[84:100] = hashlib.md5(identity).digest()
    return name, bytes(result)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true', help='Verify committed profiles without writing')
    args = parser.parse_args()
    for target in ('pq', 'hlg'):
        name, data = profile(target)
        path = Path(__file__).with_name('jerboa-' + target + '.icc')
        if args.check:
            if path.read_bytes() != data:
                raise SystemExit(f'Generated profile differs: {path}')
        else:
            path.write_bytes(data)
        print(target, name, len(data), hashlib.sha256(data).hexdigest())


if __name__ == '__main__':
    main()
