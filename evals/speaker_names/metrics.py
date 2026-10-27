"""Deterministic DeepEval metrics. No LLM judge, API key or model calls."""
import json
import re
from deepeval.metrics import BaseMetric
from .schema import Suggestions


def key(s):
    return s.speakerId, s.name.casefold()


def grounded(s, state):
    segments = {v["id"]: v for v in state["segments"]}
    ids = s.evidenceSegmentIds
    return (all(i in segments for i in ids)
            and any(segments[i]["speakerId"] == s.speakerId for i in ids)
            and any(re.search(r"(?<!\w)" + re.escape(s.name) + r"(?!\w)",
                              segments[i]["text"], re.I) for i in ids))


def permitted(s, state):
    context = state["context"]
    return (context.get("speakerAssignmentsStable") is True
            and s.speakerId != "unassigned"
            and s.speakerId not in context.get("confirmedNames", {})
            and s.name.casefold() not in {n.casefold() for n in context.get("uncertainNames", [])})


def apply_gates(output, state):
    # Parse first, including empty arrays. Malformed output is never converted into a successful abstention.
    parsed = Suggestions.model_validate(output)
    return {"suggestions": [s.model_dump() for s in parsed.suggestions
                             if grounded(s, state) and permitted(s, state)]}


def assessment(test_case):
    expected = Suggestions.model_validate_json(test_case.expected_output).suggestions
    actual = Suggestions.model_validate_json(test_case.actual_output).suggestions
    state = json.loads(test_case.input)
    gold = {key(s): s for s in expected}
    pred = {key(s): s for s in actual}
    matched = gold.keys() & pred.keys()
    wrong = len(pred.keys() - gold.keys())
    missing = len(gold.keys() - pred.keys())
    strength_errors = sum(gold[k].strength != pred[k].strength for k in matched)
    evidence_errors = sum(not grounded(s, state) or
                          (key(s) in gold and not set(gold[key(s)].evidenceSegmentIds).issubset(s.evidenceSegmentIds))
                          for s in actual)
    policy_errors = sum(not permitted(s, state) for s in actual)
    return {"correct": len(matched), "wrong": wrong, "missed": missing,
            "strengthErrors": strength_errors, "evidenceErrors": evidence_errors,
            "policyErrors": policy_errors, "expected": len(expected), "predicted": len(actual)}


class SpeakerNameMetric(BaseMetric):
    def __init__(self, kind):
        self.kind = kind
        self.threshold = 1.0
        self.async_mode = False
        self.strict_mode = True
        self.include_reason = True
        self.evaluation_model = None
        self.error = None

    def measure(self, test_case, **kwargs):
        try:
            a = assessment(test_case)
            values = {
                "No incorrect names": float(a["wrong"] == 0),
                "Name coverage": a["correct"] / a["expected"] if a["expected"] else 1.0,
                "Evidence grounding": float(a["evidenceErrors"] == 0),
                "Evidence strength": float(a["strengthErrors"] == 0),
                "Abstention": float(a["predicted"] == 0) if a["expected"] == 0 else 1.0,
                "Naming policy": float(a["policyErrors"] == 0),
            }
            self.score = values[self.kind]
            self.reason = json.dumps(a)
        except (ValueError, TypeError, KeyError) as error:
            self.score = 0.0
            self.reason = f"Invalid structured output: {error}"
        self.success = self.score >= self.threshold
        return self.score

    async def a_measure(self, test_case, **kwargs):
        return self.measure(test_case)

    def is_successful(self):
        return self.error is None and self.success

    @property
    def __name__(self):
        return self.kind


def metrics():
    return [SpeakerNameMetric(kind) for kind in ["No incorrect names", "Name coverage",
            "Evidence grounding", "Evidence strength", "Abstention", "Naming policy"]]
