// Explicit download of the preregistered public clips; no app/library integration.
import {createHash} from 'node:crypto';
import {mkdir, readFile, writeFile} from 'node:fs/promises';
import path from 'node:path';

const [destination, publicKennedyPCM] = process.argv.slice(2);
if (!destination || !path.isAbsolute(destination) || !publicKennedyPCM || !path.isAbsolute(publicKennedyPCM)) {
  throw new Error('Usage: node prepare-fixtures.mjs /absolute/isolated/fixtures /absolute/public-kennedy-16k.f32');
}
const experiment = JSON.parse(await readFile(new URL('./experiment.json', import.meta.url), 'utf8'));
async function boundedFetch(url, maximum) {
  const response = await fetch(url, {signal: AbortSignal.timeout(120_000)});
  if (!response.ok || !response.body) throw new Error(`Fixture HTTP ${response.status}`);
  const chunks = []; let size = 0;
  for await (const chunk of response.body) { size += chunk.length; if (size > maximum) throw new Error('Fixture exceeds size bound'); chunks.push(chunk); }
  return Buffer.concat(chunks, size);
}
await mkdir(destination, {recursive: true, mode: 0o700});
const rowsURL = 'https://datasets-server.huggingface.co/rows?dataset=argmaxinc%2Fcontextual-earnings22&config=default&split=test&offset=0&length=6';
const rows = JSON.parse((await boundedFetch(rowsURL, 65_536)).toString('utf8')).rows;
if (rows.length !== 6) throw new Error('The preregistered dataset rows changed');
for (let i = 0; i < 6; i++) {
  const fixture = experiment.fixtures[i], row = rows[i].row;
  if (row.file_id !== fixture.id || !row.audio[0].src.includes(`/${experiment.datasetRevision}/`)) throw new Error('Dataset revision/selection changed');
  const wav = await boundedFetch(row.audio[0].src, 1_000_000);
  if (createHash('sha256').update(wav).digest('hex') !== fixture.sha256) throw new Error('Public fixture checksum changed');
  // These pinned WAV fixtures use the canonical 44-byte PCM header.
  if (wav.toString('ascii',0,4) !== 'RIFF' || wav.toString('ascii',8,16) !== 'WAVEfmt ' || wav.readUInt16LE(20) !== 1 || wav.readUInt16LE(22) !== 1 || wav.readUInt32LE(24) !== 16_000 || wav.readUInt16LE(34) !== 16 || wav.toString('ascii',36,40) !== 'data' || wav.readUInt32LE(40) !== 480_000 || wav.length !== 480_044) throw new Error('Pinned WAV shape changed');
  const pcm = Buffer.alloc(240_000 * 4);
  for (let frame = 0; frame < 240_000; frame++) pcm.writeFloatLE(wav.readInt16LE(44 + frame * 2) / 32768, frame * 4);
  fixture.pcm = path.join(destination, `${fixture.id}.f32`);
  await writeFile(fixture.pcm, pcm, {mode: 0o600});
}
const kennedy = await readFile(publicKennedyPCM);
// The same explicitly provisioned public fixture used in the existing native ASR checks.
if (kennedy.length !== 61521 * 4 || createHash('sha256').update(kennedy).digest('hex') !== '01ce3dfe8a93bd58c9931e3a0d74c9c92330582455ff517f03e7547f5b510583') throw new Error('Expected the verified public Kennedy16k f32 fixture');
experiment.fixtures[6].pcm = publicKennedyPCM;
experiment.fixtures[7].pcm = path.join(destination,'silence.f32');
await writeFile(experiment.fixtures[7].pcm, Buffer.alloc(16_000 * 4), {mode: 0o600});
await writeFile(path.join(destination,'experiment.json'), JSON.stringify(experiment,null,2)+'\n', {mode: 0o600});
console.log(`Prepared six pinned public clips and controls: ${destination}/experiment.json`);
