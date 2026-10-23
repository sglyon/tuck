# Vocabulary support decision

Custom names and terminology remain a requested product feature. A text box alone would not deliver it: the selected recognition path must actually consume the terms, and corrected text must retain its original speech evidence.

## Apple Parakeet

The pinned FluidAudio source includes a real acoustic vocabulary pipeline: a separate Parakeet CTC 110M pass finds candidate terms, then timestamp-constrained rescoring evaluates changes to the TDT transcript. This is different from blind text substitution or asking an LLM to guess a name. The [upstream vocabulary guide](https://github.com/FluidInference/FluidAudio/blob/main/Documentation/ASR/CustomVocabulary.md) describes an additional roughly 97.5 MB encoder for Parakeet 0.6B v2/v3. Its speed, recall and memory numbers are upstream measurements, not Tuck measurements.

The current helper deliberately loads only explicitly installed local files. `VocabularyBoostingSession` in pinned commit `503b4bd1bbf7220882de39fe8ae6716aae4132da` derives its tokenizer/rescorer directory from the SDK default cache, even when CTC models were loaded elsewhere. It cannot simply be enabled against Tuck's model directory and assumed to preserve that offline boundary. A small adapter using the public spotter/rescorer APIs with explicit local paths is a viable candidate; it still needs pinned/checksummed CTC assets, explicit installation, bounded term validation and real speech tests.

## Portable CPU Parakeet

Sherpa's [hotword guide](https://k2-fsa.github.io/sherpa/onnx/hotwords/index.html) describes contextual biasing for transducers. That general capability does not qualify every TDT configuration. The [v1.13.8 NeMo implementation](https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.8/sherpa-onnx/csrc/offline-recognizer-transducer-nemo-impl.h) has a modified-beam path, while [issue 3267](https://github.com/k2-fsa/sherpa-onnx/issues/3267) reports hallucination/empty-result failures in an older TDT configuration. An issue report is evidence of a test case to reproduce, not proof that the pinned runtime still fails or that every beam configuration is unsafe.

The production CPU path keeps its measured greedy decoder until term tokenization and current-runtime quality/resource behavior are checked. Enabling a generic hotword flag without the matching tokenizer/model configuration would be a false feature.

## Acceptance before enabling

- Explicit user-managed names/terms, bounded length/count and local-only model installation.
- No hidden SDK download or cache fallback; verified immutable assets and the same prepared physical directory used at runtime.
- Preserve original ASR text and distinguish any vocabulary-assisted revision.
- Public or consented speech containing target names plus distractor/no-target turns; check false insertions, unchanged ordinary speech, language support and term deletion.
- Compare live segment delay, peak memory and CPU to the same primary model without vocabulary assistance; no reuse of unrelated upstream performance numbers.

No vocabulary setting is presented as active recognition support in the current app. Transcript correction provides an honest manual recovery path while this adapter is qualified.
