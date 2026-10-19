from pathlib import Path
import json,hashlib,subprocess,sys
B=Path('/private/tmp/hawdb-wip-ready-20261015');O=B/'supplemental-python-failure-heap-0c298984';W=B/'qualify-resources-final-0c298984';base=B/'resource-qualification-v2/baseline'
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
manifest=json.loads((O/'source-manifest.json').read_text())
for f,h in manifest['files'].items():assert sha(O/f)==h
held=json.loads((B/'986-final-held-write-controls-0c298984-state.json').read_text())
assert held['engine']==manifest['production_head']
for f,h in held['packages']['instrumented']['files_sha256'].items():
 if f.startswith('hawdb/'):assert sha(O/'candidate'/f)==h
old=json.loads((B/'ordinary-failure-profile-baseline-validation/receipt.json').read_text());assert sha(O/'baseline/hawdb/_hawdb.so')==old['artifacts']['python_extension']['sha256']
# Check entire Rust engine and Python target input paths; the benchmark BUILD change
# is retained for review, while both included Rust observer source blobs must match.
paths=['src','crates','bindings/python','Cargo.toml','Cargo.lock','BUILD.bazel','MODULE.bazel','MODULE.bazel.lock','rust-toolchain.toml','.bazelrc','.cargo','build.rs','tools/bazel','bindings/benchmarks/native_alloc.rs','bindings/benchmarks/native_profile.rs']
diff=subprocess.check_output(['git','diff','--name-only',old['source']['tree'],manifest['current_baseline_observer_tree'],'--',*paths],cwd=base,text=True);assert not diff,diff
for name in ['python_boundary.py','checksum.py','run.py']:
 raw=subprocess.check_output(['git','show',manifest['current_baseline_observer_tree']+':bindings/benchmarks/'+name],cwd=base)
 current=(W/'bindings/benchmarks'/name).read_bytes();assert raw==current,name
interpreter=(B/'resource-qualification/venv/bin/python').resolve();assert sha(interpreter)==held['interpreter']['sha256']
receipt=json.loads((O/'receipt.json').read_text());assert len(receipt['probes'])==3
for item in receipt['probes']:
 r=item['result']
 if item['label']=='probe-success':assert r['status']=='ok' and 'failure_python_traced_heap' not in r
 elif item['label']=='probe-warmup_refusal':assert r['phase']=='warmup' and r['failure_python_traced_heap']['available'] is False
 else:assert r['phase']=='execute' and r['failure_python_traced_heap']['python_traced_peak_bytes']>0
result={'verified':True,'engine':manifest['production_head'],'source_manifest_sha256':sha(O/'source-manifest.json'),'observer_patch_sha256':sha(O/'observer.patch'),'candidate_package_matches_prior_final_source_build':True,'baseline_binary_matches_original_build_receipt':True,'baseline_native_input_paths_unchanged':paths,'original_python_wrapper_checksum_and_fixture_generator_equal_current_baseline':True,'interpreter':{'path':str(interpreter),'sha256':sha(interpreter)},'real_probe_count':3,'scope':'Source/observer wiring qualification only. Supplemental sampling may still run; no completed performance or original matrix acceptance implied.'}
(O/'source-qualified-audit.json').write_text(json.dumps(result,indent=2)+'\n');print('source, native packages, interpreter and three real probes verified')
