# Acoustic vocabulary qualification

**Decision: keep this as a developer prototype. Do not enable production
vocabulary assistance yet.** Real acoustic rescoring corrected several target
names, but both configurations also inserted a name into ordinary speech.
The production helper, model installation, settings and CPU greedy decoder
were not changed by this experiment.

## Reproducible local path

The separate executable in `native/apple-speech/qualification` uses FluidAudio
commit `503b4bd1bbf7220882de39fe8ae6716aae4132da` (the same pinned source as
production), with only the existing manifest patch excluding unused NeMo.
It uses public APIs `CtcModels.loadDirect(from:)`, `CtcTokenizer.load(from:)`
and `VocabularyRescorer.create(ctcModelDirectory:)`. It does not use
`VocabularyBoostingSession`, `downloadAndLoad`, SDK default cache paths or
text-only substitution. Every term is explicitly CTC-tokenized before the
spotter is invoked. Token timings from the unmodified TDT result constrain
rescoring. The non-mutating candidate API is checked to return exactly the
original decoder text, and its raw scores and decisions are retained.

The optional CTC model is
[FluidInference/parakeet-ctc-110m-coreml](https://huggingface.co/FluidInference/parakeet-ctc-110m-coreml/tree/accdafd8cf8a2ff1cabe3c11e54416b405d409aa),
revision `accdafd8cf8a2ff1cabe3c11e54416b405d409aa`. The checked-in manifest
pins 14 encoder/mel/tokenizer/vocabulary files totaling **102,803,366 bytes**
(98.04 MiB), with a SHA256 and exact size for every file. The model card
declares CC-BY-4.0; weights are not redistributed in this repository or app.
Only the explicit developer fetch command downloads them, into the chosen
isolated directory. The runtime verifies both CTC and existing TDT assets
before loading. Core ML can prepare local execution artifacts; this is not
an SDK model download or an app-start installation.

The experiment limits vocabulary to 64 terms of 3–80 characters and at most
32 CTC tokens each. It limits each supplied PCM fixture to 15 seconds of
finite mono 16 kHz Float32 and at most 32 fixtures. One serial inference pass
runs at a time. These are prototype bounds, not a new production port.

## Test selection fixed before inference

The first six test rows from
[argmaxinc/contextual-earnings22](https://huggingface.co/datasets/argmaxinc/contextual-earnings22)
were selected without filtering or looking at ASR results. The dataset's
cached extraction revision is `59d7f94771cc197984d8038e494c83abeff87282`.
Every downloaded WAV has a pinned checksum in `qualification/experiment.json`.
No user audio, device, account or private library was used.

Reference text/vocabulary credit Argmax and Rev's Earnings-22 authors.
The exact Argmax dataset card follows the original dataset license; Rev's
Earnings-22 transcript/alignment-text notice is CC BY-SA 4.0. Copied and
adapted experiment data retain that license, with selection/scoring changes
identified in [DATA-NOTICE.md](../native/apple-speech/qualification/DATA-NOTICE.md).
The original audio is downloaded explicitly for local tests and is not
redistributed or bundled.

| Row / file | Reference targets |
| --- | --- |
| 0 / `4469291_chunk82` | Anna, Pryanova, Gazprom |
| 1 / `4452058_chunk143` | HepsiPay |
| 2 / `4480850_chunk135` | Ericsson |
| 3 / `4475604_chunk58` | Arqit |
| 4 / `4481601_chunk9` | Urbanimmersive, twice |
| 5 / `4426736_chunk1` | Andi, Setiawan |

All clips received the same nine target terms plus absent NVIDIA, Anthropic
and Yandex Pay distractors. The existing public Kennedy 3.845-second fixture
and one second of generated silence were additional no-target controls.
The same CTC probabilities were used for both preregistered configurations:
SDK defaults, and a conservative configuration with acoustic rescue disabled
and short-term context-weight taper pivot 5. Empty vocabulary was also tested
against every baseline. There was no parameter search or selection of a
successful retry after seeing results.

An initial harness asset guard rejected the known 594 MB TDT encoder before
inference because its per-file cap was only 200 MB. The guard was corrected
to 700 MB for that existing pinned asset. The single subsequent complete
measurement is retained in `qualification/evidence/results.jsonl` and
`run.log`; the measured executable SHA256 is in `summary.json`.

## Recognition results

| Baseline | Both rescoring configurations |
| --- | --- |
| Kupyanova | Pryanova |
| FCPA | FCPA — HepsiPay still missed |
| Eriksson | Ericsson |
| Archit's | Arqit — possessive suffix also removed |
| urban immersive | Urbanimmersive, both occurrences |
| Andy Setyawan | Andi Setiawan |
| hand the conference over | **Andi the conference over — false insertion** |

Exact canonical target spelling was present for **2/9 target/clip pairs**
before rescoring and **8/9** afterwards. This is a nine-pair smoke check,
not a general recall estimate. Case-folded alphanumeric word-error counting
over all eight references moved from 38/237 (16.03%) to 31/237 (13.08%).
That aggregate improvement does not excuse the false name insertion.

Both configurations applied `hand` → `Andi` in the sixth clip. The raw
candidate CTC score was -7.4927 versus -7.8151 for `hand`; even the
conservative configuration accepted it. Acoustic rescoring therefore is not
sufficient by itself to guarantee faithful evidence. The full candidate log
also shows rejected/overlapping alternatives; it is not only a list of
successful corrections. The public Kennedy and silence transcripts remained
unchanged in both configurations. Empty vocabulary returned byte-identical
baseline text on all eight clips, confirming term removal disables changes.
Spotter detections alone are not reliable transcript evidence: it emitted
low-score candidates for all 12 terms even on silence, while the rescorer
made no replacements there.

## Resource observations

On the development Mac used for the existing native checks (32 GiB, not the
16 GiB minimum target), six 15-second clips took 51.6–65.0 ms each for the
already loaded TDT model. The additional CTC spotting pass took
107.9–115.7 ms per 15-second clip (median 109.7 ms), excluding CTC loading.
Candidate evaluation plus legacy rescoring were both run for diagnostic
comparison; the JSONL reports their combined duration separately. Shorter
controls took 85.8 ms for Kennedy and 77.4 ms for silence.

The measured cold TDT load was **78.44 s**, followed by **30.91 s** to load
CTC/tokenizer/rescorers. Full wall time was 111.26 s; process user+system CPU
time was 4.43 s. These are one-run development observations, with OS model
preparation and accelerator services outside process CPU accounting. They
are not guaranteed prepared-start latency.

Process peak RSS reached **1,409,613,824 bytes (1.31 GiB)** during TDT
loading and did not rise above that prior peak after adding CTC. This does
**not** measure incremental steady-state memory or include all ANE service
memory. `time -l` reported peak process memory footprint of 872,056,224 bytes.
No active video call, sustained recording, energy or thermal workload was
tested. This does not qualify the 16 GiB meeting target.

## Portable CPU path

The pinned Sherpa 1.13.8 Rust API exposes per-stream hotwords. Its
[NeMo recognizer source](https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.8/sherpa-onnx/csrc/offline-recognizer-transducer-nemo-impl.h)
constructs a TDT-aware modified-beam decoder and context graph. The
[official hotword guide](https://k2-fsa.github.io/sherpa/onnx/hotwords/index.html)
requires the matching modeling unit and scored BPE vocabulary. Tuck's
installed CPU model currently includes `tokens.txt`, not that BPE vocabulary.
The pinned `EncodeHotwords` implementation can terminate the process for an
unsupported modeling unit; a missing BPE encoder is not a safe fallback.
CTC IDs from the Apple auxiliary model are not TDT IDs and cannot fill this gap.

No CPU beam/hotword switch or quality claim was added. The next CPU experiment
needs a pinned tokenizer/BPE vocabulary demonstrably matching the installed
TDT tokens, then greedy versus bounded-beam versus biased-beam comparisons
on fixed target/distractor/no-target audio, including empty outputs and false
insertions. That experiment should run outside the app process first.

## Reproduce and next gate

Verify the existing FluidAudio checkout and reviewed manifest-only patch using
the normal native setup instructions. With the already explicitly provisioned
TDT models and public Kennedy PCM, run:

```sh
node native/apple-speech/qualification/fetch-ctc-assets.mjs /tmp/tuck-ctc-models
node native/apple-speech/qualification/prepare-fixtures.mjs \
  /tmp/tuck-vocabulary-fixtures /absolute/public-kennedy-16k.f32
TUCK_FLUIDAUDIO_SOURCE=/absolute/verified-FluidAudio \
  swift build -c release --package-path native/apple-speech/qualification \
  --scratch-path /tmp/tuck-vocabulary-build
/usr/bin/time -l /tmp/tuck-vocabulary-build/release/qualify-vocabulary \
  /absolute/verified-TDT-directory /tmp/tuck-ctc-models \
  native/apple-speech/qualification/ctc-models.json \
  native/apple-speech/models.json /tmp/tuck-vocabulary-fixtures/experiment.json \
  > /tmp/vocabulary-results.jsonl 2> /tmp/vocabulary-run.log
```

The fixture preparation command checks the fixed dataset revision/IDs/hashes
and the known public Kennedy checksum before writing the experiment. It
rejects changed data rather than silently substituting a different test set.
The two Node scripts passed syntax checks; cached model verification and a
fresh fixture preparation completed successfully. The qualification Swift
executable compiled and completed all eight cases with three contexts each.

The original stdlib-only WER, exact-target and latency analysis is retained as
`qualification/analyze.py`. Reproduce the checked-in numeric results without
models, downloads or inference:

```sh
python3 native/apple-speech/qualification/analyze.py \
  --check native/apple-speech/qualification/evidence/summary.json
```

The analyzer verifies all preregistered cases and references are present,
calculates edit-distance word errors and exact term/clip pairs, counts extra
configured-term occurrences, and checks empty-context text preservation.
It reproduced every original count, rate and timing exactly. The regenerated
summary also explicitly records the failed false-insertion gate. File hashes
are omitted unless supplied with `--binary PATH`, `--source PATH` or
`--model-file PATH`; `--sdk-revision SHA` records only an explicitly supplied
revision. Those optional provenance values are not invented from missing local
files. `--check` compares deterministic analysis fields independently of these
optional provenance fields.

Before a production setting, address ordinary-word false replacements and
possessive/punctuation preservation, then evaluate a larger held-out corpus
with near-homophone distractors, repeated names, deleted terms, accents,
languages and short turns. Preserve immutable baseline ASR and make any
assisted revision distinguishable. Finally measure sustained latency and
whole-system memory alongside a call on the actual minimum hardware.
