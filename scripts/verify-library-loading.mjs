// Isolated synthetic-library verification. Requires Node 22.13+ for node:sqlite.
// No personal library, recording, model download, sign-in or external request is used.
import assert from 'node:assert/strict';
import { spawn, execFileSync } from 'node:child_process';
import { mkdtemp, mkdir, readFile, writeFile, rm } from 'node:fs/promises';
import { createHash, randomUUID } from 'node:crypto';
import { createInterface } from 'node:readline';
import { tmpdir, platform, arch, totalmem, cpus } from 'node:os';
import path from 'node:path';
import * as sqlite from 'node:sqlite';

const args = process.argv.slice(2);
function option(name, fallback) {
  const i = args.indexOf(name);
  if (i < 0) return fallback;
  assert(args[i + 1], 'Missing value for ' + name);
  return args[i + 1];
}
function boundedInteger(value, low, high) {
  const n = Number(value);
  assert(Number.isInteger(n) && n >= low && n <= high, 'Invalid fixture size');
  return n;
}
const engine = path.resolve(option('--engine', 'src-tauri/target/debug/tuck-runtime'));
const count = boundedInteger(option('--meetings', '1000'), 120, 2000);
const segmentsPerMeeting = boundedInteger(option('--segments', '120'), 2, 300);
const destination = path.resolve(option('--out', 'benchmarks/native-workflow/library-loading.json'));
const compareFull = args.includes('--compare-full');
const library = await mkdtemp(path.join(tmpdir(), 'tuck-library-loading-'));
const checks = [];
let requests = 0;
function host() {
  const child = spawn(engine, ['--data-dir', library], { stdio: ['pipe', 'pipe', 'pipe'] });
  const launched = performance.now();
  const pending = new Map();
  let stderr = '';
  child.stderr.on('data', chunk => { stderr = (stderr + chunk).slice(-2000); });
  const closed = new Promise(resolve => child.once('close', code => {
    for (const request of pending.values()) {
      clearTimeout(request.timer);
      request.reject(new Error('Runtime exited ' + code + ': ' + stderr));
    }
    pending.clear();
    resolve(code);
  }));
  createInterface({ input: child.stdout }).on('line', line => {
    const value = JSON.parse(line);
    const request = pending.get(value.id);
    if (!request) return;
    pending.delete(value.id);
    clearTimeout(request.timer);
    if (value.error) request.reject(Object.assign(new Error(value.error.message), { code: value.error.code }));
    else request.resolve({ result: value.result, responseBytes: Buffer.byteLength(line), durationMs: performance.now() - request.started });
  });
  function measured(method, params = {}) {
    requests++;
    return new Promise((resolve, reject) => {
      const id = randomUUID();
      const timer = setTimeout(() => { pending.delete(id); reject(new Error(method + ' timed out')); }, 120000);
      pending.set(id, { resolve, reject, timer, started: performance.now() });
      child.stdin.write(JSON.stringify({ id, method, params }) + '\n');
    });
  }
  return { child, closed, launched, measured, call: async (method, params) => (await measured(method, params)).result };
}
function rss(pid) {
  if (!['darwin', 'linux'].includes(platform())) return null;
  try {
    return Number(execFileSync('ps', ['-p', String(pid), '-o', 'rss='], { encoding: 'utf8' }).trim()) * 1024;
  } catch { return null; }
}
async function shutdown(runtime) {
  await runtime.call('shutdown');
  assert.equal(await runtime.closed, 0);
}
function assertHeader(header) {
  for (const key of ['notes', 'summary', 'tasks', 'segments', 'speakers', 'messages', 'bookmarks']) {
    assert(!(key in header), 'Header leaked detail body: ' + key);
  }
  assert.equal(header.segmentCount, segmentsPerMeeting);
}
let runtime;
try {
  runtime = host();
  const template = await runtime.call('create_meeting', { title: 'Synthetic fixture seed' });
  await shutdown(runtime);
  const Database = sqlite.Database || sqlite.DatabaseSync;
  const db = new Database(path.join(library, 'tuck.sqlite3'));
  const stored = JSON.parse(db.prepare('SELECT document FROM meetings WHERE id=?').get(template.id).document);
  const insertMeeting = db.prepare('INSERT INTO meetings(id,created_at,document,version) VALUES(?,?,?,1)');
  const insertSegment = db.prepare('INSERT INTO segments(meeting_id,id,start,document) VALUES(?,?,?,?)');
  const identities = [];
  let transcriptBytes = 0;
  db.exec('BEGIN');
  db.prepare('DELETE FROM meetings WHERE id=?').run(template.id);
  for (let i = 0; i < count; i++) {
    const id = randomUUID();
    const createdAt = new Date(Date.UTC(2026, 0, 1) + Math.floor(i / 10) * 60000).toISOString();
    const meeting = {
      ...stored, id, createdAt, title: 'Synthetic meeting ' + i, status: 'complete', duration: segmentsPerMeeting * 5,
      notes: ('Synthetic personal note ' + i + '. ').repeat(100), summary: ('Synthetic recap ' + i + '. ').repeat(80),
      folder: i === 0 ? 'Oldest-only folder' : 'Customer calls', tags: ['Synthetic', i % 2 ? 'Odd' : 'Even'],
      people: [i === 0 ? 'Oldest-only person' : 'Synthetic colleague'], starred: i % 7 === 0,
      tasks: i === 0 ? [{ id: randomUUID(), title: 'Oldest meeting action', owner: 'Synthetic owner', dueDate: '2026-11-01', done: false, createdAt }] : [],
    };
    insertMeeting.run(id, createdAt, JSON.stringify(meeting));
    identities.push({ id, createdAt, index: i });
    for (let j = 0; j < segmentsPerMeeting; j++) {
      const text = i === 0 && j === 0 ? 'rare-original-token belongs to the oldest meeting' : ('Synthetic spoken evidence ' + i + ' ' + j + '. ').repeat(12);
      const segment = { id: randomUUID(), meetingId: id, start: j * 5, end: j * 5 + 4, text, speakerId: 'speaker-1', source: 'import' };
      const document = JSON.stringify(segment);
      insertSegment.run(id, segment.id, segment.start, document);
      transcriptBytes += Buffer.byteLength(document);
    }
  }
  db.exec('COMMIT');
  const firstSegment = db.prepare('SELECT id FROM segments WHERE meeting_id=? ORDER BY start LIMIT 1').get(identities[0].id).id;
  db.close();
  identities.sort((a, b) => b.createdAt.localeCompare(a.createdAt) || b.id.localeCompare(a.id));
  const oldest = identities.find(row => row.index === 0);
  const newest = identities[0];

  runtime = host();
  const lean = await runtime.measured('bootstrap_library');
  const leanMetrics = { responseBytes: lean.responseBytes, launchToResponseMs: performance.now() - runtime.launched, requestMs: lean.durationMs, rustRssAfterResponseBytes: rss(runtime.child.pid) };
  assert.equal(lean.result.library.meetingCount, count);
  assert.equal(lean.result.library.openTaskCount, 1);
  assert(lean.result.library.folders.includes('Oldest-only folder'));
  assert(lean.result.library.people.includes('Oldest-only person'));
  assert(lean.responseBytes < 160000, 'Lean bootstrap exceeded the fixed header budget');
  assert.equal(lean.result.meetingPage.meetings.length, 50);
  lean.result.meetingPage.meetings.forEach(assertHeader);
  checks.push('bounded first-page payload without transcript, notes or recap bodies');

  const seen = [];
  let page = lean.result.meetingPage;
  do {
    page.meetings.forEach(header => { assertHeader(header); seen.push(header.id); });
    page = page.nextCursor ? await runtime.call('list_meetings', { cursor: page.nextCursor, limit: 100 }) : null;
  } while (page);
  assert.deepEqual(seen, identities.map(row => row.id));
  checks.push('all pages include every meeting once in deterministic timestamp-tie order');

  const tasks = await runtime.call('list_task_page');
  assert.equal(tasks.total, 1);
  assert.equal(tasks.tasks[0].meetingId, oldest.id);
  const filtered = await runtime.call('list_meetings', { scope: { folder: 'Oldest-only folder', people: ['Oldest-only person'] } });
  assert.deepEqual(filtered.meetings.map(row => row.id), [oldest.id]);
  const searched = await runtime.call('list_meetings', { query: 'rare-original-token' });
  assert.deepEqual(searched.meetings.map(row => row.id), [oldest.id]);
  assert.equal((await runtime.call('list_meetings', { starred: true })).total, Math.ceil(count / 7));
  checks.push('global facets, task inbox, starred and transcript search include off-page meetings');

  const detail = await runtime.measured('get_meeting', { meetingId: oldest.id });
  assert.equal(detail.result.segments.length, segmentsPerMeeting);
  assert.equal(detail.result.tasks.length, 1);
  const cursorPage = await runtime.call('list_meetings', { limit: 1 });
  await runtime.call('correct_transcript', { meetingId: oldest.id, segmentId: firstSegment, baseText: detail.result.segments[0].text, text: 'rare-corrected-token belongs to the oldest meeting' });
  await assert.rejects(runtime.call('list_meetings', { cursor: cursorPage.nextCursor }), error => error.code === 'cursor_stale');
  assert.equal((await runtime.call('list_meetings', { query: 'rare-original-token' })).total, 0);
  assert.equal((await runtime.call('list_meetings', { query: 'rare-corrected-token' })).meetings[0].id, oldest.id);
  await assert.rejects(runtime.call('list_meetings', { limit: 101 }));
  await assert.rejects(runtime.call('list_meetings', { cursor: 'not-a-valid-cursor' }));
  checks.push('on-demand details, correction-aware search, stale cursors and request bounds');
  await shutdown(runtime);

  let fullMetrics = null;
  if (compareFull) {
    runtime = host();
    const full = await runtime.measured('bootstrap');
    fullMetrics = { responseBytes: full.responseBytes, launchToResponseMs: performance.now() - runtime.launched, requestMs: full.durationMs, rustRssAfterResponseBytes: rss(runtime.child.pid) };
    assert.equal(full.result.meetings.length, count);
    assert(full.responseBytes > transcriptBytes);
    assert(lean.responseBytes < full.responseBytes / 100);
    await shutdown(runtime);
    checks.push('same-binary full-bootstrap comparison shows over 100x less transfer');
  }

  // A malformed unselected speech document must not be decoded during startup/listing.
  const corrupt = new Database(path.join(library, 'tuck.sqlite3'));
  corrupt.prepare('UPDATE segments SET document=? WHERE meeting_id=? AND id=?').run('{invalid synthetic fixture', oldest.id, firstSegment);
  corrupt.close();
  runtime = host();
  const reopened = await runtime.call('bootstrap_library');
  assert.equal(reopened.library.meetingCount, count);
  assert.equal((await runtime.call('get_meeting', { meetingId: newest.id })).segments.length, segmentsPerMeeting);
  await assert.rejects(runtime.call('get_meeting', { meetingId: oldest.id }), error => error.code === 'storage_error');
  await shutdown(runtime);
  checks.push('startup and unrelated selection do not decode an unselected malformed transcript');

  const result = {
    verifiedAt: new Date().toISOString(), engineSha256: createHash('sha256').update(await readFile(engine)).digest('hex'),
    fixture: { meetings: count, segments: count * segmentsPerMeeting, transcriptDocumentBytes: transcriptBytes, synthetic: true },
    hardware: { platform: platform(), architecture: arch(), ramBytes: totalmem(), cpu: cpus()[0]?.model },
    requests, lean: leanMetrics, full: fullMetrics, selectedDetail: { responseBytes: detail.responseBytes, requestMs: detail.durationMs }, checks,
    caveats: ['Diagnostic Rust composition only; excludes WebKit, speech models, local LLM and video calls.', 'RSS sampled after first response, not peak memory or total machine use.', 'Single-run timings with shared OS caches; not cold-start or minimum-spec qualification.', 'Selected meetings still load their complete transcript.'],
    minimum16GBRealCallQualified: false,
  };
  await mkdir(path.dirname(destination), { recursive: true });
  await writeFile(destination, JSON.stringify(result, null, 2) + '\n');
  console.log(JSON.stringify(result, null, 2));
} finally {
  if (runtime?.child.exitCode === null) { runtime.child.kill(); await runtime.closed; }
  await rm(library, { recursive: true, force: true });
}
