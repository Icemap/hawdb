# Go boundary profiling

Developer builds with the explicit `hawdb_boundary_profile` Go build tag expose
`DB.BoundaryProfile()` for cumulative host parameter/JSON, native-call and result
copy/free/JSON/normalization timers. Ordinary builds compile out these observers
and do not expose the profiling API. See the manual [boundary qualification
instructions](../benchmarks/README.md#go-host-json-conversion-observations) for
scope, saturation, failure and snapshot limitations. This tag is a local
qualification facility, not a production configuration plane.
