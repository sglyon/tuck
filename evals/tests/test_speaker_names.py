import copy
import json
from pathlib import Path
import pytest
from deepeval.dataset import EvaluationDataset, Golden
from pydantic import ValidationError
from speaker_names.dataset import ROOT, load_dataset, test_case as make_case
from speaker_names.metrics import apply_gates, assessment, metrics
from speaker_names.inference import parse_events
from speaker_names.__main__ import read_predictions

DATASET = load_dataset()
FIXTURE = ROOT / "fixtures/speaker_names/sonnet-4-6-direct.json"
PREDICTIONS = read_predictions(FIXTURE, [g.name for g in DATASET.goldens])


def test_dataset_contract_and_provenance():
    assert isinstance(DATASET, EvaluationDataset)
    assert len(DATASET.goldens) == 24
    assert all(isinstance(g, Golden) for g in DATASET.goldens)
    assert sum(g.additional_metadata["expectsAbstention"] for g in DATASET.goldens) == 16
    for golden in DATASET.goldens:
        assert golden.additional_metadata["synthetic"] is True
        assert golden.additional_metadata["split"] == "development"
        assert set(json.loads(golden.input)) == {"id", "segments", "context"}


@pytest.mark.parametrize("fixture", ["direct", "verify"])
@pytest.mark.parametrize("golden", DATASET.goldens, ids=lambda g: g.name)
def test_historical_gated_output(golden, fixture):
    predictions = read_predictions(ROOT / f"fixtures/speaker_names/sonnet-4-6-{fixture}.json", [g.name for g in DATASET.goldens])
    output = apply_gates(predictions[golden.name], json.loads(golden.input))
    for metric in metrics():
        assert metric.measure(make_case(golden, output)) == 1.0, metric.reason
        assert metric.is_successful()


def test_original_llm_mistake_is_visible_without_gates():
    golden = next(g for g in DATASET.goldens if g.name == "c15")
    case = make_case(golden, PREDICTIONS[golden.name])
    counts = assessment(case)
    assert counts["wrong"] == 1 and counts["policyErrors"] == 1
    assert any(metric.measure(case) == 0 for metric in metrics())


def test_silent_model_cannot_pass_by_always_abstaining():
    golden = DATASET.goldens[0]
    case = make_case(golden, {"suggestions": []})
    coverage = next(m for m in metrics() if m.__name__ == "Name coverage")
    assert coverage.measure(case) == 0
    assert not coverage.is_successful()


def test_unsupported_or_incomplete_evidence_fails():
    golden = DATASET.goldens[2]
    output = copy.deepcopy(PREDICTIONS[golden.name])
    # Real name + real target but missing the second handoff: not enough for supported.
    output["suggestions"][0]["evidenceSegmentIds"] = ["s1", "s2"]
    assert assessment(make_case(golden, output))["evidenceErrors"] == 1
    output["suggestions"][0]["evidenceSegmentIds"] = ["invented"]
    assert apply_gates(output, json.loads(golden.input)) == {"suggestions": []}


def test_overstating_certainty_fails():
    golden = DATASET.goldens[0]
    output = copy.deepcopy(PREDICTIONS[golden.name])
    output["suggestions"][0]["strength"] = "explicit"
    assert assessment(make_case(golden, output))["strengthErrors"] == 1


@pytest.mark.parametrize("output", [None, {}, {"suggestions": "Sarah"},
    {"suggestions": [{"speakerId": "B", "name": "Sarah"}]}])
def test_malformed_outputs_fail_every_metric(output):
    case = make_case(DATASET.goldens[0], output)
    assert all(metric.measure(case) == 0 for metric in metrics())
    with pytest.raises(ValidationError):
        apply_gates(output, json.loads(DATASET.goldens[0].input))


def test_conflicting_names_are_not_silently_deduplicated():
    output = copy.deepcopy(PREDICTIONS["c01"])
    other = copy.deepcopy(output["suggestions"][0]); other["name"] = "Mina"
    output["suggestions"].append(other)
    with pytest.raises(ValidationError):
        apply_gates(output, json.loads(DATASET.goldens[0].input))


def test_missing_or_duplicate_case_rows_rejected(tmp_path):
    for rows in [[], [{"id": "c01", "suggestions": []}] * 2,
                 [{"id": "wrong", "suggestions": []}]]:
        path = tmp_path / "outputs.json"
        path.write_text(json.dumps({"cases": rows}))
        with pytest.raises(ValueError):
            read_predictions(path, ["c01"])


def event(output, **overrides):
    return {"type": "message_end", "message": {"role": "assistant", "model": "test",
        "stopReason": "stop", "content": [{"type": "text", "text": json.dumps(output)}], **overrides}}


def test_omp_parser_selects_successful_final_output():
    output = {"suggestions": []}
    assert parse_events(json.dumps(event(output)))[0] == output
    for events in [[event(output, stopReason="error")],
                   [{"type": "tool_execution_start"}, event(output)], []]:
        with pytest.raises(ValueError):
            parse_events("\n".join(json.dumps(e) for e in events))


def test_inference_uses_omp_and_only_supplied_input(monkeypatch):
    from types import SimpleNamespace
    from speaker_names.inference import infer
    captured = []
    def fake_run(args, **kwargs):
        captured.append((args, kwargs))
        return SimpleNamespace(returncode=0, stdout=json.dumps(event({"suggestions": []})))
    monkeypatch.setattr("speaker_names.inference.subprocess.run", fake_run)
    golden = DATASET.goldens[0]
    infer(json.loads(golden.input), "anthropic/test")
    args, kwargs = captured[0]
    assert args[0] == "omp"
    assert "--no-tools" in args and "--no-session" in args
    assert "expected_output" not in args[-1] and "additional_metadata" not in args[-1]
    assert golden.additional_metadata["reason"] not in args[-1]
    assert json.loads(golden.input)["segments"][0]["text"] in args[-1]


def test_deepeval_is_local_only():
    import os
    from deepeval.confident.api import is_confident
    assert os.environ["DEEPEVAL_TELEMETRY_OPT_OUT"] == "1"
    assert os.environ["DEEPEVAL_DISABLE_DOTENV"] == "1"
    assert not is_confident()
