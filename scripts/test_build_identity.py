import json
from pathlib import Path
import unittest
from unittest.mock import patch

from verify_build_identity import inspect


class BuildIdentityTest(unittest.TestCase):
    def test_bundle_guard_rejects_stale_component_even_at_same_commit(self):
        paths = {role: Path(role) for role in ['gui', 'cli', 'core', 'worker']}
        def output(args, **_):
            component = Path(args[0]).name
            return json.dumps({'component': component, 'git_sha': 'a'*40,
                               'version': '0.1.0', 'protocol_version': 18,
                               'dirty': False, 'built_at_epoch': 1 if component=='worker' else 2})
        with patch('verify_build_identity.subprocess.check_output', side_effect=output):
            with self.assertRaisesRegex(ValueError, 'mixed component builds'):
                inspect(paths)

    def test_exact_identity_accepts_roles_but_not_unknown_source(self):
        paths = {role: Path(role) for role in ['gui', 'cli', 'core', 'worker']}
        for sha in ['b'*40, 'unknown']:
            def output(args, **_):
                return json.dumps({'component': Path(args[0]).name, 'git_sha': sha,
                                   'version': '0.1.0', 'protocol_version': 18,
                                   'dirty': True, 'built_at_epoch': 2})
            with patch('verify_build_identity.subprocess.check_output', side_effect=output):
                if sha=='unknown':
                    with self.assertRaises(ValueError):
                        inspect(paths)
                else:
                    self.assertEqual(len(inspect(paths)['components']),4)
