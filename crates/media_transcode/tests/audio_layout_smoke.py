#!/usr/bin/env python3
"""Generated unlabeled QuickTime PCM stereo regression; no customer bytes or Adobe.

QuickTime Sound Sample Description's numberOfChannels defines ordinary mono/stereo;
chan is optional. Remove only the generated chan atom (replace it with free).
Use distinct 440/997 Hz signals to detect swapping/remixing independently of probes.
"""
import argparse
import hashlib
import json
import math
from pathlib import Path
import struct
import subprocess


def run(argv):
    result = subprocess.run([str(x) for x in argv], capture_output=True, timeout=120)
    if result.returncode:
        raise RuntimeError(result.stderr.decode(errors='replace'))
    return result.stdout


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ['converter', 'ffmpeg', 'ffprobe', 'work-dir']:
        parser.add_argument('--' + name, type=Path, required=True)
    args = parser.parse_args()
    root = args.work_dir.resolve()
    root.mkdir(parents=True, exist_ok=False)
    raw = root / 'tones.s16be'
    raw.write_bytes(b''.join(struct.pack('>hh',
        round(10000 * math.sin(2 * math.pi * 440 * i / 48000)),
        round(7000 * math.sin(2 * math.pi * 997 * i / 48000))) for i in range(192192)))
    source = root / 'implicit-stereo.mp4'
    run([args.ffmpeg, '-v', 'error', '-f', 'lavfi', '-i',
         'color=size=32x32:rate=30000/1001:duration=4.004',
         '-f', 's16be', '-ar', '48000', '-ac', '2', '-i', raw,
         '-c:v', 'libx264', '-pix_fmt', 'yuv422p10le', '-threads', '1',
         '-c:a', 'pcm_s16be', '-f', 'mov', '-brand', 'XAVC', source])
    data = bytearray(source.read_bytes())
    # The generated fixture has exactly one audio sample entry and one chan atom.
    index = data.index(b'chan')
    assert data.count(b'chan') == 1
    size = struct.unpack_from('>I', data, index - 4)[0]
    assert size == 24
    data[index:index + 4] = b'free'
    source.write_bytes(data)
    original = sha(source)

    def probe(path):
        value = json.loads(run([args.ffprobe, '-v', 'error', '-show_streams', '-show_format',
                               '-of', 'json', path]))
        path.with_suffix(path.suffix + '.probe.json').write_text(json.dumps(value, indent=2))
        return next(x for x in value['streams'] if x['codec_type'] == 'audio')

    audio = probe(source)
    assert 'channel_layout' not in audio
    assert audio['codec_name'] == 'pcm_s16be'
    assert audio['duration_ts'] == 192192
    results = []
    def backend_options(backend):
        if backend == 'library':
            return ['--backend', 'library']
        return ['--backend', 'external-ffmpeg-command', '--ffmpeg-path', args.ffmpeg,
                '--ffprobe-path', args.ffprobe]

    for backend in ['library', 'external']:
        output = root / f'{backend}.mp4'
        report = json.loads(run([args.converter, 'transcode', source, '--output', output,
                                *backend_options(backend), '--json']))
        (root / f'{backend}.json').write_text(json.dumps(report, indent=2))
        actual = probe(output)
        assert actual['channel_layout'] == 'stereo'
        for field in ['sample_rate', 'channels', 'start_time', 'duration']:
            assert actual[field] == audio[field], (field, actual, audio)
        decoded = run([args.ffmpeg, '-v', 'error', '-i', output, '-map', '0:a:0',
                       '-f', 'f32le', '-c:a', 'pcm_f32le', '-'])
        samples = struct.unpack('<' + 'f' * (len(decoded) // 4), decoded)
        # AAC is lossy and may expose trailing decoder padding. Only score the
        # central second; timing is independently asserted from container facts.
        powers = []
        for channel in range(2):
            signal = samples[48000 * 2 + channel:96000 * 2:2]
            row = []
            for frequency in [440, 997]:
                real = sum(x * math.cos(2 * math.pi * frequency * i / 48000)
                           for i, x in enumerate(signal))
                imag = sum(x * math.sin(2 * math.pi * frequency * i / 48000)
                           for i, x in enumerate(signal))
                row.append(real * real + imag * imag)
            assert row[channel] > 100 * row[1 - channel], row
            powers.append(row)
        results.append({'backend': backend, 'audio': actual, 'tone_powers': powers})
    # Audio-only preparation selects the precision-preserving PCM path.
    pcm_source = root / 'audio-only.mov'
    run([args.ffmpeg, '-v', 'error', '-i', source, '-map', '0:a:0', '-c:a', 'copy', pcm_source])
    data = bytearray(pcm_source.read_bytes())
    if b'chan' in data:
        index = data.index(b'chan')
        data[index:index + 4] = b'free'
    pcm_source.write_bytes(data)
    pcm_hash = sha(pcm_source)
    assert 'channel_layout' not in probe(pcm_source)
    for backend in ['library', 'external']:
        output = root / f'{backend}.wav'
        run([args.converter, 'transcode', pcm_source, '--output', output,
             *backend_options(backend), '--json'])
        decoded = run([args.ffmpeg, '-v', 'error', '-i', output, '-f', 's16be', '-c:a', 'pcm_s16be', '-'])
        assert decoded == raw.read_bytes(), 'PCM samples/channel order changed'
    assert sha(source) == original and sha(pcm_source) == pcm_hash
    (root / 'results.json').write_text(json.dumps({'source_sha256': original,
        'source_unchanged': True, 'pcm_bitwise_equal': True, 'results': results}, indent=2))


if __name__ == '__main__':
    main()
