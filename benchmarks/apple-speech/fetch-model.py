#!/usr/bin/env python3
"""Explicit benchmark-only download; no Python inference dependency."""
import concurrent.futures
import hashlib
import json
from pathlib import Path
import sys
import urllib.request

REPO = "FluidInference/parakeet-tdt-0.6b-v3-coreml"
REVISION = "7dd20fe6b1797d35f5e3307e8b1732d9a178edfe"
COMPONENTS = {"Preprocessor.mlmodelc", "Encoder.mlmodelc", "Decoder.mlmodelc",
              "JointDecisionv3.mlmodelc", "parakeet_vocab.json"}

def main():
    destination = Path(sys.argv[1])
    components = set(COMPONENTS)
    if "--int8-v2" in sys.argv[2:]:
        components.remove("Encoder.mlmodelc")
        components.add("Encoder_v2.mlmodelc")
    with urllib.request.urlopen(
        f"https://huggingface.co/api/models/{REPO}/tree/{REVISION}?recursive=true&expand=false"
    ) as response:
        entries = json.load(response)
    files = [entry for entry in entries if entry["type"] == "file"
             and entry["path"].split("/")[0] in components]
    assert len(files) == (20 if "--int8-v2" in sys.argv[2:] else 21), "Review changed model manifest"
    destination.mkdir(parents=True, exist_ok=True)

    def fetch(entry):
        relative = entry["path"]
        target = destination / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        if not target.exists():
            partial = target.with_suffix(target.suffix + ".partial")
            urllib.request.urlretrieve(
                f"https://huggingface.co/{REPO}/resolve/{REVISION}/{relative}", partial)
            partial.replace(target)
        data = target.read_bytes()
        assert len(data) == entry["size"], relative
        sha256 = hashlib.sha256(data).hexdigest()
        if "lfs" in entry:
            assert sha256 == entry["lfs"]["oid"], relative
        else:
            blob = f"blob {len(data)}\0".encode() + data
            assert hashlib.sha1(blob).hexdigest() == entry["oid"], relative
        print(f"verified {relative}", flush=True)
        return {"path": relative, "bytes": len(data), "sha256": sha256}

    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as executor:
        verified = list(executor.map(fetch, files))
    manifest = {"repo": REPO, "revision": REVISION, "files": verified,
                "total_bytes": sum(item["bytes"] for item in verified)}
    (destination / "verified-manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(json.dumps({"total_bytes": manifest["total_bytes"], "destination": str(destination)}))

if __name__ == "__main__":
    main()
