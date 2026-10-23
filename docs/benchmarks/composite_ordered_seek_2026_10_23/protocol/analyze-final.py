import json,statistics as st
from pathlib import Path
Q=Path('/private/tmp/hawdb-ordered-seek-qualification')
summary={'source':'cac072479786f01c658022916e3ada75314ed129','ordered':[],'point':[],'setup':[]}
for n in (64,1000,10000,100000):
 for backend in ('memory','disk'):
  p=Q/f'measured-final-{n}-{backend}.jsonl'
  rows=[json.loads(s) for s in p.read_text().splitlines()]
  assert len(rows)==48
  summary['setup'].append({'rows':n,'backend':backend,'seconds':rows[0]['setup_ns']/1e9})
  for desc in (False,True):
   for depth in (0,n//2,n-2):
    data=[r for r in rows if not r['warmup'] and r['descending']==desc and r['depth']==depth]
    assert len(data)==7
    paths=[{s['path']:s for s in r['samples']} for r in data]
    ordered=st.median(p['ordered']['ns'] for p in paths)
    reference=st.median(p['reference']['ns'] for p in paths)
    summary['ordered'].append({'rows':n,'backend':backend,'direction':'DESC' if desc else 'ASC','depth':depth,'output_rows':data[0]['output_rows'],'ordered_us':ordered/1000,'reference_us':reference/1000,'median_ratio':ordered/reference,'paired_ratio_range':[min(p['ordered']['ns']/p['reference']['ns'] for p in paths),max(p['ordered']['ns']/p['reference']['ns'] for p in paths)],'ordered_decodes_range':[min(p['ordered']['index_entries_decoded'] for p in paths),max(p['ordered']['index_entries_decoded'] for p in paths)],'ordered_blocks_range':[min(p['ordered']['index_blocks'] for p in paths),max(p['ordered']['index_blocks'] for p in paths)],'ordered_descriptors_range':[min(p['ordered']['index_descriptors'] for p in paths),max(p['ordered']['index_descriptors'] for p in paths)],'ordered_index_bytes_range':[min(p['ordered']['index_bytes'] for p in paths),max(p['ordered']['index_bytes'] for p in paths)],'ordered_query_peak':max(p['ordered']['query_peak_bytes'] for p in paths),'reference_query_peak':max(p['reference']['query_peak_bytes'] for p in paths),'ordered_blocking_peak':max(p['ordered']['blocking_peak_bytes'] for p in paths),'reference_blocking_peak':max(p['reference']['blocking_peak_bytes'] for p in paths)})
points=[json.loads(s) for s in (Q/'point-pairs-final.jsonl').read_text().splitlines()]
assert len(points)==20
for composite in (False,True):
 pairs=[]
 for pair in range(5):
  matches={x['kind']:x['samples'] for x in points if x['pair']==pair and x['composite']==composite}
  for rows in matches.values():
   assert len(rows)==11
   assert all(r['hits']==1000 and r['misses']==0 and r['iterations']==1000 and r['checksum']==7380 for r in rows)
  a=st.median(r['ns'] for r in matches['baseline']);b=st.median(r['ns'] for r in matches['candidate'])
  pairs.append({'pair':pair,'baseline_us_per_lookup':a/1e6,'candidate_us_per_lookup':b/1e6,'ratio':b/a})
 summary['point'].append({'composite':composite,'pairs':pairs,'median_paired_ratio':st.median(p['ratio'] for p in pairs)})
(Q/'analysis-final.json').write_text(json.dumps(summary,indent=2)+'\n')
print(json.dumps({'ordered_ratio_range':[min(x['median_ratio'] for x in summary['ordered']),max(x['median_ratio'] for x in summary['ordered'])],'point':summary['point'],'setup':summary['setup']},indent=2))
