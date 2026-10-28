import unittest
from restore_debug_session import readiness_errors


class RestoreReadinessTests(unittest.TestCase):
    def setUp(self):
        self.build = {'protocol_version': 21}
        self.before = {'enabled_intent': True, 'paused_intent': True, 'microphone_enabled': True,
                       'system_enabled': True, 'recording_schedule': {'enabled': True, 'start_minute': 600,
                       'stop_minute': 1380, 'next_at_ms': 10000, 'paused_by_schedule': False, 'error': None}}
        self.state = self.before | {'capture_blocker': 'none', 'microphone_recording': False, 'system_recording': False}
        self.owner = {'build': self.build, 'allow_capture': True, 'normal_run': True}

    def check(self, state=None, owner=None, gui=True, now=1000):
        return readiness_errors(self.before, state or self.state, owner or self.owner, gui, self.build, now)

    def test_paused_ready_app_succeeds_but_query_only_core_fails(self):
        self.assertEqual(self.check(), [])
        errors = self.check(self.state | {'capture_blocker': 'query_only'}, self.owner | {'allow_capture': False})
        self.assertIn('capture_blocker=query_only', errors)
        self.assertTrue(self.check(gui=False))

    def test_pause_schedule_and_model_failures_are_visible(self):
        self.assertTrue(self.check(self.state | {'paused_intent': False}))
        self.assertTrue(self.check(self.state | {'capture_blocker': 'model_not_ready'}))
        changed = self.state | {'recording_schedule': self.before['recording_schedule'] | {'start_minute': 601}}
        self.assertIn('recording schedule changed: start_minute', self.check(changed))
        self.assertTrue(self.check(self.state | {'microphone_recording': True}))

    def test_normal_schedule_boundary_may_change_pause_but_not_config(self):
        self.assertEqual(self.check(self.state | {'paused_intent': False}, now=10001), [])
        changed = self.state | {'recording_schedule': self.before['recording_schedule'] | {'enabled': False}}
        self.assertTrue(self.check(changed, now=10001))
