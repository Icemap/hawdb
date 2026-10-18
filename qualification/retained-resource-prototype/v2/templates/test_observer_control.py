"""Observer failures may invalidate measurements, never alter consumer outcomes."""
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('observer', Path(__file__).with_name('consume.py'))
observer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(observer)

def snap(**changes):
    return dict(protocol='hawdb-retained-phase-v2', allocation_instrumented=False,
                overflowed=False, prepare_calls=0, prepare_ns=0, pull_calls=0,
                pull_ns=0, owned_calls=0, owned_ns=0, **changes)

class Native:
    def __init__(self, *values): self.values = iter(values)
    def _retained_phase_snapshot(self):
        value = next(self.values)
        if isinstance(value, Exception): raise value
        return value

class Consumer:
    def __init__(self, result): self.result, self.calls = result, 0
    def attempt(self, *args):
        self.calls += 1
        if isinstance(self.result, Exception): raise self.result
        return dict(self.result)

class ObserverControl(unittest.TestCase):
    def test_before_failure_does_not_skip_successful_consumer(self):
        c = Consumer(dict(status='ok', checksum='oracle', query_boundary_ns=10))
        result = observer.attempt(Native(RuntimeError('observer unavailable')), c, None, 'retained', {})
        self.assertEqual(c.calls, 1)
        self.assertEqual((result['status'], result['checksum']), ('ok', 'oracle'))
        self.assertFalse(result['phase_attribution_valid'])
        self.assertEqual(result['phase_validation_error']['stage'], 'before_snapshot')

    def test_after_failure_preserves_query_failure(self):
        c = Consumer(dict(status='error', error='budget refusal', retained_snapshot={'owners': 0}))
        result = observer.attempt(Native(snap(), RuntimeError('observer overflow')), c, None, 'arrow', {})
        self.assertEqual((result['status'], result['error']), ('error', 'budget refusal'))
        self.assertEqual(result['retained_snapshot'], {'owners': 0})
        self.assertIsNone(result['handoff_boundary_residual_ns'])

    def test_original_consumer_exception_is_not_caught_as_observer_error(self):
        c = Consumer(ValueError('consumer parity mismatch'))
        with self.assertRaisesRegex(ValueError, 'consumer parity mismatch'):
            observer.attempt(Native(RuntimeError('observer unavailable')), c, None, 'arrow', {})
        self.assertEqual(c.calls, 1)

    def test_negative_residual_invalidates_measurement_without_rewriting_success(self):
        after = snap();after.update(prepare_calls=1, prepare_ns=5, pull_calls=1, pull_ns=6)
        c = Consumer(dict(status='ok', query_boundary_ns=10, checksum='oracle'))
        result = observer.attempt(Native(snap(), after), c, None, 'retained', {})
        self.assertEqual(result['status'], 'ok')
        self.assertFalse(result['phase_attribution_valid'])
        self.assertIsNone(result['handoff_boundary_residual_ns'])
        self.assertIn('phase_validation_error', result)

    def test_valid_native_coverage_produces_residual(self):
        after = snap();after.update(prepare_calls=1, prepare_ns=5, pull_calls=2, pull_ns=6)
        c = Consumer(dict(status='ok', query_boundary_ns=20, checksum='oracle'))
        result = observer.attempt(Native(snap(), after), c, None, 'arrow', {})
        self.assertTrue(result['phase_attribution_valid'])
        self.assertEqual(result['handoff_boundary_residual_ns'], 9)
        self.assertEqual(result['checksum'], 'oracle')

    def test_owned_requires_real_native_coverage_and_preserves_success(self):
        c = Consumer(dict(status='ok', query_boundary_ns=20, checksum='oracle'))
        result = observer.attempt(Native(snap(), snap()), c, None, 'owned', {})
        self.assertEqual(result['status'], 'ok')
        self.assertEqual(c.calls, 1)
        self.assertFalse(result['phase_attribution_valid'])
        self.assertIsNone(result['handoff_boundary_residual_ns'])

    def test_owned_native_coverage_excludes_unused_prepare_pull(self):
        after = snap(); after.update(owned_calls=1, owned_ns=7)
        c = Consumer(dict(status='ok', query_boundary_ns=20, checksum='oracle'))
        result = observer.attempt(Native(snap(), after), c, None, 'owned', {})
        self.assertTrue(result['phase_attribution_valid'])
        self.assertEqual(result['native_work_ns'], 7)
        self.assertEqual(result['handoff_boundary_residual_ns'], 13)
        self.assertEqual(result['checksum'], 'oracle')

if __name__ == '__main__': unittest.main()
