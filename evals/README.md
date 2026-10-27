# Tuck evaluations

A separate **uv-managed Python project** using **DeepEval 4.3.2** and **omp for LLM inference**. Nothing here changes the installed app, reads its library, or loads audio/voice templates.

## Speaker-name dataset

`datasets/speaker_names/synthetic-v1.json` contains 24 native DeepEval `Golden` records:

- Eight cases with nine desired suggestions, including two distinct voices named Alex.
- Sixteen cases where the correct outcome is abstention.
- Expected speaker IDs, verbatim names, evidence strength, and minimum supporting segment citations.
- Version, category, synthetic provenance, rationale, and a `development` split in metadata.

These are the original invented examples from October 27, 2026, converted without changing the desired outcomes. Citations make the evidence requirements explicit. Categories include direct/repeated handoffs, introductions, interruptions, third-person mentions, quotations, corrections, ambiguity, shared names, unstable diarization, uncertain ASR names, existing confirmed names, and transcript prompt injection.

This is a hand-authored development set, **not a held-out benchmark or a measurement of real-person identity accuracy**. Context flags are experimental prerequisites; Tuck does not currently expose all of them. Unknown speech should remain unidentified. Names must never merge acoustic voices or automatically become confirmed cross-meeting identities.

Goldens store `input` and `expected_output` as JSON strings, as DeepEval expects. `additional_metadata` carries annotations. The runner passes **only the input** to inference, never expected answers, rationale, or evaluator metadata.

## Setup and offline regression checks

From this directory:

```sh
uv sync --locked
PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 uv run python -m pytest -q
uv run python -m speaker_names list

uv run python -m speaker_names replay \
  --predictions fixtures/speaker_names/sonnet-4-6-direct.json \
  --gated --output runs/direct-gated
```

The fixture contains the original sanitized Sonnet 4.6 output obtained through `omp`. It preserves the mistake in c15 rather than editing it into a perfect answer. The second fixture, `sonnet-4-6-verify.json`, contains the separate LLM verification pass. Those historical calls batched all 24 cases together; provenance records that limitation. No CLI session logs, authentication metadata, private meeting text, or voice vectors are included.

Run the same command **without `--gated`** to expose the original mistake. It should return exit code **1**, with 23 passing cases and one unwanted Sarah suggestion on c15. With the explicit checks enabled, both saved fixtures pass all 24 cases. A replay needs no `omp`, model call, or API key.

## Fresh inference through omp

```sh
# One synthetic case; new inference is per-case, not batched.
uv run python -m speaker_names infer \
  --case c01 --gated --output runs/omp-smoke

# Full dataset, one completion per case.
uv run python -m speaker_names infer \
  --model anthropic/claude-sonnet-4-6 \
  --gated --output runs/omp-direct

# Candidate extraction followed by a separate verification call per case.
uv run python -m speaker_names infer \
  --strategy cascade --gated --output runs/omp-cascade
```

`omp` must be installed and authenticated. Its existing authentication is used directly; no keys are read or copied by this runner. No Claude CLI, Gemma, OpenAI inference adapter, or LLM judge is used. The CLI disables built-in tools, LSP, extensions, rules, skills, title generation, and session persistence. Its configured MCP discovery may still add startup context/overhead; any actual tool-execution event makes the run fail. Calls run from empty temporary directories. Each call has a time limit; failures are surfaced rather than treated as abstention.

All 24 examples in the default dataset are synthetic. Adding a private real-meeting dataset and running `infer` would send its inputs through the selected omp provider; keep such datasets and results outside Git and obtain the required authorization before hosted processing.

## Metrics and output

DeepEval's `evaluate()` runs six deterministic `BaseMetric` implementations, each with threshold 1:

| Metric | Failure it detects |
| --- | --- |
| No incorrect names | A name/voice mapping absent from the expected suggestions. |
| Name coverage | Missing a desired mapping; always abstaining cannot pass. |
| Evidence grounding | Missing/nonexistent citations, a name absent from evidence, or omitted required supporting turns. |
| Evidence strength | Upgrading a tentative cue to an explicit identification, or another strength mismatch. |
| Abstention | Suggesting a name when the expected output is empty. |
| Naming policy | Naming an unstable/unassigned voice, overwriting a confirmed name, or accepting an uncertain ASR name. |

Malformed responses and conflicting/duplicate suggestions for one voice fail validation; they never silently become successful abstentions. Two different voices can share a name. `--gated` explicitly removes unsupported or prohibited proposals before scoring, and stores raw proposals alongside final output so that the transformation remains visible. Gates are experimental evaluation logic, not a shipped app feature.

A new `--output` directory is mandatory. It receives sanitized predictions, a run manifest with dataset/prompt hashes and inference usage, per-case counts, aggregate `summary.json`, and DeepEval's native test-run export. Existing directories are never overwritten. Exit 0 means every evaluated case passes every metric; exit 1 indicates a failed evaluation. Exceptions also fail the process. Runtime data, virtual environments, caches, and generated reports are ignored by Git.

DeepEval telemetry, automatic dotenv/legacy-key loading, and Confident AI upload credentials are disabled by this project's entry point. Scoring stays local and uses no hosted judge. `uv.lock` fixes all dependency versions; use `uv sync --locked` in automation.

When expanding the set, add versioned Goldens with human-reviewed expectations and distinguish development cases from genuinely held-out cases. Do not tune on a set and then label it held-out. For real recordings, assess ASR/diarization quality separately from conversational name inference.
