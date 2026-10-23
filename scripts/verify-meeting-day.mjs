// Real native composition, temporary libraries and generated calendar fixtures only.
// No calendar helper, OS authorization, notification, microphone or provider is used.
import assert from 'node:assert/strict';
import {spawn, execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {mkdtemp, mkdir, readFile, writeFile, rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import path from 'node:path';
import {createInterface} from 'node:readline';
import {randomUUID, createHash} from 'node:crypto';
import {DatabaseSync} from 'node:sqlite';

const engine = path.resolve(process.env.TUCK_RUNTIME_BIN || 'src-tauri/target/debug/tuck-runtime');
const root = await mkdtemp(path.join(tmpdir(), 'tuck-meeting-day-'));
const hosts = [];
let requests = 0;
const checks = [];
const now = Date.now();
const date = minutes => new Date(now + minutes * 60000).toISOString().replace(/[-:]/g, '').replace(/\.\d{3}Z$/, 'Z');
const event = (id, minutes, extra = 'ATTENDEE;CN=Morgan:mailto:morgan@example.com', duration = 30) => [
  'BEGIN:VEVENT', `UID:${id}`, `SUMMARY:Fixture ${id}`, `DTSTART:${date(minutes)}`,
  `DTEND:${date(minutes + duration)}`, extra, 'END:VEVENT',
].filter(Boolean).join('\r\n');
const calendar = events => ['BEGIN:VCALENDAR', 'VERSION:2.0', ...events, 'END:VCALENDAR', ''].join('\r\n');
async function host(name) {
  const data = path.join(root, name); await mkdir(data, {recursive: true});
  const child = spawn(engine, ['--data-dir', data], {stdio: ['pipe', 'pipe', 'pipe']});
  const pending = new Map(); let stderr = '';
  child.stderr.on('data', chunk => { stderr = (stderr + chunk).slice(-3000); });
  const closed = new Promise(resolve => child.once('close', code => {
    for (const p of pending.values()) { clearTimeout(p.timer); p.reject(new Error(`Runtime exited ${code}: ${stderr}`)); }
    pending.clear(); resolve(code);
  }));
  child.once('error', error => { for (const p of pending.values()) { clearTimeout(p.timer); p.reject(error); } pending.clear(); });
  createInterface({input: child.stdout}).on('line', line => {
    const value = JSON.parse(line), p = pending.get(value.id); if (!p) return;
    pending.delete(value.id); clearTimeout(p.timer);
    value.error ? p.reject(Object.assign(new Error(value.error.message), {code: value.error.code})) : p.resolve(value.result);
  });
  const call = (method, params = {}) => new Promise((resolve, reject) => {
    requests++; const id = randomUUID();
    const timer = setTimeout(() => { pending.delete(id); reject(new Error(`${method} timed out: ${stderr}`)); }, 20000);
    pending.set(id, {resolve, reject, timer}); child.stdin.write(JSON.stringify({id, method, params}) + '\n');
  });
  const result = {data, child, closed, call, async stop() { await call('shutdown'); assert.equal(await closed, 0); }};
  hosts.push(result); return result;
}
try {
  let source = await host('source');
  assert.equal((await source.call('bootstrap_library')).settings.meetingReminders, false);
  const status = await source.call('meeting_reminder_status');
  assert.equal(status.available, false); assert.equal(status.running, false); assert.equal(status.permission, 'unavailable');
  await assert.rejects(source.call('set_meeting_reminders', {enabled: true}));
  await assert.rejects(source.call('update_settings', {changes: {meetingReminders: true}}));
  assert.equal((await source.call('get_meeting_day')).enabled, false);
  checks.push('diagnostic host has no notification worker or permission path; default off and generic preference bypass rejected');

  const fixture = path.join(root, 'generated.ics');
  const allDay = ['BEGIN:VEVENT', 'UID:all-day', 'SUMMARY:All day fixture', `DTSTART;VALUE=DATE:${date(0).slice(0, 8)}`, 'DURATION:P1D', 'END:VEVENT'].join('\r\n');
  await writeFile(fixture, calendar([
    event('due', 3), event('future', 30), event('solo', 2, ''), event('too-late', -5),
    event('ended', -60), allDay,
  ]));
  const imported = await source.call('import_calendar', {path: fixture});
  assert.equal(imported.length, 6);
  const day = await source.call('get_meeting_day');
  assert.equal(day.events.length, 3); assert.deepEqual(day.reminders, []);
  assert.equal(day.snapshots[0].scope, 'import');
  const occurrence = day.events.find(o => o.event.title === 'Fixture due'); assert(occurrence);
  const prepared = await source.call('prepare_calendar_meeting', {event: occurrence.event});
  assert.equal(prepared.status, 'draft'); assert.equal(prepared.summary, ''); assert.equal(prepared.notionUrl, '');
  assert.deepEqual(prepared.people, ['Morgan']); assert.deepEqual(prepared.segments, []);
  await source.call('update_meeting', {id: prepared.id, changes: {title: 'My durable title', notes: 'Keep my preparation.'}});
  const equivalent = {...occurrence.event, title: 'Changed upstream title', attendees: ['Other person'], start: new Date(occurrence.event.start).toISOString().replace('Z', '+00:00')};
  const reused = await source.call('prepare_calendar_meeting', {event: equivalent});
  assert.equal(reused.id, prepared.id); assert.equal(reused.title, 'My durable title'); assert.equal(reused.notes, 'Keep my preparation.');
  assert.deepEqual(reused.calendarEvent, prepared.calendarEvent);
  const recurrence = {...occurrence.event, start: new Date(new Date(occurrence.event.start).getTime() + 86400000).toISOString(), end: new Date(new Date(occurrence.event.end).getTime() + 86400000).toISOString()};
  assert.notEqual((await source.call('prepare_calendar_meeting', {event: recurrence})).id, prepared.id);
  checks.push('native ICS import filters ended/all-day/old starts; atomic preparation reuses and preserves notes while separating recurrence');
  await source.stop();

  // Seed only this stopped, disposable fixture library: diagnostics deliberately
  // cannot grant native authorization. This exercises enabled core reads without
  // introducing a production fake-clock or permission-bypass RPC.
  const db = new DatabaseSync(path.join(source.data, 'tuck.sqlite3'));
  assert.equal(db.prepare('PRAGMA user_version').get().user_version, 4);
  const raw = db.prepare('SELECT document FROM settings WHERE id=1').get();
  const settings = raw ? JSON.parse(raw.document) : {};
  settings.meetingReminders = true;
  db.prepare('INSERT INTO settings(id,document) VALUES(1,?) ON CONFLICT(id) DO UPDATE SET document=excluded.document').run(JSON.stringify(settings));
  db.close();
  source = await host('source');
  const restoredDay = await source.call('get_meeting_day');
  assert.equal(restoredDay.enabled, true); assert.equal(restoredDay.suppressed, false);
  assert.equal(restoredDay.reminders.length, 1); assert.equal(restoredDay.reminders[0].meetingId, prepared.id);
  assert.equal((await source.call('prepare_calendar_meeting', {event: occurrence.event})).notes, 'Keep my preparation.');
  await source.call('dismiss_meeting_reminder', {occurrenceKey: occurrence.occurrenceKey});
  assert.deepEqual((await source.call('get_meeting_day')).reminders, []);
  await source.call('import_calendar', {path: fixture});
  assert.deepEqual((await source.call('get_meeting_day')).reminders, []);
  checks.push('SQLite v4 restart preserves snapshot, workspace identity and durable dismissal across snapshot replacement; solo/future events do not nag');

  const backup = path.join(root, 'library.zip');
  await source.call('backup_library', {path: backup, includeAudio: false});
  const {stdout} = await promisify(execFile)('unzip', ['-p', backup, 'manifest.json'], {maxBuffer: 8 * 1024 * 1024});
  const manifest = JSON.parse(stdout);
  assert.equal(manifest.settings.meetingReminders, false);
  assert(!stdout.includes('calendar_snapshots')); assert(!stdout.includes('occurrenceKey'));
  assert(!stdout.includes('Fixture future')); assert(!stdout.includes('Fixture solo'));
  const target = await host('target');
  const preview = await target.call('preview_restore_library', {path: backup});
  await target.call('restore_library', {path: backup, expectedFingerprint: preview.fingerprint});
  const targetDay = await target.call('get_meeting_day');
  assert.equal(targetDay.enabled, false); assert.deepEqual(targetDay.events, []); assert.deepEqual(targetDay.snapshots, []);
  assert.equal((await target.call('get_meeting', {meetingId: prepared.id})).notes, 'Keep my preparation.');
  await target.stop();
  checks.push('portable backup excludes cached calendar and reminder state, resets opt-in, and retains explicitly prepared notes');

  await writeFile(fixture, calendar(Array.from({length: 300}, (_, i) => event(`bounded-${String(i).padStart(3, '0')}`, 10 + i))));
  assert.equal((await source.call('import_calendar', {path: fixture})).length, 300);
  const bounded = await source.call('get_meeting_day');
  assert.equal(bounded.events.length, 256); assert.equal(bounded.events[0].event.title, 'Fixture bounded-000');
  assert.equal(bounded.events.at(-1).event.title, 'Fixture bounded-255');
  await source.stop(); source = await host('source');
  assert.equal((await source.call('get_meeting_day')).events.length, 256);
  assert.equal((await source.call('bootstrap_library')).meetingPage.meetings.length, 2);
  await source.stop();
  checks.push('300-entry native ICS import stores only earliest256 and remains bounded across restart');
  const evidence = {verifiedAt: new Date().toISOString(), engineSha256: createHash('sha256').update(await readFile(engine)).digest('hex'), requests, checks, generatedCalendarOnly: true, disposablePreferenceSeedOnly: true, microphoneAccess: false, systemAudioAccess: false, calendarContentsAccess: false, notificationAuthorizationRequested: false, notificationDeliveryAttempted: false, externalProviderAccess: false};
  await mkdir('benchmarks/native-workflow', {recursive: true});
  await writeFile('benchmarks/native-workflow/meeting-day.json', JSON.stringify(evidence, null, 2) + '\n');
  console.log(JSON.stringify(evidence));
} finally {
  for (const host of hosts) if (host.child.exitCode === null) { host.child.kill(); await host.closed; }
  await rm(root, {recursive: true, force: true});
}
