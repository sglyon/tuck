import {spawnSync} from 'node:child_process';
import {copyFileSync, existsSync, mkdirSync, readFileSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import path from 'node:path';

if (process.platform !== 'darwin') {
  console.log('The Apple Core ML helper is built only on macOS.');
  process.exit(0);
}

const root = fileURLToPath(new URL('../', import.meta.url));
const packagePath = path.join(root, 'native/apple-speech');
const cache = path.join(root, 'src-tauri/target/apple-speech-build');
const source = process.env.TUCK_FLUIDAUDIO_SOURCE ?? path.join(cache, 'FluidAudio');
const revision = '503b4bd1bbf7220882de39fe8ae6716aae4132da';
const patchFile = path.join(packagePath, 'omit-unused-nemo.patch');

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {cwd: root, encoding: 'utf8', ...options});
  if (result.status !== 0) {
    if (result.stdout) process.stderr.write(result.stdout);
    if (result.stderr) process.stderr.write(result.stderr);
    throw new Error(`${command} failed (${result.status ?? 'could not start'}).`);
  }
  return result.stdout?.trim() ?? '';
}

mkdirSync(cache, {recursive: true});
if (!existsSync(path.join(source, '.git'))) {
  run('git', ['clone', '--branch', 'v0.17.7', '--depth', '1',
    'https://github.com/FluidInference/FluidAudio.git', source], {stdio: 'inherit'});
}
if (run('git', ['-C', source, 'rev-parse', 'HEAD']) !== revision) {
  throw new Error('FluidAudio source does not match the pinned revision.');
}
const status = run('git', ['-C', source, 'status', '--porcelain']);
if (status && status !== 'M Package@swift-6.2.swift') {
  throw new Error('FluidAudio source has unexpected modifications; use a clean pinned checkout.');
}
// traits: [] disables this binary in the SDK, but SwiftPM still attempted to
// download it. Apply only the reviewed manifest patch, keeping ASR source exact.
const reversed = spawnSync('git', ['-C', source, 'apply', '--reverse', '--check', patchFile]);
if (reversed.status !== 0) {
  run('git', ['-C', source, 'apply', '--check', patchFile]);
  run('git', ['-C', source, 'apply', patchFile]);
}
const actualPatch = run('git', ['-C', source, 'diff', '--', 'Package@swift-6.2.swift']);
if (actualPatch !== readFileSync(patchFile, 'utf8').trim()) {
  throw new Error('FluidAudio manifest differs from the reviewed opt-out patch.');
}
console.log(`Building native Apple speech with FluidAudio ${revision} (manifest-only NeMo opt-out).`);
const env = {...process.env, TUCK_FLUIDAUDIO_SOURCE: source};
run('swift', ['build', '-c', 'release', '--package-path', packagePath,
  '--cache-path', path.join(cache, 'cache'), '--config-path', path.join(cache, 'config'),
  '--security-path', path.join(cache, 'security')], {stdio: 'inherit', env});
const binPath = run('swift', ['build', '-c', 'release', '--show-bin-path',
  '--package-path', packagePath, '--cache-path', path.join(cache, 'cache'),
  '--config-path', path.join(cache, 'config'), '--security-path', path.join(cache, 'security')], {env});
const destination = path.join(root, 'src-tauri/binaries');
mkdirSync(destination, {recursive: true});
copyFileSync(path.join(binPath, 'tuck-speech'), path.join(destination, 'tuck-speech'));
console.log('Built src-tauri/binaries/tuck-speech. Model installation remains a separate explicit action.');
