#!/usr/bin/env python3
"""Pin newly authored audio inputs; never render, score, upload or replace a registry.

Run only after guarded Adobe authoring/readback. Explicit FX JSONs must already
exist. Native references are deliberately created later by an opted-in E2E run.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import math
from pathlib import Path

IMPORT_ONLY = {'native-muted', 'stereo-levels', 'expression'}
EXPORT_ONLY = {'hidden-layer', 'static-zero', 'constant-zero-zero-base',
               'constant-zero-nonzero-base', 'all-zero-one-key', 'all-zero-two-keys',
               'conflicting-gain-clock', 'invalid-gain-hull', 'empty-only-group'}
MUTED = {'native-muted', 'hidden-layer', 'hidden-group', 'static-zero',
         'constant-zero-zero-base', 'constant-zero-nonzero-base',
         'all-zero-one-key', 'all-zero-two-keys'}
GROUPS = {'hidden-group', 'audio-group', 'group-affine-playback',
          'empty-nested-group', 'fractional-duration'}
HALF_DB = 20 * math.log10(0.5)


def pin(path: Path) -> dict:
    return {'path': path.name, 'sha256': hashlib.sha256(path.read_bytes()).hexdigest()}


def field(layer: str, name: str, value) -> dict:
    return {'layer': layer, 'field': name, 'value': value}


def export_controls(case: str) -> list[dict]:
    layer = 'audio-701' if case in {'conflicting-gain-clock', 'invalid-gain-hull'} else (
        'retained-sibling' if case == 'empty-only-group' else 'audio-700')
    if case == 'source-switch':
        return [field(layer, '$count', 2), field(layer, '$all.inPoint', [2, 3]),
                field(layer, '$all.outPoint', [3, 4]), field(layer, '$all.startTime', [1.5, 1.5]),
                field(layer, '$all.stretch', [100, 100]), field(layer, '$all.enabled', [False, False]),
                field(layer, '$all.audioEnabled', [True, True]),
                field(layer, '$all.audioLevels.keys.0.time', [2, 2]),
                field(layer, '$all.audioLevels.keys.1.time', [3, 3]),
                field(layer, '$all.audioLevels.keys.0.value', [[HALF_DB, HALF_DB]] * 2),
                field(layer, '$all.audioLevels.keys.1.value', [[0, 0]] * 2)]
    result = [field(layer, '$count', 1), field(layer, 'hasAudio', True),
              field(layer, 'enabled', False), field(layer, 'audioEnabled', case not in MUTED),
              field(layer, 'inPoint', 1),
              field(layer, 'outPoint', 4.5 if case == 'eof-tail' else (
                  2.001 if case == 'fractional-duration' else 2 if case == 'nonlinear-remap' else 3))]
    if case != 'nonlinear-remap':
        result += [field(layer, 'stretch', 200 if case == 'affine-playback' else 100),
                   field(layer, 'startTime', 0 if case == 'affine-playback' else 0.5)]
    key_cases = {'all-zero-one-key', 'all-zero-two-keys', 'zero-base-unmute',
                 'hold-gain', 'linear-gain', 'bezier-gain', 'affine-playback'}
    if case in key_cases:
        times = [1.5] if case == 'all-zero-one-key' else ([1, 2] if case == 'affine-playback' else [1.5, 2.5])
        values = [-192] * len(times) if case.startswith('all-zero') else (
            [-192, 0] if case == 'zero-base-unmute' else [HALF_DB, 0])
        result.append(field(layer, 'audioLevels.keys.length', len(times)))
        for index, (time, value) in enumerate(zip(times, values)):
            result += [field(layer, f'audioLevels.keys.{index}.time', time),
                       field(layer, f'audioLevels.keys.{index}.value', [value, value])]
    else:
        base_db = -192 if case in {'static-zero', 'constant-zero-zero-base', 'constant-zero-nonzero-base'} else HALF_DB
        result.append(field(layer, 'audioLevels.value', [base_db, base_db]))
    if case in GROUPS:
        parent = 'audio-group-701'
        result += [field(parent, 'sourceKind', 'composition'), field(parent, 'sourceWidth', 1),
                   field(parent, 'sourceHeight', 1),
                   field(parent, 'inPoint', 1 if case == 'group-affine-playback' else 0),
                   field(parent, 'outPoint', 5 if case == 'group-affine-playback' else (2.001 if case == 'fractional-duration' else 3)),
                   field(parent, 'stretch', 200 if case == 'group-affine-playback' else 100)]
        if case == 'fractional-duration':
            result.append(field(parent, 'sourceDuration', 49 / 24))
    if case == 'nonlinear-remap':
        result += [field(layer, 'timeRemapEnabled', True), field(layer, 'timeRemap.keys.length', 4)]
    if case in {'conflicting-gain-clock', 'invalid-gain-hull'}:
        result.append(field('audio-700', '$count', 0))
    if case == 'empty-only-group':
        result.append(field('audio-group-701', '$count', 0))
    return result


def reference_expectation(case: str) -> dict:
    audible, silent = [[1.1, 1.4]], [[0, 0.9], [5.1, 6]]
    if case in MUTED:
        audible, silent = [], [[0, 6]]
    elif case == 'zero-base-unmute':
        audible, silent = [[2.6, 2.9]], [[0, 2.4], [3.1, 6]]
    elif case == 'source-switch':
        audible, silent = [[2.1, 2.4], [3.1, 3.4]], [[0, 1.9], [4.1, 6]]
    elif case == 'group-affine-playback':
        audible, silent = [[3.1, 3.4], [4.1, 4.4]], [[0, 2.9], [5.1, 6]]
    elif case == 'eof-tail':
        audible, silent = [[1.1, 1.4], [4.1, 4.4]], [[0, 0.9], [4.6, 6]]
    return {'audible_windows': audible, 'silent_windows': silent,
            'minimum_rms': 0.005, 'maximum_silent_rms': 0.0001}


def register(root: Path) -> None:
    target = root / 'cases.json'
    provenance = root / 'provenance.json'
    if target.exists() or provenance.exists():
        raise ValueError('refusing to replace pinned source/case provenance')
    receipt = json.loads((root / 'native-readback.json').read_text())
    if receipt['status'] != 'ok' or not receipt['owned_project_closed']:
        raise ValueError('native authoring did not complete in an owned project')
    source = pin(root / 'audio_cases.aep')
    media = [pin(root / name) for name in ('sound.wav', 'other.wav', 'movie.mov')]
    cases = []
    for comp in receipt['result']['roots']:
        case = comp['name']
        directions = ['import'] if case in IMPORT_ONLY else ['export'] if case in EXPORT_ONLY else ['import', 'export']
        native = []
        for layer in comp['layers']:
            for name in ('inPoint', 'outPoint', 'startTime', 'stretch', 'audioEnabled'):
                native.append(field(layer['name'], name, layer[name]))
            if layer['audioLevels']:
                if not layer['audioLevels']['expressionEnabled'] and not layer['audioLevels']['keys']:
                    native.append(field(layer['name'], 'audioLevels.value', layer['audioLevels']['value']))
                native.append(field(layer['name'], 'audioLevels.keys.length', len(layer['audioLevels']['keys'])))
                for index, key in enumerate(layer['audioLevels']['keys']):
                    for name in ('time', 'value', 'inType', 'outType'):
                        native.append(field(layer['name'], f'audioLevels.keys.{index}.{name}', key[name]))
        diagnostics = {'import': [], 'export': []}
        if case in {'linear-gain', 'bezier-gain'}:
            diagnostics['export'] = ['continuous gain keys are approximated']
        if case == 'eof-tail':
            diagnostics['export'] = ['retains the audible prefix']
        if case == 'conflicting-gain-clock':
            diagnostics['export'] = ['Time Remap cannot drive occurrence-owned Audio Levels']
        if case == 'invalid-gain-hull':
            diagnostics['export'] = ['gain control hull is negative']
        if case == 'stereo-levels':
            diagnostics['import'] = ['quieter channel']
        if case == 'expression':
            diagnostics['import'] = ['expression requires the AE environment']
        fx = root / 'fx' / f'{case}.json'
        exported_controls = export_controls(case) if 'export' in directions else []
        if 'export' in directions and case in {'all-zero-one-key', 'all-zero-two-keys', 'zero-base-unmute', 'hold-gain', 'linear-gain', 'bezier-gain', 'affine-playback', 'source-switch'}:
            native_keys = next(layer['audioLevels']['keys'] for layer in comp['layers']
                               if layer['audioLevels'] and layer['audioLevels']['keys'])
            prefix = '$all.' if case == 'source-switch' else ''
            out_type = native_keys[0]['outType']
            exported_controls.append(field('audio-700', prefix + 'audioLevels.keys.0.outType',
                                            [out_type, out_type] if case == 'source-switch' else out_type))
            if len(native_keys) > 1:
                in_type = native_keys[1]['inType']
                exported_controls.append(field('audio-700', prefix + 'audioLevels.keys.1.inType',
                                                [in_type, in_type] if case == 'source-switch' else in_type))
        cases.append({'id': case, 'directions': directions,
                      'source': {**source, 'composition_id': comp['id'], 'composition_name': case},
                      'primary': media, 'fx_input': {'document_sha256': pin(fx)['sha256']} if 'export' in directions else None,
                      'reference': None, 'reference_expectation': reference_expectation(case),
                      'expected_diagnostics': diagnostics,
                      'native_expected': {'import': native, 'export': exported_controls}})
    if len(cases) != 28 or len({c['id'] for c in cases}) != len(cases):
        raise ValueError('expected 28 independently authored root cases')
    policy = {'sample_rate': 48000, 'channels': 2, 'duration_seconds': 6,
              'duration_tolerance_seconds': 0.01, 'relative_rms_error_max': 0.05,
              'window_rms_error_max': 0.003, 'silence_reference_rms_max': 0.0001,
              'silence_leak_rms_max': 0.0005}
    target.write_text(json.dumps({'version': 1, 'policy': policy, 'cases': cases}, indent=2) + '\n')
    provenance.write_text(json.dumps({
        'source': {**source, 'bytes': (root / 'audio_cases.aep').stat().st_size},
        'authoring': pin(root / 'author_audio_cases.jsx'),
        'native_readback': pin(root / 'native-readback.json'),
        'adobe_version': receipt['adobe_version'], 'adobe_build': receipt['adobe_build'],
        'source_fps': 24, 'source_duration_seconds': 6, 'primary_media': media,
        'supporting_targets': [{'composition_id': item['id'], 'name': item['name'],
                                'consuming_cases': [item['name'].split('__')[0]]}
                               for item in receipt['result']['nested']],
        'edited_fx_inputs': [{'case': case['id'], 'path': 'fx/' + case['id'] + '.json',
                             'sha256': case['fx_input']['document_sha256']}
                            for case in cases if 'export' in case['directions']],
        'reference_render': 'UNRUN', 'asset_publication': 'UNRUN',
        'import_execution': 'UNRUN', 'export_execution': 'UNRUN', 'audio_scores': 'unmeasured',
        'note': 'Primary media are inputs, never independent Adobe reference output.'
    }, indent=2) + '\n')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dir', type=Path, default=Path(__file__).resolve().parents[1] / 'crates/aftereffects_file/tests/fixtures/audio_e2e')
    register(parser.parse_args().dir)
