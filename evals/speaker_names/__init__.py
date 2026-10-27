"""Local evaluation only; inference is explicitly delegated to omp."""
import os

# Set before importing DeepEval. Do not load ambient dotenv/key files or upload runs.
os.environ["DEEPEVAL_TELEMETRY_OPT_OUT"] = "1"
os.environ["DEEPEVAL_DISABLE_DOTENV"] = "1"
os.environ["DEEPEVAL_DISABLE_LEGACY_KEYFILE"] = "1"
os.environ.pop("CONFIDENT_API_KEY", None)
