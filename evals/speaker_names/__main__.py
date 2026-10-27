import argparse
import hashlib
import json
from pathlib import Path
from deepeval import evaluate
from deepeval.evaluate.configs import AsyncConfig, CacheConfig, DisplayConfig
from .dataset import DEFAULT_DATASET, load_dataset, test_case
from .inference import POLICY, infer
from .metrics import apply_gates, assessment, metrics


def read_predictions(path, expected_ids):
    rows = json.loads(Path(path).read_text())["cases"]
    ids = [row["id"] for row in rows]
    if len(ids) != len(set(ids)) or set(ids) != set(expected_ids):
        raise ValueError("Prediction file must contain every dataset ID exactly once, with no unknown IDs.")
    return {row["id"]: {"suggestions": row["suggestions"]} for row in rows}


def main():
    parser = argparse.ArgumentParser(description="Local DeepEval speaker-name evaluation; inference via omp.")
    parser.add_argument("command", choices=["list", "replay", "infer"])
    parser.add_argument("--dataset", type=Path, default=DEFAULT_DATASET)
    parser.add_argument("--predictions", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--case", action="append", default=[])
    parser.add_argument("--gated", action="store_true", help="Apply explicit evidence/policy checks before scoring.")
    parser.add_argument("--strategy", choices=["direct", "cascade"], default="direct")
    parser.add_argument("--model", default="anthropic/claude-sonnet-4-6")
    args = parser.parse_args()
    dataset = load_dataset(args.dataset)
    goldens = dataset.goldens
    if args.command == "list":
        for g in goldens:
            print(g.name, "abstain" if g.additional_metadata["expectsAbstention"] else "suggest")
        return 0
    if not args.output:
        parser.error("--output is required (new directory)")
    ids = {g.name for g in goldens}
    if set(args.case) - ids:
        parser.error("Unknown case ID")
    predictions = None
    if args.command == "replay":
        if not args.predictions:
            parser.error("replay requires --predictions")
        predictions = read_predictions(args.predictions, ids)
    args.output.mkdir(parents=True, exist_ok=False)
    selected = [g for g in goldens if not args.case or g.name in args.case]
    test_cases, outputs, counts, usage = [], [], [], []
    for golden in selected:
        state = json.loads(golden.input)
        if predictions is not None:
            raw = predictions[golden.name]
        else:
            raw, meta = infer(state, args.model)
            usage.append({"case": golden.name, "stage": "direct", **meta})
            if args.strategy == "cascade":
                raw, meta = infer(state, args.model, raw["suggestions"])
                usage.append({"case": golden.name, "stage": "verify", **meta})
        output = apply_gates(raw, state) if args.gated else raw
        tc = test_case(golden, output)
        test_cases.append(tc)
        counts.append({"id": golden.name, **assessment(tc)})
        outputs.append({"id": golden.name, "suggestions": output["suggestions"], "raw": raw})
    provenance = {"datasetSha256": hashlib.sha256(args.dataset.read_bytes()).hexdigest(),
                  "policySha256": hashlib.sha256(POLICY.encode()).hexdigest(),
                  "inferenceImplementationSha256": hashlib.sha256(Path(__file__).with_name("inference.py").read_bytes()).hexdigest(),
                  "command": args.command, "strategy": args.strategy, "gated": args.gated,
                  "model": args.model if args.command == "infer" else None,
                  "predictionSource": str(args.predictions) if args.predictions else None,
                  "predictionSha256": hashlib.sha256(args.predictions.read_bytes()).hexdigest() if args.predictions else None,
                  "predictionProvenance": json.loads(args.predictions.read_text()).get("provenance") if args.predictions else None,
                  "executionMode": "per-case" if args.command == "infer" else "replay",
                  "cases": [g.name for g in selected], "usage": usage}
    (args.output / "predictions.json").write_text(json.dumps({"cases": outputs}, indent=2) + "\n")
    (args.output / "manifest.json").write_text(json.dumps(provenance, indent=2) + "\n")
    result = evaluate(test_cases, metrics=metrics(), async_config=AsyncConfig(run_async=False),
                      cache_config=CacheConfig(write_cache=False, use_cache=False),
                      display_config=DisplayConfig(show_indicator=False, print_results=False,
                                                   inspect_after_run=False, results_folder=str(args.output)))
    totals = {key: sum(c[key] for c in counts) for key in ["correct", "wrong", "missed", "strengthErrors", "evidenceErrors", "policyErrors"]}
    summary = {"cases": len(counts), "passed": sum(t.success for t in result.test_results),
               **totals, "details": counts}
    (args.output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    print(json.dumps({k:v for k,v in summary.items() if k != "details"}, indent=2))
    return 0 if all(t.success for t in result.test_results) else 1


if __name__ == "__main__":
    raise SystemExit(main())
