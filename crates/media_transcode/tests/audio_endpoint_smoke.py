#!/usr/bin/env python3
"""Sample-clock MOV edits through both public backends; generated media stays outside Git."""
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


def clocks(path):
    """Read raw movie/media clocks and edits independently of libavformat."""
    data = path.read_bytes()
    result = []

    def walk(start, end):
        while start < end:
            size, tag = struct.unpack_from('>I4s', data, start)
            header = 8
            if size == 1:
                size = struct.unpack_from('>Q', data, start + 8)[0]
                header = 16
            if size == 0:
                size = end - start
            assert size >= header and start + size <= end
            p = start + header
            if tag in [b'mvhd', b'mdhd']:
                version = data[p]
                offset = p + (20 if version == 1 else 12)
                scale = struct.unpack_from('>I', data, offset)[0]
                duration = struct.unpack_from('>Q' if version == 1 else '>I', data, offset + 4)[0]
                result.append({'atom': tag.decode(), 'scale': scale, 'duration': duration})
            if tag == b'elst':
                version = data[p]
                count = struct.unpack_from('>I', data, p + 4)[0]
                fmt = '>Qqhh' if version == 1 else '>Iihh'
                result.append({'atom': 'elst', 'entries': [struct.unpack_from(
                    fmt, data, p + 8 + i * struct.calcsize(fmt)) for i in range(count)]})
            if tag == b'stts':
                count = struct.unpack_from('>I', data, p + 4)[0]
                result.append({'atom': 'stts', 'entries': [struct.unpack_from(
                    '>II', data, p + 8 + i * 8) for i in range(count)]})
            if tag in [b'moov', b'trak', b'mdia', b'minf', b'stbl', b'edts']:
                walk(p, start + size)
            start += size
    walk(0, len(data))
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ['converter', 'ffmpeg', 'ffprobe', 'work-dir']:
        parser.add_argument('--' + name, type=Path, required=True)
    args = parser.parse_args()
    root = args.work_dir.resolve()
    root.mkdir(parents=True, exist_ok=False)
    fixture = json.loads((Path(__file__).parent / 'fixtures/sony_audio_endpoint.json').read_text())
    count = fixture['source_audio_samples']
    rate = fixture['audio']['sample_rate']
    raw = root / 'tones.s16le'
    raw.write_bytes(b''.join(struct.pack('<hh',
        round(10000 * math.sin(2 * math.pi * 440 * i / rate)),
        round(7000 * math.sin(2 * math.pi * 997 * i / rate))) for i in range(count)))
    source = root / 'source.mov'
    run([args.ffmpeg, '-v', 'error', '-f', 'lavfi', '-i',
         'color=size=32x32:rate=30000/1001:duration=5.5055', '-f', 's16le',
         '-ar', rate, '-ac', '2', '-i', raw, '-c:v', 'libx264', '-pix_fmt',
         'yuv422p10le', '-threads', '1', '-c:a', 'pcm_s16be',
         '-movie_timescale', '240000', source])
    original = hashlib.sha256(source.read_bytes()).hexdigest()
    results = []

    def inspect(path, aac):
        probe = json.loads(run([args.ffprobe, '-v', 'error', '-show_streams', '-of', 'json', path]))
        audio = next(x for x in probe['streams'] if x['codec_type'] == 'audio')
        video = next(x for x in probe['streams'] if x['codec_type'] == 'video')
        assert audio['duration_ts'] == count and audio['time_base'] == '1/48000', audio
        assert audio['start_pts'] == 0 and audio['channels'] == 2
        assert int(video['nb_frames']) == fixture['frames']
        assert video['duration_ts'] == 165165 and video['time_base'] == '1/30000'
        atoms = clocks(path)
        movie = next(x for x in atoms if x['atom'] == 'mvhd')
        edits = [x for x in atoms if x['atom'] == 'elst']
        assert all(len(x['entries']) == 1 for x in edits)
        for edit in edits:
            assert edit['entries'][0][0] * rate == count * movie['scale'], edit
        assert edits[1]['entries'][0][1] == (1024 if aac else 0)
        audio_header = [x for x in atoms if x['atom'] == 'mdhd'][1]
        stts = [x for x in atoms if x['atom'] == 'stts'][1]['entries']
        assert audio_header['duration'] == count + (1024 if aac else 0)
        assert sum(n * delta for n, delta in stts) == audio_header['duration']
        packets = json.loads(run([args.ffprobe, '-v', 'error', '-select_streams', 'a:0',
                                 '-show_packets', '-show_data_hash', 'sha256', '-of', 'json', path]))['packets']
        assert packets[-1]['pts'] + packets[-1]['duration'] == count
        if aac:
            assert packets[0]['pts'] == -1024
            assert packets[0]['side_data_list'][0]['skip_samples'] == 1024
        decoded = run([args.ffmpeg, '-v', 'error', '-i', path, '-map', '0:a:0', '-f', 'f32le', '-'])
        samples = len(decoded) // 8
        # The decoder exposes AAC tail padding, not a shortened signal. The edit
        # and final packet bound presentation; do not claim lossless PCM equality.
        assert samples == (math.ceil(count / 1024) * 1024 if aac else count)
        if aac:
            values = struct.unpack('<' + 'f' * (len(decoded) // 4), decoded)
            for channel, frequency in enumerate([440, 997]):
                signal = values[48000 * 2 + channel:96000 * 2:2]
                def power(hz):
                    return abs(sum(x * complex(math.cos(2 * math.pi * hz * i / rate),
                                               math.sin(2 * math.pi * hz * i / rate))
                                   for i, x in enumerate(signal))) ** 2
                assert power(frequency) > 100 * power(997 if channel == 0 else 440)
        report = {'file': path.name, 'probe': probe, 'atoms': atoms, 'decoded_samples': samples,
                  'packets': packets}
        (root / (path.name + '.facts.json')).write_text(json.dumps(report, indent=2))
        results.append({'file': path.name, 'presented_samples': count, 'decoded_samples': samples})
        return decoded, [p['data_hash'] for p in packets]

    inspect(source, False)
    for backend in ['library', 'external']:
        options = ['--backend', 'library'] if backend == 'library' else [
            '--backend', 'external-ffmpeg-command', '--ffmpeg-path', args.ffmpeg,
            '--ffprobe-path', args.ffprobe]
        encoded = root / (backend + '.mp4')
        remuxed = root / (backend + '-remux.mov')
        for input_path, output in [(source, encoded), (encoded, remuxed)]:
            report = run([args.converter, 'transcode', input_path, '--output', output, *options, '--json'])
            (root / (output.name + '.report.json')).write_bytes(report)
        decoded, hashes = inspect(encoded, True)
        copied, copied_hashes = inspect(remuxed, True)
        assert hashes == copied_hashes and decoded == copied, 'remux changed AAC packets or samples'
    assert hashlib.sha256(source.read_bytes()).hexdigest() == original
    (root / 'results.json').write_text(json.dumps(results, indent=2))


if __name__ == '__main__':
    main()
