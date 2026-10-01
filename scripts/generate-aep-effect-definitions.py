#!/usr/bin/env python3
"""Extract AE 26 parameter ABI declarations from the pinned Adobe-authored catalog.

Only typed pard fields and independent ExtendScript receipt values are emitted;
no layer values, keyframes, sdat, or source RIFX chunks are retained.
"""
import argparse
import hashlib
import json
import math
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SOURCE = ROOT / 'crates/aftereffects_file/tests/fixtures/effects/catalog.aep'
RECEIPT = ROOT / 'crates/aftereffects_file/tests/fixtures/effects/catalog-receipt.json'
DEST = ROOT / 'crates/aftereffects_file/src/effects/definitions.json'
SOURCE_SHA256 = '7519496eb44f5ebc0eff5c2ad78476afdd738303cfb18449ac4f3596e82070c2'
RECEIPT_SHA256 = '3f5aeab2710950e954a9e749b62b8747a1defc2092e6f46c2cee8a3030e4a780'
RADIAL_RECEIPT_SHA256 = 'fcf5eb5aa145cd9e39789fd1293b37dff2c703e4e8d3e3ab561f161a3f4f543f'


def numeric_defaults(kind, raw_value):
    arity = {1: 1, 2: 1, 3: 1, 4: 1, 5: 4, 6: 2, 7: 1, 10: 1}.get(kind)
    if arity is None:
        return []
    values = raw_value if isinstance(raw_value, list) else [raw_value]
    if len(values) != arity or any(
        not isinstance(value, (float, int)) or isinstance(value, bool)
        or not math.isfinite(value) for value in values
    ):
        raise ValueError(f'invalid independent numeric receipt for parameter kind {kind}')
    return values


def chunks(data, start, end):
    nodes = []
    pos = start
    while pos + 8 <= end:
        tag = data[pos:pos + 4]
        length = int.from_bytes(data[pos + 4:pos + 8], 'big')
        body, last = pos + 8, pos + 8 + length
        if last > end:
            raise ValueError('truncated RIFX chunk')
        if tag == b'LIST':
            kind = data[body:body + 4].decode('ascii')
            children = chunks(data, body + 4, last) if kind != 'btdk' else []
            nodes.append((kind, None, children))
        else:
            nodes.append((tag.decode('ascii'), data[body:last], []))
        pos = last + (length & 1)
    if pos != end:
        raise ValueError('invalid RIFX alignment')
    return nodes


def visit(nodes):
    for node in nodes:
        yield node
        yield from visit(node[2])


def utf8(blob):
    length = int.from_bytes(blob[4:8], 'big')
    if blob[:4] != b'Utf8' or len(blob) not in (8 + length, 9 + length) or any(blob[8 + length:]):
        raise ValueError('malformed Utf8 string')
    return blob[8:8 + length].decode('utf-8')


def extract_definitions(data, receipt):
    if data[:4] != b'RIFX':
        raise ValueError('not a RIFX file')
    tree = chunks(data, 12, 8 + int.from_bytes(data[4:8], 'big'))
    group, = (node for node in visit(tree) if node[0] == 'EfdG')
    by_name = {effect['matchName']: effect for effect in receipt['effects']}
    if len(by_name) != len(receipt['effects']):
        raise ValueError('duplicate receipt effect')
    definitions = []
    for effect in group[2]:
        if effect[0] != 'EfDf':
            continue
        match_name = next(body.split(b'\0')[0].decode('utf-8') for tag, body, _ in effect[2] if tag == 'tdmn')
        native, = (node for node in effect[2] if node[0] == 'sspc')
        name = utf8(next(body for tag, body, _ in native[2] if tag == 'fnam'))
        table, = (node for node in native[2] if node[0] == 'parT')
        evidence = by_name[match_name]
        if evidence['name'] != name:
            raise ValueError(f'Adobe name mismatch for {match_name}')
        receipt_params = {p['matchName']: p for p in evidence['properties']}
        if len(receipt_params) != len(evidence['properties']):
            raise ValueError(f'duplicate receipt parameter for {match_name}')
        params = []
        current = None
        for tag, body, _ in table[2]:
            if tag == 'tdmn':
                current = body.split(b'\0')[0].decode('utf-8')
            elif tag == 'pard':
                if current is None or len(body) != 148:
                    raise ValueError(f'invalid pard for {match_name}')
                words = [int.from_bytes(body[i:i + 4], 'big') for i in range(0, len(body), 4)]
                kind = words[3]
                raw_value = receipt_params.get(current, {}).get('value')
                # Receipt is an independent Adobe UI readout. Point receipt
                # values are evidence, while the ABI encodes relative defaults.
                defaults = numeric_defaults(kind, raw_value)
                params.append({
                    'match_name': current,
                    'label': body[16:48].split(b'\0')[0].decode('utf-8'),
                    'header_flags': words[:3],
                    'kind': kind,
                    'reserved': words[12:14],
                    'payload_words': words[14:],
                    'popup': None,
                    'defaults': defaults,
                    'point_relative': kind == 6,
                })
                current = None
            elif tag == 'pdnm':
                if not params or current is not None:
                    raise ValueError(f'orphan popup for {match_name}')
                params[-1]['popup'] = utf8(body)
        names = {parameter['match_name'] for parameter in params}
        if len(names) != len(params) or names - {match_name + '-0000'} != set(receipt_params):
            raise ValueError(f'parameter definition / receipt identity mismatch for {match_name}')
        definitions.append({'match_name': match_name, 'name': name, 'parameters': params})
    if len(definitions) != len(by_name) or {d['match_name'] for d in definitions} != set(by_name):
        raise ValueError('effect definition / Adobe receipt count mismatch')
    return definitions


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true', help='verify without rewriting the registry')
    args = parser.parse_args()
    data = SOURCE.read_bytes()
    if hashlib.sha256(data).hexdigest() != SOURCE_SHA256:
        raise ValueError('Adobe-authored catalog differs from pinned native source')
    receipt_bytes = RECEIPT.read_bytes()
    if hashlib.sha256(receipt_bytes).hexdigest() != RECEIPT_SHA256:
        raise ValueError('independent Adobe receipt differs from pinned evidence')
    definitions = extract_definitions(data, json.loads(receipt_bytes))
    radial_source = SOURCE.with_name('radial_wipe_half_plane.aep').read_bytes()
    radial_receipt = SOURCE.with_name('radial_wipe_definition-receipt.json').read_bytes()
    if hashlib.sha256(radial_source).hexdigest() != '672afb582fa532bff7c4a3c610891da762d962bd66bd22845f147b45cfeb9f6a':
        raise ValueError('Radial Wipe differs from pinned independent native source')
    if hashlib.sha256(radial_receipt).hexdigest() != RADIAL_RECEIPT_SHA256:
        raise ValueError('Radial Wipe independent default readback differs')
    radial = extract_definitions(radial_source, json.loads(radial_receipt))
    if len(radial) != 1 or radial[0]['match_name'] != 'ADBE Radial Wipe':
        raise ValueError('unexpected independent Radial Wipe definitions')
    center, = (p for p in radial[0]['parameters'] if p['match_name'] == 'ADBE Radial Wipe-0003')
    # The global EfDf uses fractional defaults, unlike the sparse layer-side
    # parT percentage defaults handled by the importer. Never copy that instance.
    if center['payload_words'][:2] != [32768, 32768] or center['defaults'] != [48, 32]:
        raise ValueError('unexpected canonical Radial Wipe point default units')
    definitions.extend(radial)
    result = {
        'provenance': {
            'source': 'Adobe After Effects 26.5x89 independently authored catalog.aep',
            'source_sha256': hashlib.sha256(data).hexdigest(),
            'receipt': 'catalog-receipt.json: independent Adobe ExtendScript parameter readout',
            'receipt_sha256': RECEIPT_SHA256,
            'radial_wipe': {
                'source': 'radial_wipe_half_plane.aep: independently authored Adobe 26.5x89',
                'source_sha256': hashlib.sha256(radial_source).hexdigest(),
                'receipt': 'radial_wipe_definition-receipt.json: fresh default effect readback',
                'receipt_sha256': RADIAL_RECEIPT_SHA256,
            },
        },
        'effects': definitions,
    }
    output = json.dumps(result, ensure_ascii=False, indent=2) + '\n'
    if args.check:
        if DEST.read_text() != output:
            raise ValueError('generated effect registry is stale')
    else:
        DEST.write_text(output)
    print(f'{len(definitions)} effects, {sum(len(d["parameters"]) for d in definitions)} parameters → {DEST}')


if __name__ == '__main__':
    main()
