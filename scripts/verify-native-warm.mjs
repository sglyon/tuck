// Verify explicit canonical preparation followed by two new temporary model aliases.
// Public synthetic PCM only; no user audio, accounts or model download.
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {mkdtemp, mkdir, symlink, readFile, writeFile, realpath, rm} from 'node:fs/promises';
import path from 'node:path';
import {tmpdir} from 'node:os';
import {createHash} from 'node:crypto';
const args=process.argv.slice(2);
function option(name,fallback){const i=args.indexOf(name);if(i<0){if(fallback!==undefined)return fallback;throw new Error(`Missing ${name}`);}assert(args[i+1],`Missing value for ${name}`);return args[i+1];}
const engine=path.resolve(option('--engine')),models=path.resolve(option('--models')),pcm=path.resolve(option('--pcm')),speech=path.resolve(option('--speech')),out=path.resolve(option('--out','benchmarks/native-runtime/warm-start.json'));
const canonical=await realpath(path.join(models,'parakeet-v3-coreml'));
const root=await mkdtemp(path.join(tmpdir(),'tuck-warm-aliases-'));
const hash=async file=>createHash('sha256').update(await readFile(file)).digest('hex');
const engineSha256=await hash(engine),speechHelperSha256=await hash(speech);
function run(command,arguments_,timeout=300000){return new Promise((resolve,reject)=>{const child=spawn(command,arguments_,{stdio:['ignore','pipe','pipe']});let output='',errors='',settled=false;const timer=setTimeout(()=>{child.kill('SIGKILL');reject(new Error('Native warm-start check timed out'));settled=true;},timeout);child.stdout.on('data',data=>{output=(output+data).slice(-200000);});child.stderr.on('data',data=>{errors=(errors+data).slice(-4000);});child.on('error',error=>{clearTimeout(timer);if(!settled)reject(error);});child.on('close',code=>{clearTimeout(timer);if(settled)return;code===0?resolve({output,errors}):reject(new Error(`Process failed (${code}): ${errors} ${output}`));});});}
try{
  const prepareStart=performance.now();
  const prepared=await run(speech,['--models',canonical,'--prepare']);
  const prepareSeconds=(performance.now()-prepareStart)/1000;
  const status=prepared.output.trim().split('\n').filter(line=>line.startsWith('TUCK_SPEECH:')).map(line=>JSON.parse(line.slice('TUCK_SPEECH:'.length)));
  assert(status.some(item=>item.event==='prepared'),'Canonical preparation did not confirm success');
  console.log(JSON.stringify({event:'canonical_prepared',prepareSeconds}));
  const cases=[];
  for(const name of ['first','second']){
    const alias=path.join(root,name);await symlink(models,alias,'dir');
    const physicalModelDirectory=await realpath(path.join(alias,'parakeet-v3-coreml'));
    assert.equal(physicalModelDirectory,canonical);
    const measurement=path.join(root,`${name}.json`);
    await run(process.execPath,['scripts/measure-native.mjs','--engine',engine,'--models',alias,'--pcm',pcm,'--speech',speech,'--seconds','15','--out',measurement]);
    const data=JSON.parse(await readFile(measurement));
    assert(data.expectedWordsVerified&&data.errors===0&&data.segments>=1);
    assert.equal(data.engineSha256,engineSha256,'Runtime changed while checking aliases');
    assert.equal(data.speechHelperSha256,speechHelperSha256,'Speech helper changed while checking aliases');
    cases.push({alias,physicalModelDirectory,hardware:data.hardware,startupSeconds:data.startupSeconds,actualRecordingSeconds:data.actualRecordingSeconds,segments:data.segments,expectedWordsVerified:data.expectedWordsVerified,errors:data.errors,stopSeconds:data.stopSeconds,lagSeconds:data.lagSeconds});
    console.log(JSON.stringify({event:'alias_verified',name,startupSeconds:data.startupSeconds,segments:data.segments}));
  }
  const result={observedAt:new Date().toISOString(),hardware:cases[0].hardware,engineSha256,speechHelperSha256,canonicalModelDirectory:canonical,prepareSeconds,aliasCases:cases,samePhysicalModels:true,warmStartupUnderTenSeconds:cases.every(c=>c.startupSeconds<10),qualification16GB:false,caveat:'Two observed prepared starts through new aliases, not a guarantee of persistent OS caches. Public synthetic system source only; no actual video call, user audio, GUI, speaker model or local LLM.'};
  await mkdir(path.dirname(out),{recursive:true});await writeFile(out,JSON.stringify(result,null,2)+'\n');console.log(JSON.stringify(result));
}finally{await rm(root,{recursive:true,force:true});}
