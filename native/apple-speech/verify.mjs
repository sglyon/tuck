// Explicit integration verification: public PCM fixture + already installed models.
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {readFileSync} from 'node:fs';
import {createInterface} from 'node:readline';
import {fileURLToPath} from 'node:url';

const root = fileURLToPath(new URL('../../', import.meta.url));
const [models, fixture] = process.argv.slice(2);
if (!models || !fixture) throw new Error('Usage: node native/apple-speech/verify.mjs MODEL_DIR PCM_F32_FILE');
const pcm = readFileSync(fixture);
assert.equal(pcm.length % 4, 0);
assert.ok(pcm.length > 0 && pcm.length <= 240_000 * 4);

function helper(directory = models) {
  const child = spawn(process.env.TUCK_SPEECH_HELPER ?? `${root}src-tauri/binaries/tuck-speech`, ['--models', directory], {stdio: ['pipe', 'pipe', 'pipe']});
  const queue = [];
  const pending = [];
  let ended = false;
  let diagnostics = '';
  child.stderr.on('data', data => { diagnostics += data; });
  const lines = createInterface({input: child.stdout});
  lines.on('line', line => {
    assert.ok(line.startsWith('TUCK_SPEECH:'), 'Unexpected stdout output');
    const item = JSON.parse(line.slice('TUCK_SPEECH:'.length));
    if (pending.length) pending.shift().resolve(item); else queue.push(item);
  });
  const exit = new Promise((resolve, reject) => {
    child.on('error', reject);
    child.on('exit', code => {
      ended = true;
      for (const waiter of pending.splice(0)) waiter.reject(new Error(`Helper exited (${code}): ${diagnostics}`));
      resolve(code);
    });
  });
  const timer = setTimeout(() => child.kill('SIGKILL'), 180_000);
  exit.finally(() => clearTimeout(timer));
  return {
    child, exit,
    next: () => queue.length ? Promise.resolve(queue.shift()) : ended
      ? Promise.reject(new Error('Helper already exited'))
      : new Promise((resolve, reject) => pending.push({resolve, reject})),
    header: value => Buffer.from(JSON.stringify(value) + '\n'),
  };
}

async function ready(handle) {
  const event = await handle.next();
  assert.equal(event.event, 'ready');
  assert.equal(event.engine, 'coreml');
  assert.equal(event.computeUnits, 'cpuAndNeuralEngine');
  console.log('Helper ready.');
}

function transcript(result, id) {
  assert.equal(result.id, id);
  assert.match(result.text, /what your country can do for you/i);
  assert.ok(result.tokenTimings.every(token => token.startTime >= 0 && token.endTime <= result.duration));
}

{
  const h = helper();
  await ready(h);
  // Header and PCM boundaries deliberately split at non-float byte boundaries.
  const header = h.header({id: 1, op: 'transcribe', samples: pcm.length / 4});
  h.child.stdin.write(header.subarray(0, 7));
  h.child.stdin.write(Buffer.concat([header.subarray(7), pcm.subarray(0, 11)]));
  h.child.stdin.write(pcm.subarray(11));
  transcript(await h.next(), 1);
  console.log('Split frame verified.');
  // A short frame must respond while stdin stays open (no read-to-fill deadlock).
  h.child.stdin.write(Buffer.concat([h.header({id: 6, op: 'transcribe', samples: 16}), Buffer.alloc(64)]));
  const tiny = await h.next();
  assert.equal(tiny.id, 6);
  assert.equal(tiny.error, undefined);
  // Two complete frames sent together must produce two ordered responses.
  h.child.stdin.write(Buffer.concat([
    h.header({id: 2, op: 'transcribe', samples: pcm.length / 4}), pcm,
    h.header({id: 3, op: 'transcribe', samples: pcm.length / 4}), pcm,
    h.header({op: 'shutdown'}),
  ]));
  transcript(await h.next(), 2);
  console.log('First coalesced frame verified.');
  transcript(await h.next(), 3);
  console.log('Second coalesced frame verified.');
  assert.equal(await h.exit, 0);
}

for (const header of [
  {id: 4, op: 'transcribe', samples: 240_001},
  {id: 4, op: 'transcribe', samples: -1},
  {id: 4, op: 'transcribe', samples: 0},
]) {
  const h = helper();
  await ready(h);
  h.child.stdin.end(h.header(header));
  assert.equal((await h.next()).event, 'error');
  assert.equal(await h.exit, 1);
}

for (const payload of [Buffer.from([0, 0, 192, 127]), Buffer.alloc(3)]) {
  const h = helper();
  await ready(h);
  h.child.stdin.end(Buffer.concat([h.header({id: 5, op: 'transcribe', samples: 1}), payload]));
  const result = await h.next();
  assert.equal(result.id, 5);
  assert.ok(result.error);
  await h.exit;
}

{
  const h = helper();
  await ready(h);
  h.child.stdin.end(Buffer.alloc(8_192, 65));
  assert.equal((await h.next()).event, 'error');
  assert.equal(await h.exit, 1);
}
{
  const h = helper('/tmp/tuck-apple-speech-intentionally-missing-models');
  assert.equal((await h.next()).event, 'error');
  assert.equal(await h.exit, 1);
}
console.log('Apple speech helper verified: real transcript, split/coalesced frames, ordered requests, shutdown, bounds, invalid floats, truncated PCM, oversized headers, and local-only missing-model failure.');
