#!/usr/bin/env python3
"""Reproduce the original stdlib-only WER/target/latency analysis; no inference.

File hashes are calculated only for explicit --binary/--source/--model-file
paths. --check compares all generated non-provenance fields with a saved JSON.
"""
import argparse
import hashlib
import json
import pathlib
import re
import statistics


def normalize(text):
    return " ".join(re.findall(r"[a-z0-9]+", text.casefold()))


def occurrences(text, term):
    pattern = r"(?<![a-z0-9])" + re.escape(normalize(term)) + r"(?![a-z0-9])"
    return len(re.findall(pattern, normalize(text)))


def word_errors(reference, hypothesis):
    reference, hypothesis = normalize(reference).split(), normalize(hypothesis).split()
    row = list(range(len(hypothesis) + 1))
    for i, source in enumerate(reference, 1):
        next_row = [i]
        for j, target in enumerate(hypothesis, 1):
            next_row.append(min(next_row[-1] + 1, row[j] + 1,
                                row[j - 1] + (source != target)))
        row = next_row
    return row[-1], len(reference)


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def analyze(results, experiment, assets):
    baselines = [r for r in results if r["event"] == "baseline"]
    references = {r["id"]: r for r in baselines}
    expected_ids = [f["id"] for f in experiment["fixtures"]]
    if len(references) != len(baselines) or list(references) != expected_ids:
        raise ValueError("Missing, duplicated or reordered preregistered baseline cases")
    for fixture in experiment["fixtures"]:
        row = references[fixture["id"]]
        if row["expected"] != fixture["expected"] or row["reference"] != fixture["reference"]:
            raise ValueError("Reference/target selection changed")

    summary = {
        "selection": "Preregistered first six unfiltered test rows plus Kennedy and silence controls",
        "configs": {},
        "assetsBytes": sum(f["bytes"] for f in assets["files"]),
        "ctcRevision": assets["revision"],
    }
    all_results = [("baseline", baselines)]
    for config in ["default", "conservative", "removed"]:
        rows = [r for r in results if r["event"] == "rescored" and r["config"] == config]
        if [r["id"] for r in rows] != expected_ids:
            raise ValueError("Incomplete rescoring context: " + config)
        all_results.append((config, rows))
    for name, rows in all_results:
        found = sum(sum(occurrences(r["text"], term) > 0
                        for term in references[r["id"]]["expected"]) for r in rows)
        total = sum(len(r["expected"]) for r in baselines)
        errors = [word_errors(references[r["id"]]["reference"], r["text"]) for r in rows]
        error_count, word_count = sum(e for e, _ in errors), sum(n for _, n in errors)
        extra = sum(sum(max(0, occurrences(r["text"], term)
                            - occurrences(references[r["id"]]["reference"], term))
                        for term in experiment["terms"]) for r in rows)
        metrics = {"exactTargetPairsFound": found, "targetPairs": total,
                   "wordErrors": error_count, "referenceWords": word_count,
                   "wordErrorRate": error_count / word_count,
                   "extraConfiguredTermOccurrences": extra}
        if name == "baseline":
            summary[name] = metrics
        else:
            summary["configs"][name] = metrics

    default = dict((r["id"], r) for r in all_results[1][1])
    summary["warmCTCSeconds"] = [default[r["id"]]["ctcSeconds"] for r in baselines]
    fifteen_seconds = [r for r in baselines if r["duration"] == 15]
    summary["median15sCTCSeconds"] = statistics.median(
        default[r["id"]]["ctcSeconds"] for r in fifteen_seconds)
    summary["baseline15sSeconds"] = [r["seconds"] for r in fifteen_seconds]
    summary["removedContextMatchesBaseline"] = all(
        r["text"] == references[r["id"]]["text"] for r in all_results[3][1])
    summary["falseInsertionAcceptancePassed"] = all(
        summary["configs"][name]["extraConfiguredTermOccurrences"] == 0
        for name in ["default", "conservative"])
    return summary


def main():
    root = pathlib.Path(__file__).resolve().parent
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--results", type=pathlib.Path, default=root / "evidence/results.jsonl")
    parser.add_argument("--experiment", type=pathlib.Path, default=root / "experiment.json")
    parser.add_argument("--assets", type=pathlib.Path, default=root / "ctc-models.json")
    parser.add_argument("--binary", type=pathlib.Path)
    parser.add_argument("--source", type=pathlib.Path)
    parser.add_argument("--model-file", type=pathlib.Path)
    parser.add_argument("--sdk-revision", help="Optional explicitly supplied SDK commit; not inferred")
    parser.add_argument("--output", type=pathlib.Path)
    parser.add_argument("--check", type=pathlib.Path)
    args = parser.parse_args()
    results = [json.loads(line) for line in args.results.read_text().splitlines() if line.strip()]
    summary = analyze(results, json.loads(args.experiment.read_text()),
                      json.loads(args.assets.read_text()))
    if args.check:
        saved = json.loads(args.check.read_text())
        for key, value in summary.items():
            if saved.get(key) != value:
                raise ValueError("Saved analysis differs: " + key)
    for path, key in [(args.binary, "runtimeSHA256"), (args.source, "measuredMainSourceSHA256"),
                      (args.model_file, "explicitModelFileSHA256")]:
        if path is not None:
            summary[key] = sha256(path)
    if args.sdk_revision is not None:
        if not re.fullmatch(r"[a-f0-9]{40}", args.sdk_revision):
            raise ValueError("SDK revision must be an explicit full commit SHA")
        summary["sdkRevision"] = args.sdk_revision
    output = json.dumps(summary, indent=2) + "\n"
    if args.output:
        args.output.write_text(output)
    else:
        print(output, end="")


if __name__ == "__main__":
    main()
