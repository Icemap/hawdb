"""Summarize only producer-verified final-source comparisons, preserving adverse data."""
from pathlib import Path
import hashlib,json,time,math
B=Path('/private/tmp/hawdb-wip-ready-20261015');P=B/'final-github-matrix-collection-0c298984/ordinary/summary.json';raw=P.read_bytes();s=json.loads(raw)
a={'engine':s['engine'],'engine_tree':s['engine_tree'],'source':s['source'],'source_summary_sha256':hashlib.sha256(raw).hexdigest(),'observed_at':time.time(),'observed_complete_groups':s['observed_complete_groups'],'expected_groups':90,'comparable':[],'adverse':[],'non_comparable':[],'scope':'Producer-verified original ordinary observations. Ratios are baseline/candidate medians, three samples per side. None means unavailable, not zero. No significance, causality, universal improvement or acceptance claim.'}
for bundle in s['reports']:
 assert bundle['verified_uploaded_producers_and_manifests']==12
 for g in bundle['groups']:
  row={'artifact':bundle['artifact'],'selection':[g['case'],g['layer'],g['backend'],g['size']],'result':g}
  if not g['qualified']:a['non_comparable'].append(row);continue
  assert g['before_samples']==g['after_samples']==3
  ratios={k:g.get(k) for k in ['elapsed_ns_median_speedup','query_boundary_ns_median_speedup','write_boundary_ns_median_speedup','read_boundary_ns_median_speedup']}
  for value in ratios.values():assert value is None or type(value) in [int,float] and math.isfinite(value) and value>0
  row['adverse_metrics']={k:v for k,v in ratios.items() if v is not None and v<1}
  a['comparable'].append(row)
  if row['adverse_metrics']:a['adverse'].append(row)
(B/'986-0c298984-current-ordinary-comparison-observations.json').write_text(json.dumps(a,indent=2)+'\n');print('Comparable',len(a['comparable']),'adverse',len(a['adverse']),'non-comparable',len(a['non_comparable']),'of90')
for r in a['adverse']:print(r['selection'],r['adverse_metrics'])
