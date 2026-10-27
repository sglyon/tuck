import json
from pathlib import Path
from deepeval.dataset import EvaluationDataset, Golden
from deepeval.test_case import LLMTestCase
from .schema import Suggestions

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_DATASET = ROOT / "datasets/speaker_names/synthetic-v1.json"


def load_dataset(path=DEFAULT_DATASET):
    rows = json.loads(Path(path).read_text())
    goldens = [Golden.model_validate(row) for row in rows]
    names = [g.name for g in goldens]
    if not goldens or any(not n for n in names) or len(set(names)) != len(names):
        raise ValueError("Dataset needs unique, nonempty case names.")
    for golden in goldens:
        state = json.loads(golden.input)
        if set(state) != {"id", "segments", "context"} or state["id"] != golden.name:
            raise ValueError("Input must contain only case ID, segments and context.")
        ids = [s["id"] for s in state["segments"]]
        if len(ids) != len(set(ids)):
            raise ValueError("Duplicate segment IDs.")
        Suggestions.model_validate_json(golden.expected_output)
    return EvaluationDataset(goldens=goldens)


def test_case(golden, output):
    return LLMTestCase(name=golden.name, input=golden.input,
                       actual_output=json.dumps(output), expected_output=golden.expected_output,
                       additional_metadata=golden.additional_metadata)
