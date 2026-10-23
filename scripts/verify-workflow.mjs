// Exercise the shipped Rust composition against an isolated local library.
// No microphone, calendar contents, provider account, model or external API is used.
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {mkdtemp, readFile, rm, mkdir, writeFile} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import path from 'node:path';
import {createInterface} from 'node:readline';
import {randomUUID, createHash} from 'node:crypto';

const engine = path.resolve(process.env.TUCK_RUNTIME_BIN || 'src-tauri/target/debug/tuck-runtime');
const library = await mkdtemp(path.join(tmpdir(),'tuck-workflow-'));
const checks = [];
let requests = 0;
function host() {
  const child = spawn(engine,['--data-dir',library],{stdio:['pipe','pipe','pipe']});
  const pending = new Map();
  let stderr = '';
  child.stderr.on('data',chunk=>{stderr=(stderr+chunk).slice(-2000);});
  const closed = new Promise(resolve=>child.once('close',code=>{
    for (const request of pending.values()) {clearTimeout(request.timer);request.reject(new Error(`Runtime exited ${code}: ${stderr}`));}
    pending.clear();resolve(code);
  }));
  createInterface({input:child.stdout}).on('line',line=>{
    const value = JSON.parse(line);
    const request = pending.get(value.id);
    if (!request) return;
    pending.delete(value.id);clearTimeout(request.timer);
    value.error ? request.reject(Object.assign(new Error(value.error.message),{code:value.error.code})) : request.resolve(value.result);
  });
  const call = (method,params={}) => new Promise((resolve,reject)=>{
    requests++;
    const id = randomUUID();
    const timer=setTimeout(()=>{pending.delete(id);reject(new Error(`${method} timed out`));},20000);
    pending.set(id,{resolve,reject,timer});child.stdin.write(JSON.stringify({id,method,params})+'\n');
  });
  return {child,closed,call};
}
let runtime;
try {
  runtime = host();
  const call = runtime.call;
  const empty = await call('bootstrap');
  assert.deepEqual(empty.meetings,[]);
  assert.deepEqual(empty.settings.notionPropertyMapping,{});
  assert.equal(empty.settings.microphoneDeviceId,'');
  const first = await call('create_meeting',{title:'Acme renewal review'});
  await call('update_meeting',{id:first.id,changes:{notes:'Acme agreed to a two-week pilot.',summary:'## Decision\nReview the pilot on Friday.',folder:'Customers',tags:['Renewal','Pilot'],people:['Morgan']}});
  let meeting = await call('add_task',{meetingId:first.id,title:'Send pilot plan',owner:'Morgan',dueDate:'2026-11-02'});
  const task = meeting.tasks[0];
  assert.equal(task.owner,'Morgan');
  const tasks = await call('list_tasks');
  assert.equal(tasks[0].meetingId,first.id);
  await call('update_task',{meetingId:first.id,id:task.id,changes:{done:true}});
  assert.equal((await call('list_tasks')).length,0);
  assert.equal((await call('list_tasks',{includeDone:true})).length,1);
  checks.push('meeting metadata and durable action lifecycle');

  const hits = await call('search_library',{query:'Acme',folder:'Customers',tags:['Pilot'],people:['Morgan']});
  assert(hits.length >= 1);
  assert(hits.every(hit=>hit.meetingId === first.id));
  assert(hits.every(hit=>hit.source !== 'transcript' || !!hit.segmentId));
  assert(hits.filter(hit=>hit.source !== 'transcript').every(hit=>!('start' in hit) && !('segmentId' in hit)));
  assert.equal((await call('search_library',{query:'Acme',folder:'Other'})).length,0);
  checks.push('bounded scoped evidence with genuine source types');

  const base = 'Acme agreed to a two-week pilot.';
  await call('apply_proposal',{meetingId:first.id,target:'notes',baseText:base,text:base+'\nMorgan owns the plan.'});
  await assert.rejects(call('apply_proposal',{meetingId:first.id,target:'notes',baseText:base,text:'Stale replacement'}),error=>error.code === 'conflict');
  assert((await call('note_history',{meetingId:first.id})).some(revision=>revision.text === base));
  checks.push('reviewed proposal, stale-edit rejection and prior-text history');

  const templates = await call('upsert_template',{template:{name:'Customer review',instructions:'Use sections: Outcome, Customer needs, Agreed next steps.'}});
  const template = templates.find(t=>t.name === 'Customer review');
  assert(template?.id);
  await call('update_meeting',{id:first.id,changes:{template:template.id}});
  const recipes = await call('upsert_recipe',{recipe:{name:'Follow-up draft',prompt:'Draft a short follow-up from the actual decisions. Do not send it.'}});
  assert(recipes.some(r=>r.name === 'Follow-up draft'));
  checks.push('custom templates and saved recipes');

  const event = {id:'verification:renewal',source:'verification',title:'Acme renewal review',start:'2026-11-02T14:00:00Z',end:'2026-11-02T14:30:00Z',joinUrl:'https://example.com/meeting',attendees:['Morgan']};
  await call('update_meeting',{id:first.id,changes:{calendarEvent:event}});
  assert.equal(typeof (await call('calendar_status')).connected,'boolean');
  const readiness = await call('capture_readiness');
  assert.equal(typeof readiness.modelReady,'boolean');
  checks.push('calendar metadata and non-recording native preflight');

  const exportPath = path.join(library,'meeting.json');
  await call('export_meeting',{meetingId:first.id,format:'json',path:exportPath});
  const exported = JSON.parse(await readFile(exportPath,'utf8'));
  assert.equal(exported.title,first.title);
  checks.push('native atomic JSON export');
  await call('shutdown');assert.equal(await runtime.closed,0);
  runtime = host();
  const restored = await runtime.call('bootstrap');
  meeting = restored.meetings.find(m=>m.id === first.id);
  assert.equal(meeting.folder,'Customers');assert.equal(meeting.tasks[0].done,true);
  assert.equal(meeting.template,template.id);assert.equal(meeting.calendarEvent.id,event.id);
  assert.equal(meeting.notes,base+'\nMorgan owns the plan.');
  assert((await runtime.call('note_history',{meetingId:first.id})).length >= 2);
  assert.equal(restored.settings.customTemplates.find(t=>t.id === template.id).name,template.name);
  checks.push('complete process restart persistence');
  await runtime.call('shutdown');assert.equal(await runtime.closed,0);
  const result = {verifiedAt:new Date().toISOString(),engineSha256:createHash('sha256').update(await readFile(engine)).digest('hex'),requests,checks,microphoneAccess:false,systemAudioAccess:false,calendarContentsAccess:false,externalProviderAccess:false};
  await mkdir('benchmarks/native-workflow',{recursive:true});
  await writeFile('benchmarks/native-workflow/workflow.json',JSON.stringify(result,null,2)+'\n');
  console.log(JSON.stringify(result));
} finally {
  if(runtime?.child.exitCode === null){runtime.child.kill();await runtime.closed;}
  await rm(library,{recursive:true,force:true});
}
