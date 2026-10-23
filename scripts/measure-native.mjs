// Sustained PUBLIC PCM test. No microphone, system-audio device, account or cloud call.
import assert from 'node:assert/strict';
import {spawn, execFileSync} from 'node:child_process';
import {mkdtemp, mkdir, symlink, writeFile, readFile, chmod, rm, stat} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import path from 'node:path';
import {createInterface} from 'node:readline';
import {randomUUID, createHash} from 'node:crypto';
const args=process.argv.slice(2);
function option(name,fallback){const i=args.indexOf(name);if(i<0){if(fallback!==undefined)return fallback;throw new Error(`Missing ${name}`);}if(!args[i+1])throw new Error(`Missing value for ${name}`);return args[i+1];}
const engine=path.resolve(option('--engine')),models=path.resolve(option('--models')),pcm=path.resolve(option('--pcm')),speech=path.resolve(option('--speech'));
const seconds=Number(option('--seconds','180')),destination=path.resolve(option('--out','benchmarks/native-runtime/measurement.json'));
assert(process.platform==='darwin','The synthetic process source targets macOS.');
assert(Number.isFinite(seconds)&&seconds>=15&&seconds<=600,'Use a bounded 15–600 seconds.');
const library=await mkdtemp(path.join(tmpdir(),'tuck-sustained-'));
const helper=path.join(library,'public-pcm-source');
await symlink(models,path.join(library,'models'),'dir');
await writeFile(helper,`#!${process.execPath}
const fs=require('node:fs');const audio=fs.readFileSync(process.env.TUCK_TEST_PUBLIC_PCM);
const loop=Buffer.concat([audio,Buffer.alloc(16000*4)]);let position=0;
process.stderr.write('READY\\n');
const timer=setInterval(()=>{const block=Buffer.alloc(6400);for(let n=0;n<block.length;n++){block[n]=loop[position++];if(position===loop.length)position=0;}process.stdout.write(block);},100);
process.stdin.resume();process.stdin.on('end',()=>{clearInterval(timer);process.exit(0);});
`);
await chmod(helper,0o700);
const child=spawn(engine,['--data-dir',library],{env:{...process.env,TUCK_AUDIO_HELPER:helper,TUCK_SPEECH_HELPER:speech,TUCK_TEST_PUBLIC_PCM:pcm},stdio:['pipe','pipe','pipe']});
const pending=new Map(),events=[],samples=[],lags=[];let diagnostics='',invalidProtocol=false,recordingStart=null;
child.stderr.on('data',data=>{diagnostics=(diagnostics+data).slice(-4000);});
createInterface({input:child.stdout}).on('line',line=>{try{const message=JSON.parse(line);if(message.id&&pending.has(message.id)){const request=pending.get(message.id);pending.delete(message.id);clearTimeout(request.timer);message.error?request.reject(new Error(message.error.message)):request.resolve(message.result);}else if(message.event){events.push(message);if(message.event==='segment'&&recordingStart!==null)lags.push((performance.now()-recordingStart)/1000-message.payload.segment.end);}}catch{invalidProtocol=true;}});
const closed=new Promise(resolve=>child.once('close',code=>{for(const request of pending.values()){clearTimeout(request.timer);request.reject(new Error(`Runtime exited ${code}: ${diagnostics}`));}pending.clear();resolve(code);}));
function call(method,params={}){return new Promise((resolve,reject)=>{const id=randomUUID(),timer=setTimeout(()=>{pending.delete(id);reject(new Error(`${method} timed out`));},300000);pending.set(id,{resolve,reject,timer});child.stdin.write(JSON.stringify({id,method,params})+'\n');});}
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
function children(pid){try{return execFileSync('pgrep',['-P',String(pid)],{encoding:'utf8',stdio:['ignore','pipe','ignore']}).trim().split(/\s+/).map(Number).filter(Number.isFinite);}catch{return [];}}
function processStats(){const pids=[child.pid,...children(child.pid)];const text=execFileSync('ps',['-p',pids.join(','),'-o','pid=,ppid=,rss=,pcpu=,time=,comm='],{encoding:'utf8'});return text.trim().split('\n').filter(Boolean).map(line=>{const m=line.trim().match(/^(\d+)\s+(\d+)\s+(\d+)\s+([\d.]+)\s+(\S+)\s+(.+)$/);assert(m,`Unreadable process sample: ${line}`);return {pid:Number(m[1]),parent:Number(m[2]),rssBytes:Number(m[3])*1024,cpuPercent:Number(m[4]),cpuTime:m[5],kind:Number(m[1])===child.pid?'rust':path.basename(m[6])==='tuck-speech'?'coreml':'syntheticSource'};});}
function percentile(values,p){const sorted=[...values].sort((a,b)=>a-b);return sorted[Math.min(sorted.length-1,Math.floor((sorted.length-1)*p))];}
function cpuSeconds(value){const parts=value.split(':');assert(parts.length>=2&&parts.length<=3,'Unexpected ps TIME');let days=0;if(parts[0].includes('-')){const pair=parts[0].split('-');days=Number(pair[0]);parts[0]=pair[1];}return days*86400+parts.reduce((total,part)=>total*60+Number(part),0);}
function processSummary(kind){const observed=samples.flatMap(s=>s.processes.filter(p=>p.kind===kind).map(p=>({...p,elapsedSeconds:s.elapsedSeconds})));assert(observed.length>1);const first=observed[0],last=observed.at(-1);const cpuTimeDeltaSeconds=cpuSeconds(last.cpuTime)-cpuSeconds(first.cpuTime);return {rssFirstBytes:first.rssBytes,rssLastBytes:last.rssBytes,rssPeakBytes:Math.max(...observed.map(p=>p.rssBytes)),meanReportedPsCpuPercent:observed.reduce((a,p)=>a+p.cpuPercent,0)/observed.length,cpuTimeDeltaSeconds,cpuTimeDeltaPercentOfOneCore:cpuTimeDeltaSeconds/(last.elapsedSeconds-first.elapsedSeconds)*100};}
function hardware(){const query=(cmd,args)=>execFileSync(cmd,args,{encoding:'utf8'}).trim();return {model:query('/usr/sbin/sysctl',['-n','hw.model']),chip:query('/usr/sbin/sysctl',['-n','machdep.cpu.brand_string']),ramBytes:Number(query('/usr/sbin/sysctl',['-n','hw.memsize'])),os:query('/usr/bin/sw_vers',['-productVersion']),architecture:query('/usr/bin/uname',['-m'])};}
try{
  assert.deepEqual((await call('bootstrap')).meetings,[]);
  const idle=processStats();
  await call('update_settings',{changes:{keepAudio:true}});
  const meeting=await call('create_meeting',{title:'Public PCM sustained verification'});
  const startup=performance.now();
  await call('start_recording',{meetingId:meeting.id,systemAudio:true,microphone:false});
  recordingStart=performance.now();
  const startupSeconds=(recordingStart-startup)/1000;
  const file=path.join(library,'audio',meeting.id,'system.wav');
  async function sample(){samples.push({elapsedSeconds:(performance.now()-recordingStart)/1000,processes:processStats(),wavBytes:(await stat(file)).size,segments:events.filter(e=>e.event==='segment').length});}
  await sample();
  const deadline=performance.now()+seconds*1000;
  while(performance.now()<deadline){await sleep(Math.min(5000,deadline-performance.now()));await sample();assert(!events.some(e=>e.event==='engine_error'),JSON.stringify(events.filter(e=>e.event==='engine_error')));}
  const stopStart=performance.now();
  const finished=await call('stop_recording',{meetingId:meeting.id});
  const stopSeconds=(performance.now()-stopStart)/1000;
  assert.equal(finished.status,'complete');assert(finished.segments.length>=Math.floor(seconds/10));
  assert(finished.segments.filter(s=>s.text.toLowerCase().includes('country')).length>=Math.floor(seconds/10));
  assert.equal(lags.length,finished.segments.length,'Missing segment arrival observations');
  assert(!invalidProtocol,'Non-JSON stdout');
  assert(!events.some(e=>e.event==='engine_error'),'Engine reported a failure');
  assert.equal((await call('get_status')).activeMeetingId,null);
  const wavBytes=(await stat(file)).size;
  assert(wavBytes>seconds*32000*0.9,'Too little captured PCM for wall-clock duration');
  const afterStop=processStats();
  assert(!afterStop.some(p=>p.kind==='coreml'||p.kind==='syntheticSource'),'Capture/model child survived stop');
  await call('shutdown');assert.equal(await closed,0);
  const coreml=samples.map(s=>s.processes.find(p=>p.kind==='coreml')).filter(Boolean);
  assert(coreml.length>0,'No Core ML helper observed');
  const result={observedAt:new Date().toISOString(),hardware:hardware(),scope:'Native diagnostic runtime + shipped Core ML V2 helper; one synthetic system source from public PCM. No UI, actual microphone, system capture, video call, local LLM, or speaker model.',qualification16GB:false,requestedSeconds:seconds,actualRecordingSeconds:finished.duration,startupSeconds,stopSeconds,fixtureSha256:createHash('sha256').update(await readFile(pcm)).digest('hex'),engineSha256:createHash('sha256').update(await readFile(engine)).digest('hex'),speechHelperSha256:createHash('sha256').update(await readFile(speech)).digest('hex'),segments:finished.segments.length,expectedWordsVerified:true,errors:0,wavBytes,lagSeconds:{definition:'Segment arrival wall time minus segment audio end, relative to recording start response; includes VAD finalization, buffering and inference, with small start-response timing error.',count:lags.length,median:percentile(lags,.5),p95:percentile(lags,.95),max:Math.max(...lags),min:Math.min(...lags)},processes:{idle,afterStop,rust:processSummary('rust'),coreml:processSummary('coreml'),caveat:'RSS is per-process resident mappings and may double-count shared pages. Core ML/ANE/compiler-service allocations outside these processes are excluded; this is not total machine memory or energy.'},samples};
  await mkdir(path.dirname(destination),{recursive:true});await writeFile(destination,JSON.stringify(result,null,2)+'\n');
  console.log(JSON.stringify({...result,samples:undefined,processes:{...result.processes,idle:undefined,afterStop:undefined}}));
}finally{if(child.exitCode===null){child.kill();await closed;}await rm(library,{recursive:true,force:true});}
