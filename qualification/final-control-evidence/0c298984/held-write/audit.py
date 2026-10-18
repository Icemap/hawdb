from pathlib import Path
import hashlib,json,statistics,math,time,subprocess
B=Path('/private/tmp/hawdb-wip-ready-20261015');W=B/'review-disposition-41db';s=json.loads((B/'986-final-held-write-controls-0c298984-state.json').read_text());assert s['engine']=='0c2989842321d10e91f136f3c924badb07038add';assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=W,text=True).strip()==s['engine'];assert not subprocess.check_output(['git','status','--porcelain'],cwd=W,text=True).strip()
for p in s.get('packages',{}).values():
 F=Path(p['path']);assert all(hashlib.sha256((F/f).read_bytes()).hexdigest()==h for f,h in p['files_sha256'].items())
node_modes=('ordinary','closed_source','owned_result','retained_buffers');numeric_modes=('ordinary','closed_cursor','open_cursor');results=[];pending=[];errors=[]
def empty(v):return v is None or all(v[k]==0 for k in ['retained_bytes','view_handles','buffer_owners'])
for row in s['records']:
 if row.get('exit') is None or 'sha256' not in row:pending.append([row['kind'],row['rows'],row['backend'],row['producer']]);continue
 try:
  path=Path(row['stdout']);raw=path.read_bytes();assert row['exit']==0 and hashlib.sha256(raw).hexdigest()==row['sha256'];assert not Path(row['stderr']).read_text();r=json.loads(raw);kind=row['kind'];node=kind=='node';modes=node_modes if node else numeric_modes;n=row['rows'];pad=row['pad_bytes'];samples=8 if node else 7
  assert r['status']=='ok' and r['rows']==n and r['pad_bytes']==pad and r['backend']==row['backend'] and r['samples']==samples and r['mode']=='all' and r['prefetch']==0 and r['durability']=='SyncOnEveryWrite'
  records=r['records'];assert len(records)==(samples+1)*len(modes);order=[(i,m) for i in range(samples+1) for m in modes[i%len(modes):]+modes[:i%len(modes)]];assert [(x['iteration'],x['mode']) for x in records]==order
  measured=[x for x in records if not x['discarded']];assert len(measured)==samples*len(modes)
  for x in records:
   i=x['iteration'];assert x['discarded']==(i==0) and type(x['write_boundary_ns']) is int and x['write_boundary_ns']>0
   if node:
    assert r['profiled']==(row['producer']=='instrumented') and r['hold_rows']==32 and x['updated_rows']==32 and x['verified_new_score']==i+1 and empty(x['retained_after_release'])
    assert (x['retained_before_write']['view_handles']>0)==(x['mode']=='retained_buffers');assert x['verified_old_score']==(i if x['mode'] in ['owned_result','retained_buffers'] else None)
    expected_pad=hashlib.sha256(('x'*pad+'\x00界🙂').encode()).hexdigest();assert x['payload_sha256']==expected_pad;assert len(x['point_write_ns'])==32 and all(type(v) is int and v>0 for v in x['point_write_ns']) and sum(x['point_write_ns'])==x['write_boundary_ns']
    allocations=x['write_native_allocations'];assert (allocations is not None)==r['profiled']
    if allocations is not None:assert all(type(v) is int and v>=0 for v in allocations.values())
   else:
    assert x['updated_rows']==n and x['verified_score']==i+1 and empty(x['retained_after'])
    if x['mode']=='ordinary':assert x['source_before_write'] is None
    else:assert x['source_before_write']['visited_rows']==0 and x['source_before_write']['source_pinned_capacity_bytes']>=n*pad
  stats={};paired={}
  for mode in modes:
   selected=[x for x in measured if x['mode']==mode];times=[x['write_boundary_ns'] for x in selected];observed=r['summary'][mode]
   if node:
    points=sorted(v for x in selected for v in x['point_write_ns']);assert len(points)==256 and observed['point_samples']==256 and observed['median_round_write_ns']==statistics.median(times)
    for p,key in [(.5,'point_p50_ns'),(.95,'point_p95_ns'),(.99,'point_p99_ns')]:assert observed[key]==points[int((len(points)-1)*p)]
    if r['profiled']:
     assert observed['median_write_native_allocated_bytes']==statistics.median(x['write_native_allocations']['allocated_bytes'] for x in selected)
     observed['computed_median_native_allocation_calls']=statistics.median(x['write_native_allocations']['allocation_calls'] for x in selected)
   else:assert observed['median_write_boundary_ns']==statistics.median(times) and observed['minimum_write_boundary_ns']==min(times)
   stats[mode]=observed
   if mode!='ordinary':
    ratio=[next(x['write_boundary_ns'] for x in selected if x['iteration']==i)/next(x['write_boundary_ns'] for x in measured if x['mode']=='ordinary' and x['iteration']==i) for i in range(1,samples+1)];paired[mode]={'candidate_over_no_reader_ratios':ratio,'median':statistics.median(ratio),'maximum':max(ratio)}
  assert type(r['process_peak_rss_bytes']) is int and r['process_peak_rss_bytes']>0;results.append({'kind':kind,'rows':n,'pad_bytes':pad,'backend':row['backend'],'producer':row['producer'],'report_sha256':row['sha256'],'measured_rounds':len(measured),'measured_point_writes':len(measured)*32 if node else None,'all_retained_quotas_released':True,'stats':stats,'paired_ratios':paired,'process_peak_rss_bytes':r['process_peak_rss_bytes']})
 except Exception as e:errors.append({'selection':[row['kind'],row['rows'],row['backend'],row['producer']],'kind':type(e).__name__,'detail':str(e)[:1000]})
expected={(k,n,b,p) for k,ps in [('node',['ordinary','instrumented']),('numeric',['ordinary'])] for n in [1000,10000] for b in ['memory','file'] for p in ps};seen={(r['kind'],r['rows'],r['backend'],r['producer']) for r in results};assert seen<=expected
out={'engine':s['engine'],'tree':s['tree'],'build_command':s['steps'][0]['command'],'packages':s.get('packages',{}),'groups':results,'pending':pending,'missing':sorted(expected-seen),'errors':errors,'complete':seen==expected and not errors,'observed_at':time.time(),'scope':'Existing exact-consumer final-source Node8 and numeric4 held-write controls. Same-source mode comparisons, rotated rounds and safe durability. Instrumented observations separate from ordinary timings; RSS includes all fixtures/setup, no per-mode attribution, production tails, universal COW cost, causality, leak absence, full90/110 or approval claim.'};(B/'986-final-held-write-controls-0c298984-audit.json').write_text(json.dumps(out,indent=2)+'\n');print('audited',len(results),'of12','errors',errors,'pending',len(pending))
for r in results:
 key='retained_buffers' if r['kind']=='node' else 'open_cursor';print(r['kind'],r['rows'],r['backend'],r['producer'],'held/no-reader',round(r['paired_ratios'][key]['median'],5))
