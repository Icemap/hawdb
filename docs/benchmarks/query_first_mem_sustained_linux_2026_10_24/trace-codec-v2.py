# Copyright 2026 Nowledge
# SPDX-License-Identifier: Apache-2.0

"""Losslessly pack/restore benchmark traces; restored JSON must match its SHA-256.

Only per-worker samples use a compact representation. Each sample is three
unsigned varints: acknowledged-write delta, elapsed nanoseconds, start-time delta.
Worker counts and all other JSON fields retain their original order in metadata.
"""
import argparse
import gzip
import hashlib
import io
import json
from pathlib import Path
import tarfile


def canonical(value):
    return (json.dumps(value, ensure_ascii=False, separators=(',', ':')) + '\n').encode()


def put(value, output):
    assert type(value) is int and 0 <= value < 2**64, value
    while value >= 128:
        output.append((value & 127) | 128)
        value >>= 7
    output.append(value)


def get(data, offset):
    value = shift = 0
    while offset < len(data) and shift < 70:
        byte = data[offset]
        offset += 1
        value |= (byte & 127) << shift
        if byte < 128:
            assert value < 2**64
            return value, offset
        shift += 7
    raise ValueError('Invalid or truncated unsigned varint')


def encode(raw):
    document = json.loads(raw)
    assert canonical(document) == raw, 'Unexpected source JSON serialization'
    counts, data = [], bytearray()
    for worker in document['workers']:
        samples = worker['samples']
        counts.append(len(samples))
        acknowledged = started = 0
        for sample in samples:
            assert list(sample) == ['acknowledged_writes', 'elapsed_ns', 'started_ns']
            put(sample['acknowledged_writes'] - acknowledged, data)
            put(sample['elapsed_ns'], data)
            put(sample['started_ns'] - started, data)
            acknowledged, started = sample['acknowledged_writes'], sample['started_ns']
        worker['samples'] = []
    metadata = {'encoding': 'hawdb-v2-sample-varints-1', 'raw_sha256': hashlib.sha256(raw).hexdigest(),
                'sample_counts': counts, 'document': document}
    return canonical(metadata), bytes(data)


def decode(metadata, data):
    value = json.loads(metadata)
    assert value['encoding'] == 'hawdb-v2-sample-varints-1'
    counts, document = value['sample_counts'], value['document']
    assert len(counts) == len(document['workers'])
    assert all(type(n) is int and n >= 0 for n in counts)
    assert sum(counts) * 3 <= len(data)
    offset = 0
    for worker, count in zip(document['workers'], counts):
        assert worker['samples'] == []
        acknowledged = started = 0
        for _ in range(count):
            delta, offset = get(data, offset)
            acknowledged += delta
            elapsed, offset = get(data, offset)
            delta, offset = get(data, offset)
            started += delta
            worker['samples'].append({'acknowledged_writes': acknowledged, 'elapsed_ns': elapsed, 'started_ns': started})
    assert offset == len(data)
    raw = canonical(document)
    assert hashlib.sha256(raw).hexdigest() == value['raw_sha256'], 'Restored trace hash mismatch'
    return raw


def add_bytes(archive, name, data):
    info = tarfile.TarInfo(name)
    info.size = len(data)
    info.mode = 0o644
    archive.addfile(info, io.BytesIO(data))


def pack(run, output, kind):
    receipts = sorted(run.glob(f'*-{kind}-*-receipt.json'))
    assert len(receipts) == 48
    with output.open('wb') as dst, gzip.GzipFile(fileobj=dst, mode='wb', mtime=0, compresslevel=6) as zipped, tarfile.open(fileobj=zipped, mode='w') as archive:
        for receipt in receipts:
            record = json.loads(receipt.read_text())
            assert record['validated'] and record['exit_code'] == 0
            label = record['label']
            raw = (run / (label + '-raw.json')).read_bytes()
            assert hashlib.sha256(raw).hexdigest() == record['raw_sha256']
            metadata, data = encode(raw)
            assert decode(metadata, data) == raw
            add_bytes(archive, label + '-compact.json', metadata)
            add_bytes(archive, label + '-samples.bin', data)
            for suffix in ['-job.json', '-receipt.json', '-stderr.log']:
                add_bytes(archive, label + suffix, (run / (label + suffix)).read_bytes())
    print(kind, '48 byte-exact round trips; archive bytes:', output.stat().st_size, flush=True)


def write_checked(path, data):
    if path.exists():
        assert path.read_bytes() == data, f'Refusing to replace different file {path}'
    else:
        path.write_bytes(data)


def restore(archives, output):
    output.mkdir(parents=True, exist_ok=True)
    restored = 0
    for path in archives:
        protocol = path.parent / 'protocol.json'
        if protocol.exists():
            write_checked(output / 'protocol.json', protocol.read_bytes())
        with tarfile.open(path, 'r:gz') as archive:
            for member in archive:
                assert member.isfile() and member.name == Path(member.name).name
                if member.name.endswith('-samples.bin'):
                    continue
                content = archive.extractfile(member).read()
                if member.name.endswith('-compact.json'):
                    label = member.name.removesuffix('-compact.json')
                    data = archive.extractfile(label + '-samples.bin').read()
                    write_checked(output / (label + '-raw.json'), decode(content, data))
                    restored += 1
                else:
                    assert member.name.endswith(('-job.json', '-receipt.json', '-stderr.log'))
                    write_checked(output / member.name, content)
    print('Restored and hash-verified traces:', restored, flush=True)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='command', required=True)
    packing = commands.add_parser('pack')
    packing.add_argument('run', type=Path)
    packing.add_argument('output', type=Path)
    packing.add_argument('kind', choices=['point', 'report', 'stream'])
    restoring = commands.add_parser('restore')
    restoring.add_argument('archives', type=Path, nargs='+')
    restoring.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if args.command == 'pack':
        pack(args.run, args.output, args.kind)
    else:
        restore(args.archives, args.output)
