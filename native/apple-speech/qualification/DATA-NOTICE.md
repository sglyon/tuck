# Vocabulary experiment data attribution

The six reference excerpts and vocabulary annotations in `experiment.json`
come from Argmax's
[contextual-earnings22](https://huggingface.co/datasets/argmaxinc/contextual-earnings22)
test rows 0–5, cached extraction revision
`59d7f94771cc197984d8038e494c83abeff87282`, derived from Rev's
[Earnings-22](https://github.com/revdotcom/speech-datasets/tree/main/earnings22).
Credit: Argmax, Rev, Miguel Del Rio and the Earnings-22 authors.

The Argmax dataset card states that its license follows the original dataset.
Rev's [Earnings-22 license notice](https://github.com/revdotcom/speech-datasets/blob/main/earnings22/LICENSE.md)
places transcripts and associated alignment text under
[Creative Commons Attribution-ShareAlike 4.0 International](https://creativecommons.org/licenses/by-sa/4.0/).
The copied reference/vocabulary portions of `experiment.json` and the
combined diagnostic dataset in `evidence/results.jsonl` are provided under
that same CC BY-SA 4.0 license. This notice concerns those experiment data,
not the application or qualification executable's source code.

Changes: a fixed six-row subset was selected; local PCM filenames, checksums,
expected terms and controls were added. Diagnostic text includes uncorrected
machine recognition and experimental vocabulary-assisted alternatives, which
may be inaccurate. WER uses case-folded alphanumeric normalization rather
than Rev's official `fstalign` scoring. No endorsement by Argmax, Rev or the
speakers is implied.

Dataset citation: Miguel Del Rio et al. (2023), *Accents in Speech Recognition
through the Lens of a World Englishes Evaluation Set*, Research in Language
21(3), 225–244. See the
[upstream citation and author list](https://github.com/revdotcom/speech-datasets/blob/main/earnings22/README.md#cite-this-dataset)
and [original Earnings-22 paper](https://arxiv.org/abs/2203.15591).

The audio recordings are downloaded only by explicit developer commands into
a chosen temporary directory. They are not redistributed in this repository
or bundled in the app; the transcript license notice is not represented as
a separate grant of rights to the original recordings.

The optional CTC weights remain separate downloads. NVIDIA's Parakeet
TDT-CTC 110M and FluidInference's Core ML conversion are attributed in the
[pinned model card](https://huggingface.co/FluidInference/parakeet-ctc-110m-coreml/tree/accdafd8cf8a2ff1cabe3c11e54416b405d409aa),
which declares [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/).
Conversion changes model format/precision. No weights are distributed here.
