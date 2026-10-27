#!/usr/bin/env python3
"""Preserve one Tuck meeting or compare a local refinement with that baseline."""
import argparse, collections, hashlib, json, os, pathlib, re, shutil, sqlite3, subprocess, uuid

def write(path, value):
    with path.open("x", encoding="utf-8") as file:
        os.chmod(path, 0o600)
        json.dump(value, file, indent=2)
        file.write("\n")

def tokens(text):
    return re.findall(r"\w+", text.lower())

def stats(segments):
    speakers=collections.Counter(s["speakerId"] for s in segments)
    return {"segments":len(segments), "words":sum(len(tokens(s["text"])) for s in segments),
            "speakerIds":dict(speakers), "lastTranscriptSecond":max((s["end"] for s in segments),default=0),
            "wordsBySource":{source:sum(len(tokens(s["text"])) for s in segments if s["source"]==source) for source in set(s["source"] for s in segments)}}

def edit_distance(a,b):
    row=list(range(len(b)+1))
    for i,x in enumerate(a,1):
        new=[i]
        for j,y in enumerate(b,1):
            new.append(min(new[-1]+1,row[j]+1,row[j-1]+(x!=y)))
        row=new
    return row[-1]

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    sub=parser.add_subparsers(dest="command",required=True)
    capture=sub.add_parser("capture",help="Read-only snapshot; refuses active meetings and existing case folders")
    capture.add_argument("meeting_id",type=uuid.UUID)
    capture.add_argument("case_dir",type=pathlib.Path)
    capture.add_argument("--data-dir",type=pathlib.Path,default=pathlib.Path.home()/"Library/Application Support/com.sglyon.tuck")
    compare=sub.add_parser("compare",help="Compare a candidate JSON produced by tuck-audio-eval")
    compare.add_argument("case_dir",type=pathlib.Path)
    compare.add_argument("candidate",type=pathlib.Path)
    compare.add_argument("--output",required=True,type=pathlib.Path)
    compare.add_argument("--reference",type=pathlib.Path,help="Optional JSON {segments:[{start,end,text}]} in seconds")
    compare.add_argument("--reference-offset",type=float,default=0,help="Seconds to add to reference timestamps to align to Tuck")
    args=parser.parse_args()
    if args.command=="capture":
        ident=str(args.meeting_id)
        with sqlite3.connect((args.data_dir/"tuck.sqlite3").resolve().as_uri()+"?mode=ro",uri=True) as db:
            db.execute("BEGIN")
            found=db.execute("SELECT document FROM meetings WHERE id=?",(ident,)).fetchone()
            if found is None:parser.error("Meeting not found")
            meeting=json.loads(found[0])
            if meeting["status"] in ["recording","paused","processing"]:parser.error("Finish recording before evaluating")
            segments=[json.loads(row[0]) for row in db.execute("SELECT document FROM segments WHERE meeting_id=? ORDER BY start",(ident,))]
        args.case_dir.mkdir(mode=0o700,parents=True,exist_ok=False)
        hashes={}
        for name in ["microphone.wav","system.wav","import.wav"]:
            source=args.data_dir/"audio"/ident/name
            if not source.exists():continue
            if source.is_symlink() or not source.is_file():parser.error("Saved audio must be a regular file")
            target=args.case_dir/name
            shutil.copyfile(source,target);os.chmod(target,0o600)
            with target.open("rb") as file:hashes[name]=hashlib.file_digest(file,"sha256").hexdigest()
        write(args.case_dir/"baseline.json",{"meeting":meeting,"segments":segments,"audioSha256":hashes})
        print(f"Preserved {len(segments)} segments and {len(hashes)} tracks in {args.case_dir}")
    else:
        baseline=json.loads((args.case_dir/"baseline.json").read_text())["segments"]
        candidate=json.loads(args.candidate.read_text())
        repo=pathlib.Path(__file__).resolve().parent.parent
        implementation={"revision":subprocess.check_output(["git","rev-parse","HEAD"],cwd=repo,text=True).strip(),
                        "workingTreeDirty":bool(subprocess.check_output(["git","status","--porcelain"],cwd=repo,text=True).strip()),
                        "diarizationModelRevision":json.loads((repo/"native/apple-speech/diarization-models.json").read_text())["revision"],
                        "sourceSha256":{}}
        for relative in ["src-tauri/src/adapters/refinement.rs","src-tauri/src/adapters/speech.rs","native/apple-speech/Sources/TuckSpeech/Speech.swift","native/apple-speech/Sources/TuckSpeech/Diarization.swift"]:
            with (repo/relative).open("rb") as file:implementation["sourceSha256"][relative]=hashlib.file_digest(file,"sha256").hexdigest()
        report={"implementation":implementation,"baseline":stats(baseline),"candidate":stats(candidate["segments"]),
                "audioSeconds":candidate.get("duration"),"processingSeconds":candidate.get("elapsedSeconds"),
                "limitations":["Speaker IDs are clusters, not verified people. Unassigned is uncertainty, not another voice.","Count and text similarity alone do not measure diarization correctness."]}
        if args.reference:
            reference=json.loads(args.reference.read_text())["segments"]
            end=min(max((s["end"] for s in baseline),default=0),max((s["end"]+args.reference_offset for s in reference),default=0))
            start=max(0,min((s["start"]+args.reference_offset for s in reference),default=0))
            ref=tokens(" ".join(s["text"] for s in reference if s["end"]+args.reference_offset>=start and s["start"]+args.reference_offset<=end))
            if not ref:parser.error("Reference and recording do not overlap")
            report["referenceComparison"]={"start":start,"end":end,"referenceWords":len(ref),"note":"Approximate segment-boundary comparison; a second ASR transcript is not human ground truth."}
            for name,segs in [("baseline",baseline),("candidate",candidate["segments"])]:
                hyp=tokens(" ".join(s["text"] for s in sorted(segs,key=lambda s:s["start"]) if s["end"]>=start and s["start"]<=end))
                distance=edit_distance(ref,hyp)
                report["referenceComparison"][name]={"editDistance":distance,"editRate":distance/len(ref)}
        write(args.output,report)
        print(json.dumps(report,indent=2))

if __name__=="__main__":main()
