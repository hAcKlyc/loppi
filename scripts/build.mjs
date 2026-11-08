#!/usr/bin/env node
// Build-time downloads only. The installed application never downloads models.
import { existsSync, mkdirSync, readFileSync, writeFileSync, cpSync, lstatSync, readdirSync, rmSync } from 'node:fs';
import { homedir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync, execFileSync } from 'node:child_process';
import { parseEnv } from 'node:util';
import { createMacUpdateArchive, signingEnv, writePlatformManifest } from './updater-artifacts.mjs';
import { checked, modelLockBytes, models, prepareMacNativeArchive, prepareModelCache, sha } from './build-assets.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const settings = JSON.parse(readFileSync(join(root, 'config/build-targets.json')));
const output = (cmd, args) => execFileSync(cmd, args, { encoding: 'utf8' }).trim();
function run(cmd, args, env = process.env) {
  const result = spawnSync(cmd, args, { env, stdio: 'inherit' });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${cmd} failed (${result.status})`);
}
function architectures(path, expected) {
  const actual = output('lipo', ['-archs', path]).split(/\s+/).sort();
  if (actual.join() !== [...expected].sort().join()) throw new Error(`Wrong architectures in ${path}: ${actual}`);
}
function minimumOS(path) {
  const loads = output('otool', ['-l', path]);
  const versions = [...loads.matchAll(/cmd LC_BUILD_VERSION\s+cmdsize \d+\s+platform \S+\s+minos (\d+)\.(\d+)/g), ...loads.matchAll(/cmd LC_VERSION_MIN_MACOSX\s+cmdsize \d+\s+version (\d+)\.(\d+)/g)];
  const [major, minor] = settings.minimum_system_version.split('.').map(Number);
  if (!versions.length || versions.some(m => Number(m[1]) > major || (Number(m[1]) === major && Number(m[2]) > minor))) throw new Error(`Unsupported minimum macOS version: ${path}`);
}
function versionCheck() {
  const version = JSON.parse(readFileSync('package.json')).version;
  const npmLock = JSON.parse(readFileSync('package-lock.json'));
  const tauri = JSON.parse(readFileSync('src-tauri/tauri.conf.json'));
  const cargo = readFileSync('Cargo.toml', 'utf8').match(/\[workspace.package\]\s+version = "([^"]+)"/)[1];
  const plist = output('/usr/libexec/PlistBuddy', ['-c', 'Print :CFBundleShortVersionString', 'src-tauri/native/LoppiCore-Info.plist']);
  if ([npmLock.version, npmLock.packages[''].version, tauri.version, cargo, plist].some(v => v !== version)) throw new Error('Project versions disagree');
  if (tauri.bundle.macOS.minimumSystemVersion !== settings.minimum_system_version) throw new Error('Minimum macOS versions disagree');
  const cargoLock = readFileSync('Cargo.lock', 'utf8');
  for (const crate of ['sherpa-onnx', 'sherpa-onnx-sys']) {
    if (!cargoLock.includes(`name = "${crate}"\nversion = "${settings.native.version}"`)) throw new Error('Native engine/Cargo versions disagree');
  }
  return version;
}

export function prepareNativeLibraries(extracted) {
  const native = settings.native;
  const archive = prepareMacNativeArchive(native);
  // Re-extract the verified archive; extracted files are never trusted as cache.
  mkdirSync(extracted, { recursive: true });
  run('tar', ['-xjf', archive, '-C', extracted]);
  const lib = join(extracted, native.directory, 'lib');
  for (const file of readdirSync(lib).filter(f => f.endsWith('.a'))) architectures(join(lib, file), ['arm64', 'x86_64']);
  return lib;
}

export function buildPlan(argv, config = settings) {
  const [command = 'release', ...args] = argv;
  if (!['build', 'release'].includes(command) || (args.length && (args.length !== 2 || args[0] !== '--target'))) {
    throw new Error('Usage: node scripts/build.mjs [build|release] [--target aarch64-apple-darwin|x86_64-apple-darwin|universal-apple-darwin]');
  }
  const targets = args.length ? [args[1]] : config.default_targets;
  if (!Array.isArray(targets) || !targets.length || new Set(targets).size !== targets.length) throw new Error('Invalid default build targets');
  for (const target of targets) {
    if (!Object.hasOwn(config.targets, target)) throw new Error(`Unsupported target: ${target}`);
  }
  return { command, targets };
}

// Node itself may be translated. Query the machine rather than process.arch.
export function hostArchitecture() {
  const arm = spawnSync('/usr/sbin/sysctl', ['-n', 'hw.optional.arm64'], { encoding: 'utf8' });
  return arm.status === 0 && arm.stdout.trim() === '1' ? 'arm64' : 'x86_64';
}

function main() {
  process.chdir(root);
  const { command, targets } = buildPlan(process.argv.slice(2));
  if (process.platform !== 'darwin') throw new Error('This build entry supports macOS only');
  const release = command === 'release';
  if (release && output('git', ['status', '--porcelain'])) throw new Error('Release needs a clean checkout; commit your intended source changes first');
  const version = versionCheck();
  const source = output('git', ['rev-parse', 'HEAD']);
  const epoch = Math.floor(Date.now() / 1000).toString();
  const env = { ...signingEnv(root, release), MACOSX_DEPLOYMENT_TARGET: settings.minimum_system_version, LOPPI_BUILD_EPOCH: epoch, CARGO_TARGET_DIR: resolve('target') };
  const local = existsSync('.env') ? parseEnv(readFileSync('.env', 'utf8')) : {};
  for (const key of ['APPLE_SIGNING_IDENTITY', 'APPLE_NOTARY_PROFILE', 'APPLE_API_KEY', 'APPLE_API_ISSUER', 'APPLE_API_KEY_PATH']) {
    if (env[key] === undefined && local[key] !== undefined) env[key] = local[key];
  }
  if (env.APPLE_API_KEY_PATH) env.APPLE_API_KEY_PATH = env.APPLE_API_KEY_PATH.replace(/^~(?=\/)/, homedir());
  if (release) {
    env.APPLE_SIGNING_IDENTITY ||= output('security', ['find-identity', '-v', '-p', 'codesigning']).match(/"(Developer ID Application:[^"]+)"/)?.[1];
    if (!env.APPLE_SIGNING_IDENTITY) throw new Error('Developer ID Application identity is required');
    if (!env.APPLE_NOTARY_PROFILE && !(env.APPLE_API_KEY && env.APPLE_API_KEY_PATH)) throw new Error('Notarization credentials are required');
  }
  const installed = output('rustup', ['target', 'list', '--installed']).split('\n');
  const rustTargets = [...new Set(targets.flatMap(t => settings.targets[t].rust_targets))];
  if (rustTargets.some(t => !installed.includes(t))) throw new Error(`Install Rust targets: ${rustTargets.join(' ')}`);
  if (/target-cpu[= ]native/.test(env.RUSTFLAGS ?? '')) throw new Error('Do not use target-cpu=native for distributable binaries');
  const dirty = Boolean(output('git', ['status', '--porcelain']));
  const artifacts = [];
  for (const target of targets) {
    artifacts.push(buildTarget(target, { release, version, source, epoch, dirty, env }));
  }
  console.log(`\n${release ? 'Release' : 'Development'} packages ready:\n${artifacts.join('\n')}`);
}

function buildTarget(target, { release, version, source, epoch, dirty, env: baseEnv }) {
  const platform = settings.targets[target];
  const env = { ...baseEnv };
  const stage = resolve('build/macos', `${target}-${epoch}-${process.pid}`);
  const app = resolve('target', target, 'release/bundle/macos/loppi.app');
  if (output('/bin/ps', ['-axo', 'comm=']).split('\n').some(p => p.trim().startsWith(`${app}/`))) {
    throw new Error('Quit the application running from this build output before rebuilding');
  }
  mkdirSync(stage, { recursive: true });
  const modelCache = prepareModelCache();
  env.SHERPA_ONNX_LIB_DIR = prepareNativeLibraries(join(stage, 'native'));
  for (const rustTarget of platform.rust_targets) {
    run('cargo', ['build', '--locked', '--release', '--target', rustTarget, '-p', 'loppi-core', '-p', 'loppi-cli', '-p', 'loppi-worker'], env);
  }
  const helper = join(stage, 'LoppiCore.app');
  mkdirSync(join(helper, 'Contents/MacOS'), { recursive: true });
  cpSync('src-tauri/native/LoppiCore-Info.plist', join(helper, 'Contents/Info.plist'));
  for (const name of ['loppi-core', 'loppi-worker', 'loppi']) {
    const destination = name === 'loppi' ? join(stage, name) : join(helper, 'Contents/MacOS', name);
    const inputs = platform.rust_targets.map(t => resolve('target', t, 'release', name));
    if (inputs.length === 1) cpSync(inputs[0], destination);
    else run('lipo', ['-create', ...inputs, '-output', destination]);
    architectures(destination, platform.architectures);
  }
  const bundledModels = join(helper, 'Contents/Resources/models');
  mkdirSync(bundledModels, { recursive: true });
  for (const spec of models.files) {
    const destination = join(bundledModels, spec.path);
    mkdirSync(dirname(destination), { recursive: true });
    cpSync(join(modelCache, spec.path), destination);
  }
  writeFileSync(join(bundledModels, 'models.lock.json'), modelLockBytes);
  cpSync('THIRD_PARTY_NOTICES.md', join(helper, 'Contents/Resources/THIRD_PARTY_NOTICES.md'));
  cpSync('licenses', join(helper, 'Contents/Resources/licenses'), { recursive: true });
  const override = join(stage, 'tauri.json');
  writeFileSync(override, JSON.stringify({ bundle: { createUpdaterArtifacts: false, macOS: { files: { 'Library/LoginItems/LoppiCore.app': helper, 'MacOS/loppi': join(stage, 'loppi') } } } }));
  // Signing/notarizing is deliberately deferred until the entire bundle is final.
  const unsignedEnv = { ...env };
  for (const key of Object.keys(unsignedEnv).filter(k => k.startsWith('APPLE_'))) delete unsignedEnv[key];
  run('npm', ['run', 'tauri', 'build', '--', '--target', target, '--bundles', 'app', '--config', override], unsignedEnv);
  const paths = ['Contents/MacOS/loppi-desktop', 'Contents/MacOS/loppi', 'Contents/Library/LoginItems/LoppiCore.app/Contents/MacOS/loppi-core', 'Contents/Library/LoginItems/LoppiCore.app/Contents/MacOS/loppi-worker'];
  for (const path of paths) {
    architectures(join(app, path), platform.architectures);
    minimumOS(join(app, path));
  }
  const modelRoot = join(app, 'Contents/Library/LoginItems/LoppiCore.app/Contents/Resources/models');
  for (const spec of models.files) if (!checked(join(modelRoot, spec.path), spec)) throw new Error(`Bundled asset mismatch: ${spec.path}`);
  if (!readFileSync(join(modelRoot, 'models.lock.json')).equals(modelLockBytes)) throw new Error('Bundled lock mismatch');
  const nativeArch = hostArchitecture();
  const nativeExecution = platform.architectures.includes(nativeArch);
  const rosettaExecution = !nativeExecution && nativeArch === 'arm64' && platform.architectures.includes('x86_64')
    && spawnSync('/usr/bin/arch', ['-x86_64', '/usr/bin/true'], { stdio: 'ignore' }).status === 0;
  const execution = nativeExecution ? 'native' : rosettaExecution ? 'rosetta' : 'not_available';
  const identityPath = join(app, 'Contents/Resources/build-info.json');
  rmSync(identityPath, { force: true });
  let identity = null;
  if (execution !== 'not_available') {
    run('python3', ['scripts/verify_build_identity.py', '--bundle', app, '--output', identityPath]);
    identity = JSON.parse(readFileSync(identityPath));
    const actual = identity.components.gui;
    if (actual.version !== version || actual.git_sha !== source || actual.dirty !== dirty || actual.built_at_epoch !== Number(epoch)) throw new Error('Compiled identity disagrees with build invocation');
    run('python3', ['scripts/smoke_model_preparation.py', '--bundle', app]);
  }
  const runtime = execution !== 'not_available' ? JSON.parse(output(join(app, paths[3]), ['--runtime-info'])) : null;
  if (runtime && runtime.sherpa_onnx.replace(/^v/, '') !== settings.native.version) throw new Error('Linked engine version disagrees with lock');
  const checkedArch = runtime ? (runtime.architecture === 'aarch64' ? 'arm64' : runtime.architecture) : null;
  if (checkedArch && !platform.architectures.includes(checkedArch)) throw new Error('Worker runtime architecture disagrees with target');
  const report = {
    version, source, dirty, target, architectures: platform.architectures, built_at_epoch: Number(epoch),
    models_lock_sha256: sha(modelLockBytes), models_bytes: models.files.reduce((n, f) => n + f.bytes, 0),
    native: { version: settings.native.version, sha256: settings.native.sha256 },
    toolchain: { node: process.version, rust: output('rustc', ['--version']), xcode: output('xcodebuild', ['-version']), sdk: output('xcrun', ['--show-sdk-version']) },
    identity, runtime, runtime_execution: checkedArch ? (checkedArch === nativeArch ? 'native' : 'rosetta') : 'not_available',
    runtime_checked_architecture: checkedArch,
    intel_native_test: checkedArch === 'x86_64' && nativeArch === 'x86_64' ? 'passed' : 'not run on Intel hardware', minimum_os_runtime_test: 'not run',
  };
  if (release) {
    const bundledHelper = join(app, 'Contents/Library/LoginItems/LoppiCore.app');
    const sign = (path, entitlements = false) => run('codesign', ['--force', '--options', 'runtime', '--timestamp', ...(entitlements ? ['--entitlements', 'src-tauri/native/LoppiCore.entitlements'] : []), '--sign', env.APPLE_SIGNING_IDENTITY, path]);
    sign(join(bundledHelper, 'Contents/MacOS/loppi-worker'));
    sign(bundledHelper, true);
    sign(join(app, 'Contents/MacOS/loppi'));
    sign(app, true);
    run('codesign', ['--verify', '--deep', '--strict', app]);
  }
  const destination = resolve(release ? 'build/releases' : 'build/dev', version, platform.name);
  mkdirSync(destination, { recursive: true });
  const dmg = join(destination, `loppi_${version}_${platform.name}${release ? '' : '-dev'}.dmg`);
  run('/bin/zsh', ['scripts/package_macos.sh', ...(!release ? ['--dev'] : []), app, dmg], env);
  report.dmg = dmg.split('/').pop();
  report.dmg_bytes = lstatSync(dmg).size;
  report.dmg_sha256 = sha(readFileSync(dmg));
  let checksums = `${report.dmg_sha256}  ${report.dmg}\n`;
  if (release) {
    const archive = join(destination, `loppi_${version}_${platform.name}.app.tar.gz`);
    // package_macos.sh notarizes and staples the final App before this archive.
    createMacUpdateArchive(app, archive);
    const targets = platform.architectures.map(arch => `darwin-${arch === 'arm64' ? 'aarch64' : arch}`);
    report.updater = writePlatformManifest({ root, artifact: archive, version, source, targets, env });
    checksums += `${report.updater.sha256}  ${report.updater.artifact}\n`;
  }
  writeFileSync(join(destination, release ? 'release-info.json' : 'build-info.json'), JSON.stringify(report, null, 2) + '\n');
  writeFileSync(join(destination, 'SHA256SUMS'), checksums);
  return dmg;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { if(process.platform === 'win32') { await (await import('./build_windows.mjs')).main(); } else { main(); } } catch (error) { console.error(error.message); process.exitCode = 1; }
}
