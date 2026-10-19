from pathlib import Path
import hashlib,json,shutil,subprocess,difflib
B=Path('/private/tmp/hawdb-wip-ready-20261015'); O=B/'supplemental-python-failure-heap-0c298984';O.mkdir(exist_ok=True)
W=B/'qualify-resources-final-0c298984'; baseline=B/'resource-qualification-v2/baseline'
original=(W/'bindings/benchmarks/python_boundary.py').read_text()
old='''        return {"status": "error", "layer": "python", "phase": phase,
                "error_type": type(error).__name__, "error": str(error)}'''
new='''        failure_heap = {
            "available": False,
            "reason": "query tracing window has not started",
            "scope": "query_tracing_window_before_error_cleanup",
        }
        if tracing_started:
            host_current, host_peak = tracemalloc.get_traced_memory()
            failure_heap = {
                "available": True,
                "python_traced_current_bytes": host_current,
                "python_traced_peak_bytes": host_peak,
                "scope": "query_tracing_window_before_error_cleanup",
            }
        return {"status": "error", "layer": "python", "phase": phase,
                "error_type": type(error).__name__, "error": str(error),
                "failure_python_traced_heap": failure_heap}'''
assert original.count(old)==1
modified=original.replace(old,new)
(O/'python_boundary.py').write_text(modified)
(O/'observer.patch').write_text(''.join(difflib.unified_diff(original.splitlines(True),modified.splitlines(True),fromfile='original/python_boundary.py',tofile='supplement/python_boundary.py')))
shutil.copy2(W/'bindings/benchmarks/checksum.py',O/'checksum.py')
r=json.loads((B/'ordinary-failure-profile-baseline-validation/receipt.json').read_text())
native=Path(r['artifacts']['python_extension']['path'])
assert hashlib.sha256(native.read_bytes()).hexdigest()==r['artifacts']['python_extension']['sha256']
for side,package,extension in [('baseline',baseline/'bindings/python/python/hawdb',native),('candidate',B/'final-held-write-instrumented-0c298984-frozen/hawdb',B/'final-held-write-instrumented-0c298984-frozen/hawdb/_hawdb.so')]:
 target=O/side/'hawdb';shutil.copytree(package,target,ignore=shutil.ignore_patterns('__pycache__','*.pyc'),dirs_exist_ok=True);shutil.copy2(extension,target/'_hawdb.so')
changed=subprocess.check_output(['git','diff','--name-only','8c30cf48bc6ced6fef624c8333759892ff6b21a4','4a235b06362ae6306a300e45d5974495c1229c2b'],cwd=baseline,text=True).splitlines()
assert all(p.startswith(('bindings/benchmarks/','bindings/go/')) or p=='bindings/ffi/BUILD.bazel' for p in changed)
manifest={'production_head':'0c2989842321d10e91f136f3c924badb07038add','baseline_head':r['source']['head'],'baseline_build_tree':r['source']['tree'],'current_baseline_observer_tree':'4a235b06362ae6306a300e45d5974495c1229c2b','baseline_tree_differences':changed,'native_dependency_evidence':'Python profiling target depends on //:hawdb and pyo3; Rust/Python native sources and root build configuration unchanged between observer trees. FFI/Go build changes are outside that target. Python observer is a separately hashed overlay.','scope':'Supplemental local host heap capture; does not replace or increase original matrix counts. Failure host tracing excludes setup/warmup and precedes cleanup; failure native counters include the entire producer. No cross-scope ratios.','files':{str(p.relative_to(O)):hashlib.sha256(p.read_bytes()).hexdigest() for p in O.rglob('*') if p.is_file()}}
(O/'source-manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
print(O, len(manifest['files']),'frozen files')
