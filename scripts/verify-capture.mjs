// Native pipeline integration check with PUBLIC PCM and a synthetic system-audio
// helper. This never opens a microphone or captures the user's computer audio.
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {mkdtemp, mkdir, symlink, writeFile, readFile, chmod, rm, stat} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import path from 'node:path';
import {createInterface} from 'node:readline';
import {randomUUID} from 'node:crypto';

const args=process.argv.slice(2);
const option=name=>{const value=args[args.indexOf(name)+1];if(!args.includes(name)||!value)throw new Error(`Missing ${name}`);return path.resolve(value);};
const engine=option('--engine'),models=option('--models'),pcm=option('--pcm');
if(process.platform!=='darwin')throw new Error('This synthetic helper test currently targets the macOS process-audio adapter.');
const library=await mkdtemp(path.join(tmpdir(),'tuck-capture-check-'));
const helper=path.join(library,'public-audio-source'),mode=path.join(library,'source-mode');
await symlink(models,path.join(library,'models'),'dir');
await writeFile(mode,'play');
await writeFile(helper,`#!${process.execPath}
const fs=require('node:fs');
process.stderr.write('READY\\n');
if(fs.readFileSync(process.env.TUCK_TEST_SOURCE_MODE,'utf8')==='exit')process.exit(0);
const audio=fs.readFileSync(process.env.TUCK_TEST_PUBLIC_PCM);
const silence=Buffer.alloc(16000*4);
const loop=Buffer.concat([audio,silence]);let position=0;
const timer=setInterval(()=>{let block=Buffer.alloc(6400);for(let n=0;n<block.length;n++){block[n]=loop[position++];if(position===loop.length)position=0;}process.stdout.write(block);},100);
process.stdin.resume();process.stdin.on('end',()=>{clearInterval(timer);process.exit(0);});
`);
await chmod(helper,0o700);
const child=spawn(engine,['--data-dir',library],{env:{...process.env,TUCK_AUDIO_HELPER:helper,TUCK_TEST_SOURCE_MODE:mode,TUCK_TEST_PUBLIC_PCM:pcm},stdio:['pipe','pipe','pipe']});
const pending=new Map(),events=[];let diagnostics='';
child.stderr.on('data',chunk=>{diagnostics=(diagnostics+chunk).slice(-4000);});
createInterface({input:child.stdout}).on('line',line=>{
  try{const message=JSON.parse(line);if(message.id&&pending.has(message.id)){const request=pending.get(message.id);pending.delete(message.id);clearTimeout(request.timer);message.error?request.reject(new Error(message.error.message)):request.resolve(message.result);}else if(message.event)events.push(message);}
  catch{diagnostics+='\nInvalid native protocol output';}
});
const closed=new Promise(resolve=>child.once('close',code=>{for(const request of pending.values()){clearTimeout(request.timer);request.reject(new Error(`Native runtime exited (${code}): ${diagnostics}`));}pending.clear();resolve(code);}));
function call(method,params={}) {return new Promise((resolve,reject)=>{const id=randomUUID();const timer=setTimeout(()=>{pending.delete(id);reject(new Error(`${method} timed out: ${diagnostics}`));},300000);pending.set(id,{resolve,reject,timer});child.stdin.write(JSON.stringify({id,method,params})+'\n');});}
const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function until(predicate,timeout=20000){const deadline=Date.now()+timeout;while(Date.now()<deadline){if(await predicate())return;await pause(50);}throw new Error('Capture state did not settle: '+diagnostics);}
try {
  assert.deepEqual((await call('bootstrap')).meetings,[]);
  await call('update_settings',{changes:{keepAudio:true}});
  const meeting=await call('create_meeting',{title:'Public PCM lifecycle verification'});
  await call('start_recording',{meetingId:meeting.id,systemAudio:true,microphone:false});
  await until(()=>events.some(e=>e.event==='segment'&&e.payload.meetingId===meeting.id),30000);
  await call('pause_recording',{meetingId:meeting.id});
  const file=path.join(library,'audio',meeting.id,'system.wav');
  const pausedSize=(await stat(file)).size;
  await pause(600);
  assert.equal((await stat(file)).size,pausedSize,'Paused capture must stop writing audio');
  const segmentCount=events.filter(e=>e.event==='segment'&&e.payload.meetingId===meeting.id).length;
  await call('resume_recording',{meetingId:meeting.id});
  await until(()=>events.filter(e=>e.event==='segment'&&e.payload.meetingId===meeting.id).length>segmentCount,30000);
  const finished=await call('stop_recording',{meetingId:meeting.id});
  assert.equal(finished.status,'complete');
  assert(finished.segments.length>=2);
  assert(finished.segments.every(s=>s.end>s.start));
  assert(finished.segments.some(s=>s.text.toLowerCase().includes('country')));
  const wav=await readFile(file);assert.equal(wav.subarray(0,4).toString(),'RIFF');assert(wav.length>pausedSize);
  assert.equal((await call('get_status')).activeMeetingId,null);

  // READY followed by immediate EOF must never leave a false live recording.
  await writeFile(mode,'exit');
  const failed=await call('create_meeting',{title:'Synthetic source failure'});
  try {await call('start_recording',{meetingId:failed.id,systemAudio:true,microphone:false});} catch { /* Expected failure can arrive before activation. */ }
  await until(async()=>{const state=await call('bootstrap');const item=state.meetings.find(m=>m.id===failed.id);return state.status.activeMeetingId===null&&['draft','interrupted'].includes(item.status);});
  await writeFile(mode,'play');
  const recovered=await call('create_meeting',{title:'Recording after source failure'});
  await call('start_recording',{meetingId:recovered.id,systemAudio:true,microphone:false});
  await call('stop_recording',{meetingId:recovered.id});
  await call('shutdown');assert.equal(await closed,0);
  console.log(JSON.stringify({syntheticNativeCapture:'passed',realParakeet:true,pauseResumeDrain:true,ownedWav:true,startupSourceFailureRecovery:true,microphoneAccess:false,systemAudioAccess:false}));
} finally {
  if(child.exitCode===null){child.kill();await closed;}
  await rm(library,{recursive:true,force:true});
}
