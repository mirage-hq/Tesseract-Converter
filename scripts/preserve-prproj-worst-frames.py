#!/usr/bin/env python3
"""Preserve diagnostic decoded frames; never change the scoring decision."""
import json
from pathlib import Path
import subprocess
import sys


def main():
    output = Path(sys.argv[1])
    report = output / 'report.json'
    if not report.exists():
        return  # A pre-render failure has no frames to decode.
    manifest_path = Path(__file__).resolve().parents[1] / 'tests/manifest.json'
    manifest = {case['id']: case for case in json.loads(manifest_path.read_text())['cases']}
    for case in json.loads(report.read_text())['cases']:
        if case['status'] != 'failed':
            continue
        case_id = case['case_id']
        comparison_file = output / case_id / 'comparison.json'
        if not comparison_file.exists():
            continue
        comparison = json.loads(comparison_file.read_text())
        worst = min(comparison['frame_results'], key=lambda frame: frame['score']['similarity'])
        reference = manifest_path.parents[1] / manifest[case_id]['reference_video']['repo_path']
        for label, video in (
            ('adobe', reference),
            ('tesseract', output / case_id / 'tesseract.mp4'),
        ):
            if not video.is_file():
                continue
            subprocess.run([
                'ffmpeg', '-hide_banner', '-loglevel', 'error',
                '-i', str(video), '-ss', str(worst['time_secs']),
                '-vf', 'scale=1280:720:flags=lanczos',
                '-fps_mode', 'vfr', '-frames:v', '1', '-y',
                str(output / case_id / f'{label}-worst-decoded.png'),
            ], check=True)


if __name__ == '__main__':
    main()
