from pathlib import Path
import json,hashlib,shutil,subprocess,difflib
B=Path('/private/tmp/hawdb-wip-ready-20261015');O=B/'whole-run-warmup-heap-0c298984';O.mkdir(exist_ok=True);W=B/'qualify-resources-final-0c298984';scope='run_including_store_open_setup_warmup_query_and_cleanup_excluding_job_decode_and_library_load'
prior=B/'supplemental-go-failure-heap-0c298984';shutil.copytree(prior/'module',O/'go-module',dirs_exist_ok=True);shutil.copy2(prior/'baseline-ffi.dylib',O/'baseline-ffi.dylib')
p=O/'go-module/cmd/boundary/main.go';original=p.read_text();needle='\t\tresult, err = run(j, lib, nativeRead)\n';replacement='''		var wholeBefore, wholeAfter runtime.MemStats
		runtime.ReadMemStats(&wholeBefore)
		result, err = run(j, lib, nativeRead)
		runtime.ReadMemStats(&wholeAfter)
		if result == nil {
			result = make(map[string]any)
		}
		valid := wholeAfter.Mallocs >= wholeBefore.Mallocs && wholeAfter.TotalAlloc >= wholeBefore.TotalAlloc
		whole := map[string]any{"available": valid,
			"scope": "SCOPE"}
		if valid {
			whole["go_allocations"] = wholeAfter.Mallocs - wholeBefore.Mallocs
			whole["go_allocated_bytes"] = wholeAfter.TotalAlloc - wholeBefore.TotalAlloc
		}
		result["whole_run_go_heap"] = whole
'''.replace('SCOPE',scope);assert original.count(needle)==1;p.write_text(original.replace(needle,replacement));subprocess.run(['gofmt','-w',str(p)],check=True);(O/'whole-go.patch').write_text(''.join(difflib.unified_diff(original.splitlines(True),p.read_text().splitlines(True),fromfile='prior-query-error-observer/main.go',tofile='whole-run/main.go')))
priorpy=B/'supplemental-python-failure-heap-0c298984';shutil.copytree(priorpy/'baseline',O/'python-baseline',ignore=shutil.ignore_patterns('__pycache__','*.pyc'),dirs_exist_ok=True);shutil.copy2(W/'bindings/benchmarks/checksum.py',O/'checksum.py')
original=(W/'bindings/benchmarks/python_boundary.py').read_text();modified=original
needle='''            import tracemalloc
            tracemalloc.start()
            tracing_started = True
''';assert modified.count(needle)==1;modified=modified.replace(needle,'')
needle='''            host_current, host_peak = tracemalloc.get_traced_memory()
            tracemalloc.stop()
            tracing_started = False
''';assert modified.count(needle)==1;modified=modified.replace(needle,'')
for line in ['            profile["python_traced_current_bytes"] = host_current\n','            profile["python_traced_peak_bytes"] = host_peak\n']:assert line in modified;modified=modified.replace(line,'')
needle='    failure_before = (native._boundary_profile_snapshot()\n';assert modified.count(needle)==1;modified=modified.replace(needle,'    import tracemalloc\n    tracemalloc.start()\n'+needle)
needle='    result = run(job)\n';replacement='''    result = run(job)
    whole_current, whole_peak = tracemalloc.get_traced_memory()
    tracemalloc.stop()
    result["whole_run_python_heap"] = {
        "available": True,
        "python_traced_current_bytes": whole_current,
        "python_traced_peak_bytes": whole_peak,
        "scope": "SCOPE",
    }
'''.replace('SCOPE',scope);assert modified.count(needle)==1;modified=modified.replace(needle,replacement);(O/'python_boundary.py').write_text(modified);(O/'whole-python.patch').write_text(''.join(difflib.unified_diff(original.splitlines(True),modified.splitlines(True),fromfile='original/python_boundary.py',tofile='whole-run/python_boundary.py')))
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest();g=json.loads((prior/'build-source-manifest.json').read_text());py=json.loads((priorpy/'source-manifest.json').read_text());assert sha(O/'baseline-ffi.dylib')==g['native_library_sha256'];assert sha(O/'python-baseline/hawdb/_hawdb.so')==py['files']['baseline/hawdb/_hawdb.so']
manifest={'baseline_head':'3cdb2610ba8b984676b80ba22df64821787886d3','go_source_manifest_sha256':sha(prior/'build-source-manifest.json'),'python_source_manifest_sha256':sha(priorpy/'source-manifest.json'),'files':{str(p.relative_to(O)):sha(p) for p in O.rglob('*') if p.is_file()},'scope':scope,'limits':'Unchanged default constructors, budgets,512-row read-fixture mutations, queries and checksum consumer. Tracing changes observation overhead; no ordinary-latency or cross-scope ratio claim. Whole-run Go totals count allocated traffic; tracemalloc reports traced Python current/peak, not native memory.'};(O/'source-manifest.json').write_text(json.dumps(manifest,indent=2)+'\n');print(O,len(manifest['files']))
