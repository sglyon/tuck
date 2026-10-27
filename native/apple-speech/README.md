# Apple Core ML speech adapter

This is Tuck's native Apple speech helper. Rust owns capture, bounded queues,
model installation and checksum verification, persistence, and lifecycle. The
helper owns one serialized FluidAudio `AsrManager`. It never downloads models.
It requires macOS 14+, Swift 6.2+ to build, and the 632,169,729-byte model set in
`models.json`. The fixed int8-linear `Encoder_v2.mlmodelc` is selected explicitly
with `.int8V2`; the original 6-bit encoder is not a fallback.

Build from the repository root:

```sh
node scripts/build-apple-speech.mjs
```

The build script clones FluidAudio v0.17.7 into a generated cache, verifies exact
commit `503b4bd1bbf7220882de39fe8ae6716aae4132da`, and applies only the tracked
manifest patch to omit the unused NeMo binary. ASR source remains unchanged.
The patch avoids SwiftPM's attempted download of an unused 83 MiB xcframework.
The built executable is copied to `src-tauri/binaries/tuck-speech`. No model
weights are fetched by the build. A developer can set `TUCK_FLUIDAUDIO_SOURCE`
to an existing exact checkout; the script rejects unexpected source changes.

During explicit model installation, the Rust owner verifies every file in
`models.json`, stages the final model directory, and runs:

```sh
src-tauri/binaries/tuck-speech --models /absolute/final/model/directory --prepare
```

This loads and optimizes Core ML models and exercises a silent prediction.
First-use optimization may take a few minutes. A `prepared` response means this
process succeeded; system cache eviction can still make a later load slower.
Prepare using the final model path. A successful preparation must precede the
owner marking an installation ready.

## Framed protocol

Start `tuck-speech --models /absolute/verified/model/directory`. The helper loads
local models and emits one stdout line:

```text
TUCK_SPEECH:{"event":"ready","engine":"coreml","computeUnits":"cpuAndNeuralEngine"}
```

Send a UTF-8 JSON header ending in one newline, immediately followed by binary
PCM bytes:

```text
{"id":1,"op":"transcribe","samples":61521}\n
<61521 little-endian Float32 samples, 16 kHz mono; no delimiter>
```

`id` is an unsigned 64-bit integer. `samples` must be 1–240,000 (at most 15
seconds). Samples must be finite and in [-1, 1]. Headers are bounded to 4,096
bytes. The reader shares one bounded buffer across headers and payloads and uses
POSIX `read`, so partial frames and multiple frames in a single write work.
Each request receives fresh decoder state. Turns shorter than 300 ms are padded
for the SDK's minimum length; response duration and token timestamps retain the
true turn bounds.

Response:

```text
TUCK_SPEECH:{"id":1,"text":"...","duration":3.8450625,"tokenTimings":[{"token":"...","startTime":0.08,"endTime":0.2,"confidence":0.9}]}
```

Errors use `{id,error}` when a request ID is known, or `{event:"error",error}`
for initialization/malformed framing. A failed inference or invalid payload ends
the worker after its error response; the Rust owner must preserve the recording
and report the failure. Diagnostics go to stderr without transcript or PCM.
Send `{"op":"shutdown"}\n` to exit normally, or close stdin. The owner should
also enforce deadlines and kill/wait during cancellation or shutdown.

## Explicit verification

With already verified models and the prepared public Float32 test fixture:

```sh
node native/apple-speech/verify.mjs /tmp/tuck-apple-speech/models \
  /tmp/tuck-verification/en-16k.f32
```

The integration verifier checks actual inference, partial/coalesced binary
framing, ordered requests, short turns, shutdown, bounded input, invalid floats,
truncated payloads, oversized headers, and missing-model failure. It does not
record audio or use accounts.

The original-encoder benchmark in `benchmarks/apple-speech` is separate from this
production model choice. See `docs/apple-speech-evaluation.md` for measured
results, conversion differences, known limits, and Apple compute-plan evidence.

## Saved-meeting refinement

Optional full-track diarization uses FluidAudio's offline Community-1 pipeline
with PLDA/VBx clustering rather than greedy clustering of independent VAD
embeddings. `diarization-models.json` pins the separately installed 21,599,417-byte
model set. Inference uses an explicit local-only loader; it does not call SDK
model-download APIs. Rust verifies every model hash before launching:

```sh
src-tauri/binaries/tuck-speech --diarize MODEL_DIRECTORY SAVED_WAV SOURCE
```

ASR keeps VAD context and reports word timings. Rust assigns words to full-track
speaker turns, aligns simultaneous channel phrases, and suppresses repeated
microphone spans while retaining unmatched boundaries and unique utterances.
Microphone labels are mapped through shared speech evidence rather than assumed
to be the local user. Uncertain speech stays unassigned. The app's **Saved audio
→ Refine transcript** command creates a derived meeting with original evidence
unchanged. Live labels remain provisional. This pass currently supports Apple
silicon, mono 16 kHz PCM tracks up to two hours, and the Apple ASR backend.

The diagnostics-only `tuck-audio-eval` binary invokes this same production pass
without a database, devices, or hosted AI. Use `scripts/evaluate-meeting.py` to
preserve a read-only private baseline and compare new candidate outputs. Meeting
audio, text, speaker vectors, and reference transcripts belong outside Git.

Continuous turns from one speaker are grouped across input-source changes;
segments assembled from both tracks retain `mixed` source provenance. Partial
echoes are aligned as local phrase runs, so ASR substitutions do not leave
interleaved duplicate words when an utterance misses the whole-phrase threshold.
A distinct voice inside an echoed span and unmatched phrase endings remain.

## Immediate text, delayed identity

Live meetings no longer load the legacy CAMPPlus extractor or greedily assign
speaker IDs to VAD chunks. ASR publishes text with `unassigned` speaker identity.
A separate cancellable worker analyzes at most five minutes of saved PCM context
and considers words only after 90 seconds of additional recorded context. Its
immutable UI projection preserves newer raw STT and never rewrites raw evidence.
Previously accepted labels survive when their audio leaves the rolling window.
Unknown or ambiguous voices remain unidentified.

Identity selection uses the aggregated context embedding, cosine similarity at
least 0.85, a 0.15 gap over the next stored profile, and at least 15 seconds of
context speech. Individual segmentation embeddings only verify that selected
identity (0.70 support threshold, 0.15 runner-up gap). Two independent windows
and 90% agreement are required. Live passes do not enroll voices. These are
conservative gates calibrated on two local recordings, not authentication or a
guarantee of a person's identity.

`voice-memory.json` in the local application data directory stores model-revision
bound, anonymous voice templates and user-confirmed names, written atomically
with mode 0600. No embeddings are sent to an AI provider or exposed in meeting
DTOs. Renaming a remembered voice updates its saved name across local meetings.
Settings can stop voice matching immediately or forget the templates without
removing transcript text or saved names. Templates are local to this Mac and
are not included in library backups.

After Stop, automatic refinement creates a derived meeting and keeps the
original. New voices are enrolled only from sufficient full-meeting evidence.
The final pass reuses the just-recorded word timings when available; a saved-file
replay after restart retranscribes locally. Dictation, opted-out meetings, and
meetings without retained audio skip this automation. Identification failures
cannot stop STT; final-pass failures preserve the original and allow retry.

The diagnostics evaluator accepts an explicit `--remember-voices` flag to test
persistent templates in its supplied data directory. Without that flag it never
writes voice memory. The ignored native test `evaluate_delayed_saved_recordings`
replays context checkpoints using only profiles learned from a separate earlier
recording; its `TUCK_DELAYED_*` paths must point to private evaluation directories.


Unidentified microphone words do not block echo suppression. A partial echo with
at least three exact, tightly timed anchors (including two substantive words)
can suppress duplicate microphone words without selecting a speaker identity.
Adjacent single-character ASR variants are considered only within that aligned
context. Unique interjections, unmatched boundaries, protected negation/pronoun contrasts,
and differing numeric values are retained. This prevents an unidentified echo
from alternating word-by-word with a named system speaker. It does not correct
the canonical track's recognition errors.

For a local replay with existing word timings, the ignored native test
`evaluate_saved_transcript_refinement` reads `TUCK_DELAYED_CASE`,
`TUCK_DELAYED_MODELS`, `TUCK_DELAYED_RAW` and a new `TUCK_DELAYED_OUTPUT`.
Optional `TUCK_REFINEMENT_VOICES` points to a voice-memory file copied into an
isolated temporary directory; the original is never modified. Audio, cached
transcripts, named profiles and replay output belong outside the repository.


## Brief-reply identity recovery

After the full saved-recording pass, an optional local pass revisits unidentified
system/combined paragraphs lasting 0.5–1.5 seconds (at most eight words, 64
paragraphs per meeting). It embeds exact audio at two slightly different
boundaries, tiling each span to fill the embedding model window. Padding cannot
borrow a recognized neighboring turn. Coarse ten-second chunk bounding boxes
are not evidence that a speaker spoke inside every gap in that interval.

Both system probes must select the same stored identity with cosine at least
0.45 and a 0.25 lead over every competing stored profile; that identity must also
be established by the full-meeting pass. At least two profiles are required.
Combined paragraphs additionally require both microphone probes to corroborate
that identity (0.40 minimum, same margin). Microphone-only passages and longer,
shorter, overlapping, ambiguous or failing probes stay unidentified. These
short-query thresholds were developed with one locally recorded meeting,
known-voice controls, profile-withholding negatives and a user-confirmed brief
reply. They are not probabilities or a general accuracy guarantee.

Whole-profile matching remains 0.85/0.15; live identification is unchanged.
The short pass never enrolls voices, updates templates, changes words, or
changes existing speaker assignments. It runs only during final refinement.
Native helper `--embed-spans MODEL_DIR WAV SOURCE` reads a bounded JSON array
of `{id,start,end}` from stdin and emits `TUCK_SPAN_EMBEDDINGS`. Rust enforces
output limits and a 45-second deadline per source. A failure leaves the existing
unidentified speech intact. Vectors stay in local process memory.
