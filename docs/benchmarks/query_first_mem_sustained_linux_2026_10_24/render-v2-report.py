"""Render complete sustained-reader measurements without discarding any pair."""
import argparse, collections, json, pathlib, statistics
p=argparse.ArgumentParser();p.add_argument('run');args=p.parse_args()
run=pathlib.Path(args.run);a=json.loads((run/'analysis.json').read_text());protocol=json.loads((run/'protocol.json').read_text())
assert a['complete'] and a['run_count']==protocol['expected_processes']
assert not protocol['pilot'] and a['run_count']==144
assert all(r['errors']==0 and r['durable_reopen'] for r in a['runs'])
med=statistics.median
groups=collections.defaultdict(list)
for pair in a['pairs']:groups[(pair['read_kind'],pair['out_of_core'],pair['writers'],pair['mode'])].append(pair)
assert len(groups)==24 and all(len(x)==3 for x in groups.values())
configs=sorted({key[:3] for key in groups})
def label(key):return [key[0], 'out-of-core' if key[1] else 'materialized',str(key[2])]
def runs(key,mode,side):return [r for r in a['runs'] if (r['read_kind'],r['out_of_core'],r['writers'])==key and r['mode']==mode and r['side']==side]
def ratio(key,metric):return med(x['candidate_over_baseline'][metric] for x in groups[(*key,'latency')])
def row(values):return '| '+' | '.join(str(v) for v in values)+' |'
lines=['# Sustained-reader ordinary Mem query API comparison','','Issue Number: ref #1193','','## Scope and status','',
'Complete v2 matrix: 144 processes, 72 alternating A/B pairs, zero query errors and exact durable-reopen checks. This is measurement evidence; it does not complete issue acceptance. The fixed-work v1 report and its regressions remain valid and are retained separately.','',
'Candidate: `c669b8d186574d2cda319089601d630910cfe919`; baseline: `8e9eab185ef880794d216d99ac6cd57442c511aa` plus only the identical benchmark and observational owner/preparation probes. Later test-fixture cleanup is not part of this measured source revision.','',
'Each process uses two readers, one or two writers, 256 rows and the default `SyncOnEveryWrite` durability. Each writer performs 1,000 updates to its own unique key. Each reader performs at least 4,000 calls and continues until all writers finish. Three pairs alternate baseline/candidate order for each entry/residency/writer configuration and each instrumentation mode. Setup, warmup, checkpoint and durable reopen are outside the latency window; process receipts include their cost.','',
'The sustained-reader design keeps read pressure during the writer lifetime. It permits different total read counts and work mixes between A/B runs. Elapsed time is not a fixed-total-work throughput ratio, and raw allocation totals are not equal-work costs.','',
'Host: AMD Ryzen 7 7735HS, 8 physical cores / 16 logical CPUs; Linux 7.1.10-zen1-1-zen, pinned Rust 1.97.1, NVMe/Btrfs, boost enabled. Runtime reports effective parallelism 16 with no detected quota or cpuset cap. No compilation or fuzz was started during the paired timing window. This shared developer host has uncontrolled frequency and external activity; per-process load, memory, swap and page-fault observations are retained.','',
'## Uninstrumented latency','',
'Cells are medians of three paired candidate/baseline ratios, not ratios of pooled quantiles. Below 1 means lower. All pairs, including slow runs, are retained; absolute observations and per-worker tails are in `analysis.json.gz`. Instrumented timings are excluded.','',
row(['Read entry','Residency','Writers','Read p95','Read p99','Write p95','Write p99','Elapsed']),row(['---','---','---:','---:','---:','---:','---:','---:'])]
regressions=[]
for key in configs:
 vals=[ratio(key,m) for m in ['reads_p95_ns','reads_p99_ns','writes_p95_ns','writes_p99_ns','elapsed_ns']]
 lines.append(row(label(key)+[f'{v:.3f}' for v in vals]))
 if any(v>1 for v in vals[:-1]):regressions.append(key)
lines+=['',f'{len(regressions)} of 12 configurations have at least one median p95/p99 ratio above 1. This matrix does not establish a strict no-regression pass. Improvements in other quantiles or elapsed time cannot establish an across-the-board latency improvement.','',
'The single-writer materialized report case illustrates the variation: write p99 is 31.845 / 6.203 ms (baseline / candidate) in pair 0, 12.261 / 105.251 ms in pair 1, and 8.991 / 104.252 ms in pair 2. All three remain in the paired median. Their read counts differ substantially, and this shared-host experiment does not isolate the cause of the stalls.','',
'## Simultaneous progress and fairness','',
'The overlap cohort includes every read whose call interval intersects the first-to-last writer-call interval. Progress counts use call completion timestamps in ten equal deciles of that writer lifetime. The table shows median overlap cohort size, median overlap-tail ratios, and the minimum number of deciles with both read and write completions across three runs. Nonzero activity in a decile does not bound stalls within it. Baseline overlap cohorts as small as four calls make their p95/p99 unstable; the ratios are descriptive and are not percentile acceptance for those small cohorts.','',
row(['Read entry','Residency','Writers','Overlap reads B / C','Overlap p95 ratio','Overlap p99 ratio','Active deciles B / C']),row(['---','---','---:','---:','---:','---:','---:'])]
for key in configs:
 bs,cs=(runs(key,'latency',side) for side in ['baseline','candidate'])
 counts=[med(r['reads_overlapping_writer_lifetime']['calls'] for r in rs) for rs in [bs,cs]]
 deciles=[min(sum(d['reader_completions']>0 and d['writer_completions']>0 for d in r['writer_lifetime_deciles']) for r in rs) for rs in [bs,cs]]
 lines.append(row(label(key)+[f'{counts[0]:.0f} / {counts[1]:.0f}',f'{ratio(key,"reads_overlapping_writer_lifetime_p95_ns"):.3g}',f'{ratio(key,"reads_overlapping_writer_lifetime_p99_ns"):.3g}',f'{deciles[0]} / {deciles[1]}']))
lines+=['','`worker_latency` retains each writer and reader\'s p95/p99, maximum call, maximum completion gap and last completion time. `reader_acknowledgement_ranges` samples the harness\'s global acknowledged-write counter at read completion; it is not a query-visible epoch or a Consumer watermark. Durable reopen verifies final writes. This workload therefore demonstrates ordinary read/write completion overlap, not Consumer catch-up or snapshot-freshness qualification.','',
'## Direct preparation and allocation observations','',
'Qualification builds count calls and nanoseconds at the Cypher parser and shared optimized-plan preparation boundaries. Plan-call time includes lookup/binding work, not only optimizer cache misses. Counters are subtracted over the measurement window. These diagnostic timings are not used in the release latency table. The table gives medians per successful read-or-write call; the A/B call mix differs, so preparation/allocation time and bytes are descriptive, not controlled per-reader/per-writer comparisons.','',
row(['Read entry','Residency','Writers','Parse calls B / C','Plan calls B / C','Parse us B / C','Plan us B / C','Allocated bytes B / C']),row(['---','---','---:','---:','---:','---:','---:','---:'])]
for key in configs:
 rs=[runs(key,'instrumented',side) for side in ['baseline','candidate']]
 pairs=[]
 for metric,scale in [('cypher_parse_calls',1),('cypher_plan_calls',1),('cypher_parse_nanos',1000),('cypher_plan_nanos',1000)]:
  values=[med(r['preparation_per_call'][metric]/scale for r in side) for side in rs];pairs.append(f'{values[0]:.3f} / {values[1]:.3f}')
 values=[med(r['allocation_per_call']['allocated_bytes'] for r in side) for side in rs];pairs.append(f'{values[0]:.0f} / {values[1]:.0f}')
 lines.append(row(label(key)+pairs))
lines+=['','Allocation counters cover process-wide Rust requested allocations, including benchmark sample-vector growth and thread cleanup. They exclude foreign heaps and allocator rounding and are not RSS. Instrumented sustained readers can allocate more samples; raw totals, reallocation counts and work counts remain in JSON.','',
'## Actual publication-owner locks','',
'Each cell reports baseline / candidate. Mean per-acquisition hold is derived from measured window totals. Percentiles are power-of-two histogram intervals; the displayed interval spans all three runs\' percentile bounds. Lifetime maxima are never subtracted. Acquisition counts and all read/write wait/hold buckets are retained in JSON.','',
row(['Read entry','Residency','Writers','Write hold mean us B / C','Write hold p95 us B / C','Write hold p99 us B / C','Write wait mean us B / C']),row(['---','---','---:','---:','---:','---:','---:'])]
for key in configs:
 rs=[runs(key,'instrumented',side) for side in ['baseline','candidate']]
 means=[med(r['owner']['write_hold']['mean_ns']/1000 for r in side) for side in rs]
 values=[f'{means[0]:.3f} / {means[1]:.3f}']
 for percentile in ['p95_ns_bounds','p99_ns_bounds']:
  intervals=[]
  for side in rs:
   bounds=[r['owner']['write_hold'][percentile] for r in side]
   intervals.append(f'[{min(b[0] for b in bounds)/1000:.3f}, {max(b[1] for b in bounds)/1000:.3f}]')
  values.append(' / '.join(intervals))
 waits=[med(r['owner']['write_wait']['mean_ns']/1000 for r in side) for side in rs];values.append(f'{waits[0]:.3f} / {waits[1]:.3f}')
 lines.append(row(label(key)+values))
lines+=['','Candidate writes acquire the owner for workspace capture and publication; baseline report reads also take its write lock through execution. These counts represent real owner intervals, but differing operation attribution prevents calling the aggregate a pure commit-publication service time. Preparation no longer needs to hold the owner for the entire query, but this benchmark does not attribute every remaining write-tail stall.','',
'## Reproducibility and remaining qualification','',
'- `raw-point.tar.gz`, `raw-report.tar.gz` and `raw-stream.tar.gz` retain every workload, per-call trace and process receipt; the adjacent `protocol.json` binds the builds and run matrix. The analyzer validates raw SHA-256, successful write counts, minimum read counts and durable reopen before accepting a process. `SHA256SUMS` covers delivered evidence. Per-worker samples are losslessly encoded as three unsigned varints (acknowledgement delta, elapsed nanoseconds, start delta), with all remaining JSON fields preserved. Packing verified all 144 reconstructed JSON files byte-for-byte against the original SHA-256. Restore the original analyzer inputs with `python3 trace-codec-v2.py restore raw-point.tar.gz raw-report.tar.gz raw-stream.tar.gz --output restored`, then run `python3 analyze-v2-pairs.py restored`.','- Baseline and candidate use isolated build targets, identical benchmark source, source-validated Cargo artifacts and distinct binary hashes. Root and benchmark artifacts were actually rebuilt. Full build receipts and logs record first-build versus reused dependency cost. The host environment file was captured before v1; v2 revision/binary bindings are in `protocol.json`, and runtime CPU/cgroup observations are recorded per process.','- The earlier rejected shared-target executable alias is documented with v1; it is not used here.','- Process resource counters include setup/warmup/reopen; host swap/fault changes can include unrelated processes. Neither is an isolated query resource measurement.','- These observations do not qualify Consumer watermarks, search-projection callback latency, SQL/Tokio performance, long-read conflict-history reclamation or crash/power-loss schedules. Existing focused regression/model/platform suites own those requirements.','- P5 business API fixture removal and final-head CI are still incomplete. Keep PR #1194 in Draft.','',
'## Build receipts','',row(['Binary','Build seconds','SHA-256']),row(['---','---:','---'])]
for name,r in protocol['builds'].items():lines.append(row([name,f'{r["build_seconds"]:.3f}',f'`{r["binary_sha256"]}`']))
output=run.parent/'report-v2.md';output.write_text('\n'.join(lines)+'\n');print(output)
