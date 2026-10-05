#!/usr/bin/env python3
"""One-second generated quarter-turn operation smoke; no Adobe/customer media."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import struct
import subprocess


def command(args):
    result = subprocess.run([str(x) for x in args], capture_output=True)
    if result.returncode:
        raise RuntimeError(f'{args}: {result.stderr.decode(errors="replace")}')
    return result.stdout


def set_matrix(path, matrix):
    data = bytearray(path.read_bytes())
    count = 0

    def atoms(start, end):
        nonlocal count
        while start < end:
            size, kind = struct.unpack_from('>I4s', data, start)
            assert size >= 8 and start + size <= end
            if kind in (b'moov', b'trak'):
                atoms(start + 8, start + size)
            elif kind == b'tkhd':
                version = data[start + 8]
                assert version in (0, 1)
                offset = start + (48 if version == 0 else 60)
                struct.pack_into('>9i', data, offset, *matrix)
                count += 1
            start += size

    atoms(0, len(data))
    assert count == 1
    path.write_bytes(data)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--converter', type=Path, required=True)
    parser.add_argument('--ffmpeg', type=Path, required=True)
    parser.add_argument('--ffprobe', type=Path, required=True)
    parser.add_argument('--work-dir', type=Path, required=True)
    args = parser.parse_args()
    root = args.work_dir.resolve()
    root.mkdir(parents=True, exist_ok=False)
    converter = root / 'tsrct-conv'
    shutil.copy2(args.converter, converter)
    matrix = [0, 65536, 0, -65536, 0, 0, 96 * 65536, 0, 1073741824]
    # Coded corners TL red, TR green, BL blue, BR yellow.
    image = root / 'corners.ppm'
    colors = [(255, 0, 0), (0, 255, 0), (0, 0, 255), (255, 255, 0)]
    image.write_bytes(b'P6\n160 96\n255\n' + bytes(
        channel for y in range(96) for x in range(160)
        for channel in colors[(y >= 48) * 2 + (x >= 80)]))
    sources = []
    for codec, suffix in [('libx264', 'mp4'), ('qtrle', 'mov')]:
        path = root / f'source-{codec}.{suffix}'
        command([args.ffmpeg, '-v', 'error', '-loop', '1', '-i', image, '-t', '1',
                 '-r', '30', '-c:v', codec, '-pix_fmt', 'yuv420p' if codec == 'libx264' else 'rgb24',
                 '-bf', '0', '-an', path])
        set_matrix(path, matrix)
        sources.append(path)

    def probe(path):
        raw = command([args.ffprobe, '-v', 'error', '-show_streams', '-of', 'json', path])
        path.with_suffix(path.suffix + '.probe.json').write_bytes(raw)
        stream, = json.loads(raw)['streams']
        assert (stream['width'], stream['height']) == (160, 96)
        side, = [x for x in stream['side_data_list'] if x['side_data_type'] == 'Display Matrix']
        actual = [int(x) for line in side['displaymatrix'].splitlines() if ':' in line
                  for x in line.split(':')[1].split()]
        assert actual == matrix, actual
        assert side['rotation'] == -90

    def frame(path, auto):
        options = [] if auto else ['-noautorotate']
        data = command([args.ffmpeg, '-v', 'error', *options, '-i', path,
                        '-frames:v', '1', '-pix_fmt', 'rgb24', '-f', 'rawvideo', '-'])
        width, height = (96, 160) if auto else (160, 96)
        assert len(data) == width * height * 3
        labels = []
        samples = []
        for x, y in [(width // 4, height // 4), (3 * width // 4, height // 4),
                     (width // 4, 3 * height // 4), (3 * width // 4, 3 * height // 4)]:
            offset = 3 * (y * width + x)
            pixel = list(data[offset:offset + 3])
            samples.append(pixel)
            labels.append(min(range(4), key=lambda c: sum(abs(a-b) for a, b in zip(pixel, colors[c]))))
        assert labels == ([2, 0, 3, 1] if auto else [0, 1, 2, 3]), (auto, labels, samples)
        path.with_suffix(path.suffix + ('.display.ppm' if auto else '.coded.ppm')).write_bytes(
            f'P6\n{width} {height}\n255\n'.encode() + data)
        return data, samples

    source_hashes = {str(path): hashlib.sha256(path.read_bytes()).hexdigest() for path in sources}
    for source in sources:
        probe(source)
        frame(source, False)
        frame(source, True)
    reports = []
    for backend in ['external-ffmpeg-command', 'library']:
        for operation, source, suffix in [('copy', sources[0], 'mp4'),
                                           ('remux', sources[0], 'mov'),
                                           ('transcode', sources[1], 'mp4')]:
            output = root / f'{backend}-{operation}.{suffix}'
            argv = [converter, 'transcode', source, '--output', output, '--backend', backend, '--json']
            if backend != 'library':
                argv += ['--ffmpeg-path', args.ffmpeg, '--ffprobe-path', args.ffprobe]
            raw = command(argv)
            (root / f'{backend}-{operation}.json').write_bytes(raw)
            result = json.loads(raw)
            assert result['operation'] == operation, result
            assert result['source']['video']['display_matrix'] == matrix
            assert result['media']['video']['display_matrix'] == matrix
            assert result['source']['video']['rotation_degrees'] == -90
            assert result['media']['video']['rotation_degrees'] == -90
            probe(output)
            measured = {}
            for auto in [False, True]:
                reference, _ = frame(source, auto)
                actual, samples = frame(output, auto)
                mae = sum(abs(a-b) for a, b in zip(reference, actual)) / len(reference)
                assert mae <= 6, mae
                if operation in ('copy', 'remux'):
                    assert actual == reference
                measured['display' if auto else 'coded'] = {'mae': mae, 'corners': samples}
            reports.append({'backend': backend, 'operation': operation, 'matrix': matrix,
                            'rotation': -90, 'coded_dimensions': [160, 96],
                            'display_dimensions': [96, 160], 'readback': measured})
    for source in sources:
        assert hashlib.sha256(source.read_bytes()).hexdigest() == source_hashes[str(source)]
    summary = {'converter_sha256': hashlib.sha256(converter.read_bytes()).hexdigest(),
               'source_sha256': source_hashes, 'results': reports}
    (root / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n')
    print(json.dumps(summary, indent=2))


if __name__ == '__main__':
    main()
