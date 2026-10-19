"""Offline profile regeneration, encoding and standards-math controls."""
import hashlib
import importlib.util
import math
import struct
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1] / "profiles"
SPEC = importlib.util.spec_from_file_location("owned_icc", ROOT / "generate_owned_icc.py")
icc = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(icc)


@pytest.mark.parametrize("target,expected", [
    ("pq", "0d76b1ceab273f8293a092bb8defd97a1bc4a2386a5e8fcc6b377b4ae5fd0aad"),
    ("hlg", "c76f97e1e765af063387ef397feae9f41938841b3c5e334efe780e3ffaa85ed7"),
])
def test_owned_profile_regenerates_exact_committed_bytes(target, expected):
    _, data = icc.profile(target)
    assert data == (ROOT / f"jerboa-{target}.icc").read_bytes()
    assert hashlib.sha256(data).hexdigest() == expected
    assert struct.unpack_from(">I", data)[0] == len(data)
    assert data[36:40] == b"acsp"
    identity = bytearray(data)
    identity[44:48] = bytes(4)
    identity[64:68] = bytes(4)
    identity[84:100] = bytes(16)
    assert hashlib.md5(identity).digest() == data[84:100]
    count = struct.unpack_from(">I", data, 128)[0]
    spans = []
    for index in range(count):
        signature, offset, size = struct.unpack_from(">4sII", data, 132 + index * 12)
        assert offset % 4 == 0
        assert offset >= 132 + count * 12
        assert offset + size <= len(data)
        assert all(offset >= end or offset + size <= start for start, end in spans)
        spans.append((offset, offset + size))
        assert signature.decode("ascii")


def test_pq_native_calibrated_203nit_relative_pcs():
    # Independent native controls: builtin PQ W203 is normative; the v1 owned
    # profile instead yielded relative PCS *203. Test the serialized transform,
    # not just a lumi label: changing lumi alone cannot repair native output.
    _, data = icc.profile('pq')
    tags = {}
    for index in range(struct.unpack_from('>I', data, 128)[0]):
        signature, offset, size = struct.unpack_from('>4sII', data, 132 + index * 12)
        tags[signature] = data[offset:offset + size]
    assert struct.unpack_from('>i', tags[b'lumi'], 12)[0] / 65536 == 203
    columns = [tuple(value / 65536 for value in struct.unpack_from('>3i', tags[channel + b'XYZ'], 8))
               for channel in (b'r', b'g', b'b')]
    trc = tags[b'rTRC']
    count = struct.unpack_from('>I', trc, 8)[0]
    samples = struct.unpack_from('>' + 'H' * count, trc, 12)
    for code in (.4, .5, .75, 1):
        position = code * (count - 1)
        lower = int(position)
        fraction = position - lower
        sampled = ((1 - fraction) * samples[lower]
                   + fraction * samples[min(lower + 1, count - 1)]) / 65535
        pcs_y = sum(column[1] for column in columns) * sampled
        # Exact rational ST2084, independently of the generator's pq helper.
        power = code ** (32 / 2523)
        nits = 10000 * (max(power - 3424 / 4096, 0)
                        / (2413 / 128 - 2392 / 128 * power)) ** (16384 / 2610)
        assert pcs_y * 203 == pytest.approx(nits, abs=0.16)
    # Full-scale HDR PCS is intentionally above 1; do not silently clamp it.
    assert sum(column[1] for column in columns) == pytest.approx(10000 / 203, abs=0.00002)


def test_pq_st2084_known_values():
    # Independent published signal values pin the transfer-function checks.
    assert icc.pq(0) == 0
    assert icc.pq(1) == pytest.approx(1)
    assert 10000 * icc.pq(0.5080784) == pytest.approx(100, abs=0.0001)
    assert 10000 * icc.pq(0.7518271) == pytest.approx(1000, abs=0.001)


def test_hlg_luminance_coupled_ootf_not_channel_power():
    assert icc.hlg_display((0, 0, 0)) == (0, 0, 0)
    assert icc.hlg_display((1, 1, 1)) == pytest.approx((1, 1, 1), abs=1e-7)
    grey = (1 / 12) ** 1.2
    assert icc.hlg_display((0.5, 0.5, 0.5)) == pytest.approx((grey,) * 3)
    red = icc.hlg_display((1, 0, 0))[0]
    assert red == pytest.approx(icc.BT2020[1][0] ** 0.2, abs=1e-7)
    assert red < 0.8  # independent per-channel power would yield 1.0
    for rgb in [(0.1, 0.5, 0.9), (0.5, 0.5, 0.5), (1, 0, 0)]:
        assert icc.hlg_signal(icc.hlg_display(rgb)) == pytest.approx(rgb, abs=1e-7)


def test_hlg_only_out_of_pcs_nodes_are_clamped():
    data = icc.lut_hlg()
    # mft2 header plus three 256-entry input tables; RGB grid, last axis fastest.
    start = 52 + 3 * 256 * 2
    clipped = 0
    index = 0
    for r in range(33):
        for g in range(33):
            for b in range(33):
                values = tuple(v * 1000 / 203 for v in icc.adapt(
                    icc.mul(icc.BT2020, icc.hlg_display((r/32, g/32, b/32)))))
                encoded = struct.unpack_from(">3H", data, start + index * 6)
                assert all(math.isfinite(v) for v in values)
                expected = tuple(round(min(max(v * 32768, 0), 65535)) for v in values)
                assert encoded == expected
                clipped += any(v < 0 or v > 65535/32768 for v in values)
                index += 1
    assert clipped == 8191
    assert index == 35937


def profile_tags(data):
    return {signature: data[offset:offset + size]
            for signature, offset, size in (
                struct.unpack_from('>4sII', data, 132 + index * 12)
                for index in range(struct.unpack_from('>I', data, 128)[0]))}


def evaluate_mpet(data, values):
    # Independent binary reader/evaluator: tests actual serialized float PCS,
    # channel chaining, offsets and unclipped matrices, not generator helpers.
    assert data[:8] == b'mpet\0\0\0\0'
    inputs, outputs, count = struct.unpack_from('>HHI', data, 8)
    assert inputs == outputs == 3
    for index in range(count):
        offset, size = struct.unpack_from('>II', data, 16 + index * 8)
        assert offset % 4 == 0 and offset >= 16 + count * 8
        element = data[offset:offset + size]
        assert len(element) == size
        assert element[4:8] == bytes(4)
        assert struct.unpack_from('>HH', element, 8) == (3, 3)
        if element[:4] == b'matf':
            assert size == 60
            matrix = struct.unpack_from('>12f', element, 12)
            values = tuple(sum(matrix[r * 3 + c] * values[c] for c in range(3))
                           + matrix[9 + r] for r in range(3))
        elif element[:4] == b'cvst':
            transformed = []
            for channel, value in enumerate(values):
                start, length = struct.unpack_from('>II', element, 12 + channel * 8)
                assert start % 4 == 0
                curve = element[start:start + length]
                assert curve[:8] == b'curf\0\0\0\0'
                assert struct.unpack_from('>HHf', curve, 8) == (2, 0, 0)
                segment = 16 if value <= 0 else 44
                assert curve[segment:segment + 8] == b'parf\0\0\0\0'
                assert struct.unpack_from('>HH', curve, segment + 8) == (0, 0)
                gamma, a, b, c = struct.unpack_from('>4f', curve, segment + 12)
                transformed.append((a * value + b) ** gamma + c)
            values = tuple(transformed)
        else:
            assert element[:4] == b'clut'
            grid = element[12]
            assert element[12:15] == bytes((grid,) * 3)
            assert element[15:28] == bytes(13)
            assert size == 28 + grid ** 3 * 12
            positions = [min(max(v, 0), 1) * (grid - 1) for v in values]
            lower = [min(int(v), grid - 2) for v in positions]
            fractions = [v - n for v, n in zip(positions, lower)]
            result = [0.0] * 3
            for r in range(2):
                for g in range(2):
                    for b in range(2):
                        weight = math.prod(f if n else 1 - f
                                           for n, f in zip((r, g, b), fractions))
                        node = ((lower[0] + r) * grid + lower[1] + g) * grid + lower[2] + b
                        sample = struct.unpack_from('>3f', element, 28 + node * 12)
                        for channel in range(3):
                            result[channel] += weight * sample[channel]
            values = tuple(result)
    assert all(math.isfinite(v) for v in values)
    return values


def test_hlg_float_pcs_native_luminance_and_unclipped_peak():
    _, data = icc.profile('hlg')
    tags = profile_tags(data)
    assert tags[b'D2B0'] == tags[b'D2B1']
    assert tags[b'B2D0'] == tags[b'B2D1']
    assert tags[b'A2B0'][:4] == tags[b'B2A0'][:4] == b'mft2'
    assert struct.unpack_from('>i', tags[b'lumi'], 12)[0] / 65536 == 203
    for code in (.4, .5, .75, 1):
        scene = code ** 2 / 3 if code <= .5 else (
            math.exp((code - .55991073) / .17883277) + .28466892) / 12
        pcs = evaluate_mpet(tags[b'D2B0'], (code,) * 3)
        assert pcs[1] * 203 == pytest.approx(1000 * scene ** 1.2, abs=.06)
        # Reverse is a separate interpolated coupled transform, not channel gamma.
        assert evaluate_mpet(tags[b'B2D0'], pcs) == pytest.approx((code,) * 3, abs=.002)
    assert pcs[1] > 4.92  # integer XYZ16 cannot represent this 1000nit peak.
    red = evaluate_mpet(tags[b'D2B0'], (1, 0, 0))
    assert red[2] < 0  # float PCS preserves the negative adapted saturated-red Z.


def test_hlg_nonfinite_lut_rejects(monkeypatch):
    monkeypatch.setattr(icc, "hlg_display", lambda rgb: (math.nan, 0, 0))
    with pytest.raises(ValueError, match="Nonfinite"):
        icc.lut_hlg()
