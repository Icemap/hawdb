import json
import statistics
import sys
from pathlib import Path

root = Path(sys.argv[1])
rows = [json.loads(line) for line in (root / 'results.jsonl').read_text().splitlines()]
assert len(rows) == 54
summaries = []
for documents in [32, 512]:
    for writers in [1, 4, 8]:
        groups = {mode: sorted([r for r in rows if r['documents'] == documents and r['writers'] == writers and r['mode'] == mode], key=lambda r: r['run']['round']) for mode in ['disabled', 'scoped-blocking', 'detached']}
        assert all(len(group) == 3 for group in groups.values())
        blocking, detached = groups['scoped-blocking'], groups['detached']
        endpoints = {}
        for metric, fields in [('foreground_write', ['p50_ns', 'p95_ns', 'p99_ns']), ('writer_wait_plus_entry', ['p95_ns', 'p99_ns']), ('quiescent_publication_exclusive_callback', ['p50_ns', 'p95_ns', 'p99_ns'])]:
            for field in fields:
                ratios = [d[metric][field] / b[metric][field] for b, d in zip(blocking, detached)]
                endpoints[f'{metric}.{field}'] = {'blocking_median_ms': statistics.median(r[metric][field] for r in blocking) / 1e6, 'detached_median_ms': statistics.median(r[metric][field] for r in detached) / 1e6, 'paired_ratios': ratios, 'paired_median_ratio': statistics.median(ratios)}
        summaries.append({'documents': documents, 'writers': writers, 'endpoints': endpoints, 'detached_active_post_e_advances': [r['simultaneous_committed_post_e_advances'] for r in detached], 'detached_writes_during_native_construction': [sum(a['writes_after_native_construction'] - a['writes_before_native_construction'] for a in r['publication_attempts']) for r in detached], 'whole_phase_seconds': {mode: [r['elapsed_concurrent_phase_ns'] / 1e9 for r in group] for mode, group in groups.items()}, 'all_detached_progress_qualified': all(r['simultaneous_progress_qualified'] for r in detached)})
(root / 'comparison.json').write_text(json.dumps(summaries, indent=2) + '\n')
print(json.dumps({'cases': len(rows), 'all_progress_qualified': all(r['all_detached_progress_qualified'] for r in summaries)}, indent=2))
