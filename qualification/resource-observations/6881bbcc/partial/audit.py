"""Summarize all audited predecessor resource groups without changing acceptance."""
from pathlib import Path
import collections,hashlib,json,time,statistics
B=Path('/private/tmp/hawdb-wip-ready-20261015');D=B/'composed-github-matrix-collection-6881bbcc/resources';p=D/'summary.json';s=json.loads(p.read_text());assert s['engine']=='6881bbcc9809a6195d1bab38fb489aa3684c20fc' and s['source']=='2a1a677a117a526a06cb2eb5dae224be5ece8624'
bundles={}
for path in D.glob('artifact-*.json'):
 b=json.loads(path.read_text());g=tuple(b['group_selection']['group']);assert g not in bundles;bundles[g]=(b,path)
paired=[];refused=[];partial=[];numeric=[];positive_failures=[];imbalances=[]
for r in s['reports']:
 g=r['group'];b,path=bundles[tuple(g)]
 if not r.get('complete'):partial.append({'group':g,'classification':r.get('classification','incomplete'),'artifact':b['artifact']});continue
 if g[0]=='numeric':
  for mode,m in r['modes'].items():
   item={'group':g,'mode':mode,'success':m['success'],'expected_success':m['expected_success'],'row_boundary_required':m['row_boundary_required'],'row_boundary_proven':m['row_boundary_proven'],'acceptance_failure':m['acceptance_failure'],'artifact':b['artifact'],'process_peak_rss_bytes':m['process_peak_rss_bytes'],'observed_latency':m['observed_latency'],'actual_limits':m['actual_limits'],'refusal_counts':dict(collections.Counter(m['adverse']))}
   raw=b['reports'][mode+'.stdout']['records'];assert len(raw)==31
   profiles=[x.get('native_profile') for x in raw];item['native_profile_instrumented']=all(isinstance(x,dict) for x in profiles)
   if item['native_profile_instrumented']:
    item['native_profile_medians']={key:statistics.median(x[key] for x in profiles) for key in profiles[0] if all(isinstance(x.get(key),(int,float)) and not isinstance(x.get(key),bool) for x in profiles)}
    fields=['parameter_conversion_ns','engine_call_ns','result_conversion_ns'];item['legacy_phase_coverage']={key:('missing' if any(key not in x for x in profiles) else 'positive_observed' if any(x[key]>0 for x in profiles) else 'zero_without_callback_coverage') for key in fields}
   numeric.append(item)
   if m['acceptance_failure']:positive_failures.append(item)
  continue
 if not all(v['success'] for v in r['sides'].values()):
  refused.append({'group':g,'artifact':b['artifact'],'adverse':{side:v['adverse'] for side,v in r['sides'].items()}});continue
 row={'group':g,'artifact':b['artifact'],'samples_per_side':3,'baseline':{},'candidate':{},'ratios_candidate_over_baseline':{},'phase_availability':{}}
 for side,v in r['sides'].items():
  values=v['observed_medians'];profile=values['native_profile'];row[side].update(values)
  raw=[x for x in b['reports'][side+'/report.json']['records'] if not x['discarded'] and x['status']=='ok'];assert len(raw)==3
  for x in raw:
   n=x['native_profile'];delta=n['live_requested_bytes_after']-n['live_requested_bytes_before']
   if delta or n['allocated_bytes']!=n['deallocated_bytes']:imbalances.append({'group':g,'side':side,'iteration':x['iteration'],'live_delta':delta,'allocated_minus_deallocated':n['allocated_bytes']-n['deallocated_bytes'],'scope':'Process-global native observation window; not proof of a per-query leak.'})
  if g[-1]=='go':
   assert all(x['go_host_profile']['valid'] for x in raw)
   fields=['parameter_conversion_calls','parameter_conversion_ns','ffi_calls','ffi_call_ns','result_conversion_calls','result_conversion_ns']
   assert all(x['go_host_profile'][k]>0 for x in raw for k in ['parameter_conversion_calls','ffi_calls','result_conversion_calls'])
   row[side]['go_host_profile']={k:statistics.median(x['go_host_profile'][k] for x in raw) for k in fields}
  for field in ['parameter_conversion_ns','engine_call_ns','result_conversion_ns']:
   samples=[x['native_profile'].get(field) for x in raw];row['phase_availability'][side+':'+field]=('missing' if any(x is None for x in samples) else 'zero_without_callback_coverage' if not any(samples) else 'positive_observed')
 for field in ['process_peak_rss_bytes','query_boundary_ns','go_allocations','go_allocated_bytes']:
  before=row['baseline'].get(field);after=row['candidate'].get(field)
  if isinstance(before,(int,float)) and before>0 and isinstance(after,(int,float)):row['ratios_candidate_over_baseline'][field]=after/before
 for field in ['allocation_calls','allocated_bytes','reallocation_calls','parameter_conversion_ns','engine_call_ns','result_conversion_ns']:
  before=row['baseline']['native_profile'].get(field);after=row['candidate']['native_profile'].get(field)
  if isinstance(before,(int,float)) and before>0 and isinstance(after,(int,float)):row['ratios_candidate_over_baseline']['native_'+field]=after/before
 if g[-1]=='go':
  for field in ['parameter_conversion_ns','ffi_call_ns','result_conversion_ns']:
   before=row['baseline']['go_host_profile'][field];after=row['candidate']['go_host_profile'][field]
   if before>0:row['ratios_candidate_over_baseline']['go_host_'+field]=after/before
 paired.append(row)
result={'observed':time.time(),'engine':s['engine'],'engine_tree':s['engine_tree'],'source':s['source'],'run':s['run'],'summary_sha256':hashlib.sha256(p.read_bytes()).hexdigest(),'complete_groups':s['observed_complete_groups'],'paired_ordinary':paired,'ordinary_refusals':refused,'partial_groups':partial,'numeric_mode_outcomes':numeric,'numeric_acceptance_failures':positive_failures,'process_global_imbalances':imbalances,'scope':'All currently audited predecessor resource observations. Instrumented3-sample medians are separate from ordinary latency; RSS includes startup/fixture/full producer operation. No per-query isolation, causal attribution, significance, final-source acceptance or zero-cost claim for uncovered phases. Counted complete refusals remain failures/controls, not successful timing samples.'}
out=B/'resource-6881bbcc-all-current-comparisons.json';temp=out.with_suffix('.tmp');temp.write_text(json.dumps(result,indent=2)+'\n');temp.replace(out)
print(json.dumps({'complete_groups':result['complete_groups'],'paired_ordinary':len(paired),'ordinary_refused_groups':len(refused),'incomplete_groups':len(partial),'numeric_modes':dict(collections.Counter('success' if x['success'] else 'refusal' for x in numeric)),'numeric_acceptance_failures':len(positive_failures),'native_process_global_imbalance_observations':len(imbalances),'paired_process_RSS_higher':sum(x['ratios_candidate_over_baseline']['process_peak_rss_bytes']>1 for x in paired),'paired_native_allocations_higher':sum(x['ratios_candidate_over_baseline']['native_allocation_calls']>1 for x in paired)},indent=2))
