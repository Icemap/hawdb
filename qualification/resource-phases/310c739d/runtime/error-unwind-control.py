import importlib.util,json,pathlib,tempfile
import hawdb,hawdb._hawdb as native
from hawdb.exceptions import ParseError
root=pathlib.Path.cwd()
def load(name,path):
 spec=importlib.util.spec_from_file_location(name,path);m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m);return m
observer=load("phase_observer",root/"consume.py");consumer=load("original_consumer",root/"retained_boundary.py");records=[]
for backend in ["memory","file"]:
 with tempfile.TemporaryDirectory(prefix="hawdb-phase-error-control-") as directory:
  db=hawdb.open(None if backend=="memory" else pathlib.Path(directory)/"db")
  try:
   db.execute("CREATE NODE TABLE Delivery")
   db.execute("CREATE PROPERTY ON NODE TABLE Delivery(score) TYPE INT")
   db.execute("CREATE (:Delivery {score: 0})")
   before=observer.snapshot(native)
   try:db.execute("MATCH (")
   except ParseError as error:rejected={"type":type(error).__name__,"message":str(error)}
   else:raise AssertionError("Expected malformed-Cypher parse refusal")
   after=observer.snapshot(native);delta={k:after[k]-before[k] for k in observer.METRICS}
   assert delta["owned_calls"]==1 and delta["owned_ns"]>0 and all(delta[k]==0 for k in ["prepare_calls","prepare_ns","pull_calls","pull_ns"]),delta
   retry=observer.attempt(native,consumer,db,"owned",consumer.fixture_reference(1))
   assert retry["status"]=="ok" and retry["parity"] and retry["rows"]==1 and retry["phase_control_valid"] and retry["phase_attribution_valid"]
   assert retry["native_phases"]["owned_calls"]==1 and retry["native_work_ns"]>0 and retry["handoff_boundary_residual_ns"]>=0
   records.append(dict(backend=backend,rejected=rejected,error_phase_delta=delta,healthy_retry=retry))
  finally:db.close()
print(json.dumps(dict(records=records,scope="Real parse rejection followed by unchanged complete owned consumer on both backends. Native counter/error unwind control only; no latency ratio/full exception matrix.")))
