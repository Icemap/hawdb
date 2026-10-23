import argparse,collections,hashlib,json,math,pathlib,statistics
p=argparse.ArgumentParser();p.add_argument('run');a=p.parse_args();root=pathlib.Path(a.run)
def quantile(samples,p):
 if not samples:return None
 return sorted(samples)[max(0,math.ceil(len(samples)*p)-1)]
def latency(samples):
 values=[s['elapsed_ns'] for s in samples]
 return dict(calls=len(values),p50_ns=quantile(values,.5),p95_ns=quantile(values,.95),p99_ns=quantile(values,.99),max_ns=max(values,default=0))
def histogram(before,after):
 buckets=[y-x for x,y in zip(before['buckets'],after['buckets'])];assert all(x>=0 for x in buckets)
 count=sum(buckets);total=after['total_nanos']-before['total_nanos'];assert total>=0
 def bound(p):
  if not count:return None
  threshold=math.ceil(count*p);cum=0
  for i,v in enumerate(buckets):
   cum+=v
   if cum>=threshold:return [0 if i==0 else 2**(i-1)+1,2**i if i<63 else 2**64-1]
 return dict(count=count,total_ns=total,mean_ns=total/count if count else None,p95_ns_bounds=bound(.95),p99_ns_bounds=bound(.99),bucket_deltas=buckets)
runs=[]
for file in sorted(root.glob('*-receipt.json')):
 receipt=json.loads(file.read_text());label=receipt['label'];raw_file=root/(label+'-raw.json');d=json.loads(raw_file.read_text());job=d['job']
 assert receipt['exit_code']==0 and d['errors']==0 and d['durable_reopen']
 assert d['successful_writes']==job['writers']*job['writer_iterations']
 assert d['reads']['calls']==job['readers']*job['reader_iterations']
 samples={key:[s for w in d['workers'] if w['writer']==writer for s in w['samples']] for key,writer in [('read',False),('write',True)]}
 start_write=min(s['started_ns'] for s in samples['write']);end_write=max(s['started_ns']+s['elapsed_ns'] for s in samples['write'])
 start_read=min(s['started_ns'] for s in samples['read']);end_read=max(s['started_ns']+s['elapsed_ns'] for s in samples['read'])
 active=[s for s in samples['read'] if s['started_ns']<end_write and s['started_ns']+s['elapsed_ns']>start_write]
 row=dict(label=label,side=label.rsplit('-',1)[1],pair=int(label.split('-')[4]),mode='instrumented' if d['instrumented'] else 'latency',read_kind=job['read_kind'],out_of_core=job['out_of_core'],writers=job['writers'],readers=job['readers'],source_binary_sha256=receipt['binary_sha256'],raw_sha256=hashlib.sha256(raw_file.read_bytes()).hexdigest(),elapsed_ns=d['elapsed_ns'],reads=d['reads'],writes=d['writes'],successful_writes=d['successful_writes'],durable_reopen=d['durable_reopen'],errors=d['errors'],cpu=d['cpu'],load_before=receipt['load_before'],load_after=receipt['load_after'],reads_overlapping_writer_lifetime=latency(active),writes_completing_during_reader_lifetime=sum(start_read<=s['started_ns']+s['elapsed_ns']<=end_read for s in samples['write']),reader_acknowledgement_ranges=[dict(index=w['index'],first=w['samples'][0]['acknowledged_writes'],last=w['samples'][-1]['acknowledged_writes'],distinct_observations=len({s['acknowledged_writes'] for s in w['samples']})) for w in d['workers'] if not w['writer']])
 if d['instrumented']:
  m=d['instrumentation'];row['allocation']={k:v for k,v in m.items() if k not in ['lock_before','lock_after']};row['owner']={k:histogram(m['lock_before'][k],m['lock_after'][k]) for k in ['read_wait','read_hold','write_wait','write_hold']};row['owner_write_hold_ns_per_successful_write']=row['owner']['write_hold']['total_ns']/d['successful_writes'];row['owner_write_wait_ns_per_successful_write']=row['owner']['write_wait']['total_ns']/d['successful_writes']
 runs.append(row)
groups=collections.defaultdict(dict)
for row in runs:groups[(row['read_kind'],row['out_of_core'],row['writers'],row['pair'],row['mode'])][row['side']]=row
pairs=[]
for key,v in groups.items():
 if set(v)!=set(['baseline','candidate']):continue
 b,c=v['baseline'],v['candidate'];ratios={f'{kind}_{percentile}':c[kind][percentile]/b[kind][percentile] for kind in ['reads','writes'] for percentile in ['p50_ns','p95_ns','p99_ns','max_ns']};ratios['elapsed_ns']=c['elapsed_ns']/b['elapsed_ns']
 if key[-1]=='instrumented':
  for x in ['allocation_calls','allocated_bytes']:ratios[x]=c['allocation'][x]/b['allocation'][x]
  for x in ['owner_write_hold_ns_per_successful_write','owner_write_wait_ns_per_successful_write']:ratios[x]=c[x]/b[x]
 pairs.append(dict(read_kind=key[0],out_of_core=key[1],writers=key[2],pair=key[3],mode=key[4],candidate_over_baseline=ratios))
output=dict(protocol='hawdb-mem-query-concurrency-analysis-v1',run_directory=str(root),complete=len(runs)==144,run_count=len(runs),runs=runs,pairs=pairs)
(root/'analysis.json').write_text(json.dumps(output,indent=2)+'\n')
print('runs',len(runs),'paired comparisons',len(pairs))
by_config=collections.defaultdict(list)
for r in pairs:
 if r['mode']=='latency':by_config[(r['read_kind'],r['out_of_core'],r['writers'])].append(r['candidate_over_baseline'])
for key,rs in by_config.items():
 print(key,'pairs',len(rs),{k:round(statistics.median(r[k] for r in rs),3) for k in ['reads_p95_ns','reads_p99_ns','writes_p95_ns','writes_p99_ns','elapsed_ns']})
