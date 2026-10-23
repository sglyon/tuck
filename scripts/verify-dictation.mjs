// Actual Rust composition and Parakeet, using a staged public WAV in temporary libraries.
// Live microphone-only enforcement is tested through the core fake capture port;
// this integration exercises real recovery/inference without opening audio devices.
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {mkdtemp,mkdir,symlink,readFile,writeFile,copyFile,rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import path from 'node:path';
import {createInterface} from 'node:readline';
import {randomUUID,createHash} from 'node:crypto';
const args=process.argv.slice(2);
const option=name=>{const i=args.indexOf(name);if(i<0||!args[i+1])throw new Error(`Missing ${name}`);return path.resolve(args[i+1]);};
const engine=option('--engine'),models=option('--models'),fixture=option('--fixture');
const root=await mkdtemp(path.join(tmpdir(),'tuck-dictation-'));
const hosts=[];let requests=0;
const hash=value=>createHash('sha256').update(value).digest('hex');
async function host(name) {
  const data=path.join(root,name);await mkdir(data,{recursive:true});
  try{await symlink(models,path.join(data,'models'),'dir');}catch(e){if(e.code!=='EEXIST')throw e;}
  const child=spawn(engine,['--data-dir',data],{stdio:['pipe','pipe','pipe']});
  const pending=new Map();let stderr='';
  child.stderr.on('data',chunk=>{stderr=(stderr+chunk).slice(-3000);});
  const closed=new Promise(resolve=>child.once('close',code=>{for(const p of pending.values()){clearTimeout(p.timer);p.reject(new Error(`Runtime exited ${code}: ${stderr}`));}pending.clear();resolve(code);}));
  child.once('error',error=>{for(const p of pending.values()){clearTimeout(p.timer);p.reject(error);}pending.clear();});
  createInterface({input:child.stdout}).on('line',line=>{const value=JSON.parse(line),p=pending.get(value.id);if(!p)return;pending.delete(value.id);clearTimeout(p.timer);value.error?p.reject(Object.assign(new Error(value.error.message),{code:value.error.code})):p.resolve(value.result);});
  const call=(method,params={})=>new Promise((resolve,reject)=>{requests++;const id=randomUUID(),timer=setTimeout(()=>{pending.delete(id);reject(new Error(`${method} timed out: ${stderr}`));},120000);pending.set(id,{resolve,reject,timer});child.stdin.write(JSON.stringify({id,method,params})+'\n');});
  const runtime={data,child,closed,call,async stop(){await call('shutdown');assert.equal(await closed,0);}};
  hosts.push(runtime);return runtime;
}
try {
  let source=await host('source');
  const initial=await source.call('bootstrap_library');assert.equal(initial.meetingPage.meetings.length,0);
  const voice=await source.call('create_dictation');assert.equal(voice.kind,'dictation');assert.equal(voice.status,'draft');
  const folder=path.join(source.data,'audio',voice.id);await mkdir(folder,{recursive:true});
  const staged=path.join(folder,'microphone.wav');await copyFile(fixture,staged);
  const original=hash(await readFile(staged));
  // No meeting automation may execute for recovery, even when opted in globally.
  await source.call('update_settings',{changes:{autoSummary:true,notionAutoSync:true,notionTarget:'00000000-0000-4000-8000-000000000001'}});
  const recovered=await source.call('recover_audio',{meetingId:voice.id,audioId:'microphone.wav'});
  assert.equal(recovered.kind,'dictation');assert.equal(recovered.status,'complete');assert.equal(recovered.recoveredFrom,voice.id);
  assert(recovered.segments.length>0);assert(recovered.notes.toLowerCase().includes('country'));
  assert.equal(recovered.summary,'');assert.equal(recovered.notionUrl,'');assert.equal(recovered.notionStatus,'idle');
  assert.equal(hash(await readFile(staged)),original);
  const rawSpeech=recovered.segments.map(s=>s.text);
  await source.call('update_meeting',{id:recovered.id,changes:{notes:'Edited voice note, ready to copy.'}});
  const edited=await source.call('get_meeting',{meetingId:recovered.id});
  assert.equal(edited.notes,'Edited voice note, ready to copy.');assert.deepEqual(edited.segments.map(s=>s.text),rawSpeech);
  const selection={recap:false,notes:true,tasks:false};
  await source.call('update_settings',{changes:{notionContent:selection}});
  const backup=path.join(root,'library.zip');await source.call('backup_library',{path:backup,includeAudio:true});
  await source.stop();source=await host('source');
  const startup=await source.call('bootstrap_library');
  assert(startup.meetingPage.meetings.every(m=>m.kind==='dictation'));
  assert.deepEqual(startup.settings.notionContent,selection);
  assert.equal((await source.call('get_meeting',{meetingId:recovered.id})).notes,edited.notes);
  const target=await host('target');
  const preview=await target.call('preview_restore_library',{path:backup});
  await target.call('restore_library',{path:backup,expectedFingerprint:preview.fingerprint});
  const restored=await target.call('get_meeting',{meetingId:recovered.id});
  assert.equal(restored.kind,'dictation');assert.equal(restored.notes,edited.notes);assert.deepEqual(restored.segments.map(s=>s.text),rawSpeech);
  const restoredSettings=await target.call('get_settings');
  assert.equal(restoredSettings.notionAutoSync,false);assert.equal(restoredSettings.autoSummary,false);
  assert.deepEqual(restoredSettings.notionContent,selection);
  assert.equal(hash(await readFile(path.join(target.data,'audio',voice.id,'microphone.wav'))),original);
  await source.stop();await target.stop();
  const evidence={verifiedAt:new Date().toISOString(),engineSha256:hash(await readFile(engine)),requests,realParakeet:true,publicAudioFixtureSha256:original,checks:['dictation identity survives header/detail startup and backup merge','real Parakeet recovery creates editable text from public speech without AI or publishing','manual edits retain immutable source recognition and original audio bytes','publishing content choices persist; restored automatic processing stays off'],microphoneAccess:false,systemAudioAccess:false,externalProviderAccess:false,personalLibraryAccess:false,liveDictationHardwareQualified:false};
  await mkdir('benchmarks/native-workflow',{recursive:true});await writeFile('benchmarks/native-workflow/dictation.json',JSON.stringify(evidence,null,2)+'\n');
  console.log(JSON.stringify(evidence));
} finally {
  for(const runtime of hosts)if(runtime.child.exitCode===null){runtime.child.kill();await runtime.closed;}
  await rm(root,{recursive:true,force:true});
}
