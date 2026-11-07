"""Run the real local CLI without Core, capture or providers on either OS."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent
CLI = ROOT / ('target/debug/loppi.exe' if os.name == 'nt' else 'target/debug/loppi')


@unittest.skipUnless(CLI.exists(), 'build loppi CLI before running integration test')
class AgentCli(unittest.TestCase):
    def test_relative_data_directory_is_advertised_as_absolute(self):
        with tempfile.TemporaryDirectory(prefix='loppi-agent-relative-') as temp:
            relative = 'relative data/用户资料'
            env = os.environ | {'LOPPI_DATA_DIR': relative}
            info = json.loads(subprocess.check_output([str(CLI), 'integrations', 'inspect'], cwd=temp, env=env, text=True, encoding='utf-8'))
            source = Path(temp).resolve() / relative / 'skills' / 'loppi'
            self.assertTrue(Path(info['skill_directory']).is_absolute())
            self.assertEqual(Path(info['skill_directory']), source)
            self.assertIn(str(source), info['skill_prompt'])
            self.assertTrue((source / 'SKILL.md').is_file())
            text = subprocess.check_output([str(CLI), 'readme'], cwd=temp, env=env, text=True, encoding='utf-8')
            self.assertIn(str(source), text)
            self.assertFalse((Path(temp) / relative / 'loppi.sqlite').exists())

    def test_readme_source_workspace_copy_and_legacy_installer(self):
        with tempfile.TemporaryDirectory(prefix='loppi-agent-docs-') as temp:
            data = Path(temp) / '用户资料 with spaces'
            env = os.environ | {'LOPPI_DATA_DIR': str(data)}
            def run(*args, code=0):
                result = subprocess.run([str(CLI), *args], env=env, capture_output=True, text=True, encoding='utf-8', timeout=15)
                self.assertEqual(result.returncode, code, result.stderr)
                return result.stdout
            text = run('readme')
            source = data / 'skills/loppi'
            skill = (source / 'SKILL.md').read_text(encoding='utf-8')
            self.assertIn(str(source), text)
            self.assertIn(str(CLI.resolve()), text)
            info = json.loads(run('integrations', 'inspect'))
            self.assertEqual(info['skill_directory'], str(source))
            self.assertIn(info['skill_version'], text)
            self.assertIn('version: "' + info['skill_version'] + '"', skill)
            self.assertIn('主动从', skill)
            self.assertIn('name: loppi\n', skill)
            self.assertNotIn('{{', skill)
            self.assertIn(str(source), info['skill_prompt'])
            self.assertIn('当前工作区', info['skill_prompt'])
            self.assertNotIn('--destination', info['skill_prompt'])
            modified = (source / 'SKILL.md').stat().st_mtime_ns
            run('readme')
            self.assertEqual((source / 'SKILL.md').stat().st_mtime_ns, modified)
            (source / 'SKILL.md').write_text('old official skill', encoding='utf-8')
            run('readme')
            self.assertEqual((source / 'SKILL.md').read_text(encoding='utf-8'), skill)
            target = Path(temp) / 'workspace/skills/loppi'
            run('integrations', 'install-skill', '--destination', str(target))
            run('integrations', 'install-skill', '--destination', str(target))
            self.assertEqual((target / 'SKILL.md').read_text(encoding='utf-8'), skill)
            (target / 'SKILL.md').write_text('custom', encoding='utf-8')
            run('integrations', 'install-skill', '--destination', str(target), code=2)
            self.assertEqual((target / 'SKILL.md').read_text(encoding='utf-8'), 'custom')
            for args in [('--help',), ('context', 'read', '--help'), ('event', 'edit', '--help'), ('cleanup', '--help'), ('cloud', 'configure', '--help')]:
                self.assertIn('Usage:', run(*args))
            self.assertFalse((data / 'core.sock').exists())
            self.assertFalse((data / 'core.lock').exists())
            self.assertFalse((data / 'loppi.sqlite').exists())

    def test_readme_reports_invalid_source_without_success(self):
        with tempfile.TemporaryDirectory(prefix='loppi-agent-docs-error-') as temp:
            data = Path(temp)
            (data / 'skills').write_text('not a directory', encoding='utf-8')
            result = subprocess.run([str(CLI), 'readme'], env=os.environ | {'LOPPI_DATA_DIR': str(data)}, capture_output=True, text=True, encoding='utf-8', timeout=15)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(result.stdout, '')
            self.assertIn('loppi:', result.stderr)
            self.assertEqual((data / 'skills').read_text(encoding='utf-8'), 'not a directory')


if __name__ == '__main__':
    unittest.main()
