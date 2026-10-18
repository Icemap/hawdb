"""Bind phase-only qualification to an explicit engine; never run a benchmark."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess

OVERLAYS = sorted({
    '.github/workflows/native-bindings.yml', 'BUILD.bazel', 'Cargo.toml',
    'bindings/python/BUILD.bazel', 'bindings/python/Cargo.toml',
    'bindings/python/src/database.rs', 'bindings/python/src/lib.rs',
    'qualification/resources/baseline-source.json', 'qualification/resources/campaign.json',
    'qualification/resources/numeric.py', 'qualification/resources/numeric_driver.py',
    'qualification/resources/run.py', 'qualification/retained/configured.py',
    'qualification/retained/run.py', 'qualification/phases/consume.py',
    'src/api/retained_query.rs', 'src/embedded.rs', 'src/lib.rs', 'src/retained_phase_profile.rs',
})


def git(root, *args):
    return subprocess.check_output(['git', *args], cwd=root, text=True).strip()


def plan(root, engine, tree):
    assert re.fullmatch('[0-9a-f]{40}', engine), 'explicit immutable engine required'
    assert re.fullmatch('[0-9a-f]{40}', tree), 'explicit immutable engine tree required'
    assert git(root, 'rev-parse', engine + '^{tree}') == tree, 'engine tree mismatch'
    assert git(root, 'merge-base', engine, 'HEAD') == engine, 'engine is not an ancestor'
    tracked = git(root, 'diff', '--name-only', engine).splitlines()
    added = git(root, 'ls-files', '--others', '--exclude-standard').splitlines()
    actual = sorted(set(tracked + added))
    assert actual == OVERLAYS, {'unexpected': sorted(set(actual) - set(OVERLAYS)),
                               'missing': sorted(set(OVERLAYS) - set(actual))}
    consumer = root / 'bindings/benchmarks/retained_boundary.py'
    assert hashlib.sha256(consumer.read_bytes()).hexdigest() == 'a9275fa36bd57feddc463a53b60f386d37bebba4337b4fee87040ba35ae3a9d1'
    workflow_path = root / '.github/workflows/native-bindings.yml'
    workflow = workflow_path.read_text()

    def replace(before, after):
        nonlocal workflow
        assert workflow.count(before) == 1, 'workflow anchor mismatch: ' + before
        workflow = workflow.replace(before, after)

    replace('options: [numeric, ordinary, resources, retained]',
            'options: [numeric, ordinary, resources, retained, phases]')
    replace("          if selection['kind']=='resources':\n", """          if selection['kind']=='phases':
              for configuration,sizes in [('default',[1000,100000,1000000]),('large',[1000000,1000001])]:
                  for rows,backend in itertools.product(sizes,['memory','file']):
                      item=numeric(configuration,rows,backend,False)
                      item['phase_only']=True
                      groups.append(item)
              assert len(groups)==10
          elif selection['kind']=='resources':
""")
    replace('      QUAL_INSTRUMENTED: ${{ matrix.instrumented }}\n',
            '      QUAL_INSTRUMENTED: ${{ matrix.instrumented }}\n'
            '      QUAL_PHASE_ONLY: ${{ matrix.phase_only || false }}\n')
    replace('          if [[ "$QUAL_INSTRUMENTED" == true ]]; then flags+=(--instrumented); fi\n',
            '          if [[ "$QUAL_INSTRUMENTED" == true ]]; then flags+=(--instrumented); fi\n'
            '          if [[ "$QUAL_PHASE_ONLY" == true ]]; then flags+=(--phase-only); fi\n')
    campaign_path = root / 'qualification/resources/campaign.json'
    campaign = json.loads(campaign_path.read_text())
    campaign.update(engine=engine, engine_tree=tree, overlay_files=OVERLAYS,
                    phase_only_groups=10,
                    scope='Original90 ordinary/110 resource/10 retained groups remain unchanged. '
                          'An additional10-group phase-only campaign uses the unchanged complete '
                          'consumer,31 samples, default and explicit-large limits. Source binding '
                          'permits only the19 declared qualification overlays; any other production '
                          'change must belong to the explicit engine commit before qualification.')
    return {workflow_path: workflow,
            campaign_path: json.dumps(campaign, indent=2) + '\n'}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--worktree', type=Path, required=True)
    p.add_argument('--engine', required=True)
    p.add_argument('--engine-tree', required=True)
    p.add_argument('--receipt', type=Path, required=True)
    p.add_argument('--apply', action='store_true')
    a = p.parse_args()
    root = a.worktree.resolve()
    assert not a.receipt.resolve().is_relative_to(root), 'receipt must stay outside frozen source'
    edits = plan(root, a.engine, a.engine_tree)
    receipt = {'status': 'prepared_not_executed' if a.apply else 'dry_run_only',
               'engine': a.engine, 'engine_tree': a.engine_tree,
               'head_before_commit': git(root, 'rev-parse', 'HEAD'),
               'overlay_files': OVERLAYS,
               'updates': {str(path.relative_to(root)): {
                   'before': hashlib.sha256(path.read_bytes()).hexdigest(),
                   'after': hashlib.sha256(text.encode()).hexdigest()}
                   for path, text in edits.items()},
               'scope': 'Metadata/workflow preparation only; no build, runtime or acceptance claim.'}
    if a.apply:
        for path, text in edits.items():
            path.write_text(text)
    a.receipt.write_text(json.dumps(receipt, indent=2) + '\n')


if __name__ == '__main__':
    main()
