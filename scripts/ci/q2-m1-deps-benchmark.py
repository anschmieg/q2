#!/usr/bin/env python3
"""Native HTTP preview dependency benchmark: same runner, pinned baseline vs patch."""
import argparse
import json
import math
import re
import statistics
import subprocess
import tempfile
import time
import urllib.request
from pathlib import Path

CASES = [
    ("empty",0,0,32), ("depth-1",1,0,32), ("depth-8",8,0,32),
    ("depth-64",64,0,20), ("project-100",8,100,20),
    ("project-1000",8,1000,12), ("project-3000",8,3000,8),
]

def fixture(root,depth,siblings):
    root.mkdir(parents=True)
    (root/"_quarto.yml").write_text("project:\n  type: website\n")
    (root/"index.qmd").write_text("# Active\n\n"+("{{< include parts/p000.qmd >}}\n" if depth else "No includes.\n"))
    if depth:
        (root/"parts").mkdir()
        for i in range(depth):
            child=f"{{{{< include p{i+1:03d}.qmd >}}}}\n" if i+1<depth else ""
            (root/"parts"/f"p{i:03d}.qmd").write_text(f"Part {i}\n\n{child}")
    for i in range(siblings):
        (root/f"sibling_{i:04d}.qmd").write_text(f"# Unrelated {i}\n\nBody {i}\n")

def start(binary,root,logfile):
    h=logfile.open("wb")
    proc=subprocess.Popen([str(binary.resolve()),"preview","--no-browser","--port","0",str(root)],
        stdin=subprocess.DEVNULL,stdout=h,stderr=subprocess.STDOUT)
    deadline=time.monotonic()+120
    while time.monotonic()<deadline and proc.poll() is None:
        match=re.search(r"→\s+http://127\.0\.0\.1:(\d+)",logfile.read_text(errors="replace"))
        if match:
            try:
                with urllib.request.urlopen(f"http://127.0.0.1:{match[1]}/health",timeout=2) as r:
                    if r.status==200:
                        return proc,h,int(match[1])
            except OSError:
                pass
        time.sleep(.12)
    proc.terminate()
    h.close()
    raise RuntimeError("q2 preview failed to start: "+logfile.read_text(errors="replace")[-2500:])

def measure(port,depth,variant,count):
    measurements=[]
    url=f"http://127.0.0.1:{port}/api/preview/deps?page=index.qmd"
    expected=([f"parts/p{i:03d}.qmd" for i in range(depth)] if variant in ("original","patched")
        else (["parts/p000.qmd"] if depth else []))
    for i in range(count+3):
        t=time.perf_counter_ns()
        with urllib.request.urlopen(url,timeout=60) as r:
            assert r.status==200
            answer=json.loads(r.read())
        elapsed=(time.perf_counter_ns()-t)/1e6
        assert answer.get("deps")==expected,(variant,depth,answer,expected)
        if i>=3:
            measurements.append(round(elapsed,5))
    ordered=sorted(measurements)
    return {"median_ms":round(statistics.median(measurements),4),
            "p95_ms":round(ordered[math.ceil(.95*len(ordered))-1],4),
            "samples_ms":measurements}

def main():
    ap=argparse.ArgumentParser()
    for param in ["baseline","original","patched","output"]:
        ap.add_argument("--"+param,type=Path,required=True)
    opts=ap.parse_args()
    opts.output.mkdir(parents=True,exist_ok=True)
    results=[]
    with tempfile.TemporaryDirectory(prefix="q2-m1-perf-") as d:
        for label,depth,siblings,n in CASES:
            project=Path(d)/label
            fixture(project,depth,siblings)
            row={"case":label,"include_depth":depth,"sibling_count":siblings,"n":n}
            for variant,binary in [("baseline",opts.baseline),("original",opts.original),("patched",opts.patched)]:
                proc=h=None
                try:
                    proc,h,port=start(binary,project,opts.output/f"{label}-{variant}.log")
                    row[variant]=measure(port,depth,variant,n)
                finally:
                    if proc:
                        proc.terminate()
                        try:proc.wait(timeout=4)
                        except subprocess.TimeoutExpired:proc.kill()
                    if h:h.close()
            b=row["baseline"]["median_ms"]
            old=row["original"]["median_ms"]
            p=row["patched"]["median_ms"]
            row["original_delta_ms"]=round(old-b,4)
            row["optimized_delta_ms"]=round(p-b,4)
            row["optimization_ratio"]=round(p/old,3)
            results.append(row)
            print(f"{label}: base {b:.3f}ms, old M1 {old:.3f}ms, optimized {p:.3f}ms, old->new {old/p:.2f}x",flush=True)
    (opts.output/"results.json").write_text(json.dumps(results,indent=2)+"\n")
    report=[
        "# Q2 M1 native preview dependency endpoint overhead",
        "", "Paired baseline and patch binaries built on the same CI runner,",
        "native warm HTTP endpoint latency (not startup or browser rendering).",
        "Each case includes 3 warmups, then 8–32 measured requests.",
        "", "| Case | Depth | Siblings | Baseline p50 | Original M1 p50 | Optimized M1 p50 | Original p95 | Optimized p95 |",
        "|---|---:|---:|---:|---:|---:|---:|---:|"
    ]
    for row in results:
        a=row["baseline"];o=row["original"];b=row["patched"]
        report.append(f'| {row["case"]} | {row["include_depth"]} | {row["sibling_count"]} | {a["median_ms"]:.3f} | {o["median_ms"]:.3f} | {b["median_ms"]:.3f} | {o["p95_ms"]:.3f} | {b["p95_ms"]:.3f} |')
    report+=["","Single CI runner and fixed order: interpret as diagnostic, not statistically proven speedup/slowdown."]
    (opts.output/"results.md").write_text("\n".join(report)+"\n")

if __name__=="__main__":
    main()
