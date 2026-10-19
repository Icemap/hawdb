from pathlib import Path
import subprocess,json,hashlib,difflib,shutil
B=Path('/private/tmp/hawdb-wip-ready-20261015');O=B/'supplemental-go-failure-heap-0c298984';O.mkdir(exist_ok=True);W=B/'resource-qualification-v2/baseline';tree='4a235b06362ae6306a300e45d5974495c1229c2b'
files=subprocess.check_output(['git','ls-tree','-r','--name-only',tree,'--','bindings/go'],cwd=W,text=True).splitlines()
for f in files:
 dest=O/'module'/Path(f).relative_to('bindings/go');dest.parent.mkdir(parents=True,exist_ok=True);dest.write_bytes(subprocess.check_output(['git','show',tree+':'+f],cwd=W))
p=O/'module/cmd/boundary/main.go';original=p.read_text()
needle='\tvar hostRead func() hostSnapshot\n'
insert='''	var failureHeapBefore runtime.MemStats
	failureHeapStarted := false
	if hostProfiling {
		defer func() {
			if runErr == nil {
				return
			}
			if report == nil {
				report = make(map[string]any)
			}
			heap := map[string]any{
				"available": false,
				"reason": "query allocation window has not started",
				"scope": "query_allocation_window_after_error_cleanup",
			}
			if failureHeapStarted {
				var failureHeapAfter runtime.MemStats
				runtime.ReadMemStats(&failureHeapAfter)
				if failureHeapAfter.Mallocs >= failureHeapBefore.Mallocs && failureHeapAfter.TotalAlloc >= failureHeapBefore.TotalAlloc {
					heap = map[string]any{
						"available": true,
						"go_allocations": failureHeapAfter.Mallocs - failureHeapBefore.Mallocs,
						"go_allocated_bytes": failureHeapAfter.TotalAlloc - failureHeapBefore.TotalAlloc,
						"scope": "query_allocation_window_after_error_cleanup",
					}
				} else {
					heap["reason"] = "allocation counters regressed"
				}
			}
			report["failure_go_heap_profile"] = heap
		}()
	}
'''
# Registered before existing close and failure-host defers; runs after those
# defers. The scope explicitly includes error cleanup and snapshot overhead.
assert original.count(needle)==1
modified=original.replace(needle,insert+needle)
needle2='\truntime.ReadMemStats(&before)\n';assert modified.count(needle2)==1
modified=modified.replace(needle2,needle2+'\tfailureHeapBefore = before\n\tfailureHeapStarted = true\n')
p.write_text(modified)
subprocess.run(['gofmt','-w',str(p)],check=True)
(O/'observer.patch').write_text(''.join(difflib.unified_diff(original.splitlines(True),p.read_text().splitlines(True),fromfile='original/main.go',tofile='supplement/main.go')))
old=json.loads((B/'ordinary-failure-profile-baseline-validation/receipt.json').read_text());lib=Path(old['artifacts']['library']['path']);sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest();assert sha(lib)==old['artifacts']['library']['sha256'];shutil.copy2(lib,O/'baseline-ffi.dylib')
# The FFI BUILD difference only affects the non-profile target; assert the
# profiling target and every Rust source remains exactly equal.
paths=['src','crates','bindings/ffi/src','Cargo.toml','Cargo.lock','BUILD.bazel','MODULE.bazel','MODULE.bazel.lock','rust-toolchain.toml','.bazelrc','.cargo','build.rs','tools/bazel','bindings/benchmarks/native_alloc.rs','bindings/benchmarks/native_profile.rs']
assert not subprocess.check_output(['git','diff','--name-only',old['source']['tree'],tree,'--',*paths],cwd=W,text=True)
(O/'build-source-manifest.json').write_text(json.dumps({'baseline_head':old['source']['head'],'native_build_tree':old['source']['tree'],'go_source_tree':tree,'go_version':subprocess.check_output(['go','version'],text=True).strip(),'native_library_sha256':sha(O/'baseline-ffi.dylib'),'files':{str(p.relative_to(O)):sha(p) for p in (O/'module').rglob('*') if p.is_file()},'scope':'Supplemental Go profiling observer; error heap delta includes query execution, error return, deferred db.Close and failure-host snapshot, unlike original successful query-only heap fields. No cross-scope ratio.'},indent=2)+'\n')
print('prepared',O)
