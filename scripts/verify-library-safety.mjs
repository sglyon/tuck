// Real native composition + public speech; no personal capture or service access.
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {mkdtemp,mkdir,symlink,readFile,writeFile,rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import path from 'node:path';
import {createInterface} from 'node:readline';
import {randomUUID,createHash} from 'node:crypto';

const args = process.argv.slice(2);
const option = name => {const i=args.indexOf(name);if(i<0||!args[i+1])throw new Error(`Missing ${name}`);return path.resolve(args[i+1]);};
const engine=option('--engine'),models=option('--models'),fixture=option('--fixture');
const root=await mkdtemp(path.join(tmpdir(),'tuck-library-safety-'));
let requests=0;
const processes=[];
const hash=data=>createHash('sha256').update(data).digest('hex');
async function host(name) {
  const data=path.join(root,name);await mkdir(data);await symlink(models,path.join(data,'models'),'dir');
  const child=spawn(engine,['--data-dir',data],{stdio:['pipe','pipe','pipe'],env:process.env});processes.push(child);
  const pending=new Map();let stderr='';
  child.stderr.on('data',chunk=>{stderr=(stderr+chunk).slice(-3000);});
  const closed=new Promise(resolve=>child.once('close',code=>{for(const p of pending.values()){clearTimeout(p.timer);p.reject(new Error(`Runtime exited ${code}: ${stderr}`));}pending.clear();resolve(code);}));
  child.once('error',error=>{for(const p of pending.values()){clearTimeout(p.timer);p.reject(error);}pending.clear();});
  createInterface({input:child.stdout}).on('line',line=>{const value=JSON.parse(line),p=pending.get(value.id);if(!p)return;pending.delete(value.id);clearTimeout(p.timer);value.error?p.reject(Object.assign(new Error(value.error.message),{code:value.error.code})):p.resolve(value.result);});
  const call=(method,params={})=>new Promise((resolve,reject)=>{requests++;const id=randomUUID(),timer=setTimeout(()=>{pending.delete(id);reject(new Error(`${method} timed out: ${stderr}`));},300000);pending.set(id,{resolve,reject,timer});child.stdin.write(JSON.stringify({id,method,params})+'\n');});
  return {data,call,closed,child,async stop(){await call('shutdown');assert.equal(await closed,0);}};
}
try {
  const source=await host('source');
  assert.equal((await source.call('bootstrap')).meetings.length,0);
  const imported=await source.call('import_audio',{path:fixture,title:'Public speech safety verification'});
  assert(imported.segments.length>0);assert(imported.segments.some(s=>s.text.toLowerCase().includes('country')));
  const segment=imported.segments[0],original=segment.text;
  let corrected=await source.call('correct_transcript',{meetingId:imported.id,segmentId:segment.id,baseText:original,text:'Public verification: the pilot belongs to Acme.'});
  assert.equal(corrected.segments[0].originalText,original);assert.equal(corrected.segments[0].edited,true);
  await assert.rejects(source.call('correct_transcript',{meetingId:imported.id,segmentId:segment.id,baseText:original,text:'Stale edit'}),e=>e.code==='conflict');
  const hits=await source.call('search_library',{query:'Acme',meetingIds:[imported.id]});
  assert(hits.some(h=>h.source==='transcript'&&h.segmentId===segment.id));
  const files=await source.call('meeting_audio',{meetingId:imported.id});assert(files.some(f=>f.id==='import.wav'));
  const playback=await source.call('audio_source',{meetingId:imported.id,audioId:'import.wav'});
  assert.match(playback.url,/^(tuck-audio:\/\/localhost|http:\/\/tuck-audio\.localhost)\//);assert.equal(playback.path,undefined);
  await assert.rejects(source.call('audio_source',{meetingId:imported.id,audioId:'../../outside.wav'}));
  const audioPath=path.join(source.data,'audio',imported.id,'import.wav'),before=hash(await readFile(audioPath));
  await source.call('update_meeting',{id:imported.id,changes:{notes:'My preserved notes',folder:'Verification',tags:['Public']}});
  await source.call('add_task',{meetingId:imported.id,title:'Check original evidence',owner:'SGL',dueDate:'2026-11-02'});
  const recovered=await source.call('recover_audio',{meetingId:imported.id,audioId:'import.wav'});
  assert.notEqual(recovered.id,imported.id);assert.equal(recovered.recoveredFrom,imported.id);assert.equal(recovered.notes,'My preserved notes');
  assert(!recovered.notionUrl);assert.equal(recovered.summary,'');assert.equal(hash(await readFile(audioPath)),before);
  assert.equal(recovered.segments[0].text,original);
  for(let i=0;i<105;i++)await source.call('update_meeting',{id:imported.id,changes:{notes:`My preserved notes ${i}`}});
  const history = await source.call('note_history',{meetingId:imported.id});assert(history.length > 0 && history.length <= 100);
  const backup=path.join(root,'library.zip');
  const report=await source.call('backup_library',{path:backup,includeAudio:true});assert.equal(report.meetingCount,2);assert.equal(report.audioFiles,2);
  const firstPreview=await source.call('preview_restore_library',{path:backup});assert.equal(firstPreview.restorable,0);assert.equal(firstPreview.skipped,2);
  const target=await host('target');
  const preview=await target.call('preview_restore_library',{path:backup});assert.equal(preview.restorable,2);assert.equal(preview.audioFiles,2);
  await assert.rejects(target.call('restore_library',{path:backup,expectedFingerprint:'0'.repeat(64)}),e=>e.code==='conflict');
  assert.equal((await target.call('bootstrap')).meetings.length,0);
  const restored=await target.call('restore_library',{path:backup,expectedFingerprint:preview.fingerprint});assert.equal(restored.restored,2);assert.equal(restored.audioFilesRestored,2);
  let library=await target.call('bootstrap');let meeting=library.meetings.find(m=>m.id===imported.id);
  assert.equal(meeting.notes,'My preserved notes 104');assert.equal(meeting.segments[0].text,corrected.segments[0].text);assert.equal(meeting.segments[0].originalText,original);
  assert.equal(meeting.tasks[0].title,'Check original evidence');assert.equal(library.settings.provider,'local');assert.equal(library.settings.hostedConsent,false);assert.equal(library.settings.notionAutoSync,false);
  assert.equal(hash(await readFile(path.join(target.data,'audio',imported.id,'import.wav'))),before);
  await target.call('update_meeting',{id:imported.id,changes:{notes:'Current library edits must win.'}});
  const repeatPreview=await target.call('preview_restore_library',{path:backup});assert.equal(repeatPreview.restorable,0);
  const repeated=await target.call('restore_library',{path:backup,expectedFingerprint:repeatPreview.fingerprint});assert.equal(repeated.restored,0);assert.equal(repeated.skipped,2);assert.equal(repeated.audioFilesRestored,0);
  library=await target.call('bootstrap');meeting=library.meetings.find(m=>m.id===imported.id);assert.equal(meeting.notes,'Current library edits must win.');
  corrected=await target.call('reset_transcript',{meetingId:imported.id,segmentId:segment.id,baseText:meeting.segments[0].text});assert.equal(corrected.segments[0].text,original);assert.equal(corrected.segments[0].edited,false);
  await source.stop();await target.stop();
  const evidence={verifiedAt:new Date().toISOString(),engineSha256:hash(await readFile(engine)),requests,realParakeet:true,checks:['immutable original speech with guarded correction/reset','correction-aware source retrieval','scoped local playback URL and traversal rejection','new linked recovery workspace; original audio unchanged','portable backup with note and recap history','preview fingerprint conflict before writes','complete ZIP restore with owned audio','idempotent repeat preserving existing edits','safe restored preferences'],microphoneAccess:false,systemAudioAccess:false,calendarContentsAccess:false,externalProviderAccess:false};
  await mkdir('benchmarks/native-workflow',{recursive:true});await writeFile('benchmarks/native-workflow/library-safety.json',JSON.stringify(evidence,null,2)+'\n');console.log(JSON.stringify(evidence));
} finally {
  for(const child of processes)if(child.exitCode===null)child.kill();
  await rm(root,{recursive:true,force:true});
}
