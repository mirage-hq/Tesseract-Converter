#!/usr/bin/env python3
"""Explicit Adobe audio-reference preparation with local committed media.

Only this maintainer command launches Adobe; normal tests read checked-in MP4s.
"""
from __future__ import annotations

import argparse
import json
import math
import re
from pathlib import Path
import shutil
import subprocess

from aep_audio_adobe import run_adobe
from aep_audio_test import decode, require_hash, sha256
from aep_feature_proof import atomic_write
from aep_feature_proof_publish import _validate_video, utc_now

ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / 'crates/aftereffects_file/tests/fixtures/audio_e2e'
MANIFEST = FIXTURE / 'cases.json'
TEMPLATE = 'H.264 - Match Render Settings - 40 Mbps'


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2, allow_nan=False) + '\n')


def validate_black_canvas(video):
    # These native cases contain audio only (including MOV with its eye off).
    # Decode six distributed frames; reject offline slates or leaked video.
    result = subprocess.run(['ffmpeg','-v','error','-i',str(video),'-vf','fps=1',
                             '-frames:v','6','-pix_fmt','rgb24','-f','rawvideo','-'],
                            capture_output=True,check=True,timeout=60)
    if len(result.stdout) != 6*320*180*3 or max(result.stdout)>3:
        raise RuntimeError('native audio-only canvas contains unexpected video/slate pixels')
    return {'samples_seconds':[0,1,2,3,4,5],'maximum_rgb_channel':max(result.stdout),
            'expected':'black audio-only canvas; not alpha proof'}


def validate_audio(video, case):
    rate, channels, samples = decode(video)
    if (rate, channels, len(samples)) != (48000, 2, 576000):
        raise RuntimeError('native reference must present exactly six seconds of stereo 48kHz audio')
    contract = case['reference_expectation']
    measured = []
    for kind in ('audible_windows', 'silent_windows'):
        for start, end in contract[kind]:
            values = []
            for channel in range(channels):
                window = samples[round(start * rate) * channels + channel:round(end * rate) * channels:channels]
                rms = math.sqrt(sum(x*x for x in window) / len(window))
                limit = contract['minimum_rms'] if kind == 'audible_windows' else contract['maximum_silent_rms']
                if (kind == 'audible_windows' and rms < limit) or (kind == 'silent_windows' and rms > limit):
                    raise RuntimeError(f'{case["id"]}: {kind} {start}..{end} channel {channel}: RMS={rms}')
                values.append(rms)
            measured.append({'kind': kind, 'start': start, 'end': end, 'channel_rms': values})
    return {'rate': rate, 'channels': channels, 'frames': len(samples)//channels,
            'critical_windows': measured, 'scope': 'native reference content sanity, not conversion fidelity'}


def render(case, work, owned_pid, settings_only=False):
    if case['reference'] is not None:
        raise RuntimeError('reference already pinned; never overwrite an oracle')
    require_hash(FIXTURE / case['source']['path'], case['source']['sha256'], 'native source')
    for media in case['primary']:
        require_hash(FIXTURE / media['path'], media['sha256'], 'primary source media')
    work.mkdir(parents=True, exist_ok=False)
    source = work / 'source.aep'
    shutil.copyfile(FIXTURE / case['source']['path'], source)
    output = work / 'reference.mp4'
    body = '''function runAudioCase(project) {
 var c=null;
 for(var i=1;i<=project.numItems;i++) if(project.item(i) instanceof CompItem && project.item(i).id===COMP_ID) c=project.item(i);
 if(!c || c.name!==COMP_NAME || c.width!==320 || c.height!==180 || c.frameRate!==24 || Math.abs(c.duration-6)>0.000001) throw new Error('pinned composition identity/settings drift');
 for(var j=1;j<=project.numItems;j++) { var item=project.item(j); if(item instanceof FootageItem && item.footageMissing) throw new Error('offline primary media: '+item.name); }
 if(project.renderQueue.numItems!==0) throw new Error('unexpected preexisting render queue');
 var q=project.renderQueue.items.add(c);
 q.timeSpanStart=0; q.timeSpanDuration=6;
 q.setSettings({'Use this frame rate':'30','Quality':'Best','Resolution':'Full'});
 var om=q.outputModule(1); om.applyTemplate(OUTPUT_TEMPLATE);
 var f=new File(OUTPUT_FILE); if(f.exists) throw new Error('existing reference output');
 om.file=f;
 var settings=om.getSettings(GetSettingsFormat.STRING_SETTABLE);
 AUDIO_OVERRIDE
 var observed=om.getSettings(GetSettingsFormat.STRING);
 RENDER_OPERATION
 return {compositionId:c.id,compositionName:c.name,sourceFps:c.frameRate,duration:c.duration,
         renderSettings:q.getSettings(GetSettingsFormat.STRING),outputSettings:observed,
         settableSettings:settings,rendered:RENDERED,outputExists:f.exists};
}'''
    body = body.replace('COMP_ID', str(case['source']['composition_id']))
    body = body.replace('COMP_NAME', json.dumps(case['source']['composition_name']))
    body = body.replace('OUTPUT_TEMPLATE', json.dumps(TEMPLATE)).replace('OUTPUT_FILE', json.dumps(str(output)))
    body = body.replace('AUDIO_OVERRIDE', '' if settings_only else "om.setSettings({'Output Audio':'On','Include Project Link':'false'});")
    body = body.replace('RENDER_OPERATION', '' if settings_only else "project.renderQueue.render(); if(q.status!==RQItemStatus.DONE) throw new Error('native render did not complete');")
    body = body.replace('RENDERED', 'false' if settings_only else 'true')
    receipt = run_adobe(body, work / 'native', project=source, timeout=300,
                        owned_pid=owned_pid, quit_after=False)
    if settings_only:
        print(json.dumps(receipt['result']['settableSettings'], indent=2))
        return
    native = _validate_video(output, {'duration_numerator':6,'duration_denominator':1,
                                     'width':320,'height':180}, shutil.which('ffprobe'), shutil.which('ffmpeg'))
    audio = validate_audio(output, case)
    frames = validate_black_canvas(output)
    write_json(work / 'validated.json', {'case_id':case['id'], 'source':case['source'],
        'decoded_frame_inspection':frames,
        'reference':native, 'audio_inspection':audio, 'native_receipt':receipt,
        'source_copy_sha256':sha256(source), 'validated_at':utc_now()})
    print(case['id'], 'native reference validated:', native['bytes'], native['sha256'])


def publish(case, work):
    evidence = json.loads((work / 'validated.json').read_text())
    if evidence['source'] != case['source'] or evidence['case_id'] != case['id']:
        raise RuntimeError('reference evidence belongs to another source/target')
    video = work / 'reference.mp4'
    reference = evidence['reference']
    require_hash(video, reference['sha256'], 'validated native output')
    if video.stat().st_size != reference['bytes']:
        raise RuntimeError('native output size drift')
    require_hash(FIXTURE / case['source']['path'], case['source']['sha256'], 'native source')
    if not re.fullmatch(r'[a-z0-9]+(?:-[a-z0-9]+)*', case['id']):
        raise RuntimeError('unsafe audio case ID')
    current_bytes = MANIFEST.read_bytes()
    current = json.loads(current_bytes)
    targets = [item for item in current['cases'] if item['id'] == case['id']]
    if len(targets) != 1 or targets[0]['source'] != case['source'] or targets[0]['reference'] is not None:
        raise RuntimeError('manifest target changed or already has a committed reference')
    destination = ROOT / 'tests/references/aep/audio' / f"{case['id']}.mp4"
    if (destination.exists() or destination.is_symlink()
            or not destination.parent.resolve().is_relative_to(ROOT.resolve())
            or any(parent.is_symlink() for parent in
                   (destination.parent, *destination.parent.parents)
                   if parent != ROOT and parent.is_relative_to(ROOT))):
        raise RuntimeError(f'refusing unsafe or existing committed reference: {destination}')
    targets[0]['reference'] = {'path': destination.relative_to(ROOT).as_posix()}
    try:
        with video.open('rb') as source, destination.open('xb') as output:
            shutil.copyfileobj(source, output)
        if MANIFEST.read_bytes() != current_bytes:
            raise RuntimeError('manifest changed during local reference copy')
        atomic_write(MANIFEST, json.dumps(current, indent=2, allow_nan=False) + '\n')
    except BaseException:
        destination.unlink(missing_ok=True)
        raise
    print(case['id'], 'local reference recorded:', destination.relative_to(ROOT))


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('command',choices=('settings','render','publish'))
    p.add_argument('--case',required=True)
    p.add_argument('--work',type=Path,required=True)
    p.add_argument('--owned-pid',type=int)
    p.add_argument('--confirm',required=True,help='Repeat the exact case ID')
    args=p.parse_args()
    if args.confirm!=args.case: p.error('explicit case confirmation required')
    case=next(c for c in json.loads(MANIFEST.read_text())['cases'] if c['id']==args.case)
    if args.command=='publish': publish(case,args.work.resolve())
    else:
        if not args.owned_pid: p.error('reviewed owned Adobe PID is required')
        render(case,args.work.resolve(),args.owned_pid,args.command=='settings')


if __name__=='__main__':
    main()
