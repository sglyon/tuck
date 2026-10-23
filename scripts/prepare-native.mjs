import {spawnSync} from 'node:child_process';
import {copyFileSync, mkdirSync, writeFileSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import path from 'node:path';

const root=fileURLToPath(new URL('../',import.meta.url));
const staged=path.join(root,'src-tauri/binaries/native');
mkdirSync(staged,{recursive:true});
writeFileSync(path.join(staged,'README.txt'),'Tuck native helpers. Speech models are installed separately with explicit user action.\n');
function run(command,args) {
  const result=spawnSync(command,args,{cwd:root,stdio:'inherit'});
  if(result.status!==0)process.exit(result.status??1);
}
if(process.platform==='darwin') {
  mkdirSync(path.join(root,'src-tauri/target/swift-module-cache'),{recursive:true});
  run('swiftc',['-O','-parse-as-library','-target',`${process.arch==='arm64'?'arm64':'x86_64'}-apple-macosx14.2`,'-module-cache-path','src-tauri/target/swift-module-cache',
    'native/macos/AudioCapture.swift','-o','src-tauri/binaries/tuck-audio',
    '-framework','ScreenCaptureKit','-framework','AVFoundation']);
  run('swiftc',['-O','-parse-as-library','-target',`${process.arch==='arm64'?'arm64':'x86_64'}-apple-macosx14.2`,'-module-cache-path','src-tauri/target/swift-module-cache',
    'native/macos/Calendar.swift','-o','src-tauri/binaries/tuck-calendar',
    '-framework','EventKit','-framework','AVFoundation','-framework','CoreGraphics']);
  run(process.execPath,['scripts/build-apple-speech.mjs']);
  for(const name of ['tuck-audio','tuck-speech','tuck-calendar'])copyFileSync(path.join(root,'src-tauri/binaries',name),path.join(staged,name));
}
console.log('Native resources prepared. The desktop backend is compiled by Cargo; no Python runtime is packaged.');
