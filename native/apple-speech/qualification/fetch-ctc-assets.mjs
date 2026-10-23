// Explicit developer experiment only. Nothing imports this from app setup/startup.
import {createHash} from 'node:crypto';
import {createReadStream, createWriteStream} from 'node:fs';
import {lstat, mkdir, readFile, rename, rm} from 'node:fs/promises';
import {Readable, Transform} from 'node:stream';
import {pipeline} from 'node:stream/promises';
import path from 'node:path';

const destination = process.argv[2];
if (!destination || !path.isAbsolute(destination)) throw new Error('Usage: node fetch-ctc-assets.mjs /absolute/isolated/ctc-directory');
const manifest = JSON.parse(await readFile(new URL('./ctc-models.json', import.meta.url), 'utf8'));
async function valid(file, entry) {
  try {
    const stat = await lstat(file);
    if (!stat.isFile() || stat.isSymbolicLink() || stat.size !== entry.bytes) return false;
    const hash = createHash('sha256');
    for await (const bytes of createReadStream(file)) hash.update(bytes);
    return hash.digest('hex') === entry.sha256;
  } catch (error) { if (error.code === 'ENOENT') return false; throw error; }
}
await mkdir(destination, {recursive: true, mode: 0o700});
if ((await lstat(destination)).isSymbolicLink()) throw new Error('Choose an isolated real directory');
for (const entry of manifest.files) {
  if (entry.path.includes('..') || path.isAbsolute(entry.path)) throw new Error('Invalid pinned asset path');
  const target = path.join(destination, entry.path);
  if (await valid(target, entry)) { console.log(`Verified ${entry.path}`); continue; }
  await mkdir(path.dirname(target), {recursive: true, mode: 0o700});
  const temporary = `${target}.partial-${process.pid}`;
  let count = 0;
  const limit = new Transform({transform(bytes, _encoding, done) {
    count += bytes.length;
    done(count > entry.bytes ? new Error('Asset exceeds pinned size') : null, bytes);
  }});
  try {
    const url = `https://huggingface.co/${manifest.repo}/resolve/${manifest.revision}/${entry.path}`;
    const response = await fetch(url, {signal: AbortSignal.timeout(120_000)});
    if (!response.ok || !response.body) throw new Error(`Asset HTTP ${response.status}`);
    await pipeline(Readable.fromWeb(response.body), limit, createWriteStream(temporary, {flags: 'wx', mode: 0o600}));
    if (!await valid(temporary, entry)) throw new Error(`Asset checksum/size mismatch: ${entry.path}`);
    await rename(temporary, target);
    console.log(`Downloaded and verified ${entry.path}`);
  } finally { await rm(temporary, {force: true}); }
}
console.log('Optional experimental CTC assets verified. Production settings and models were not changed.');
