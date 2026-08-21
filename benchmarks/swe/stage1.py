#!/usr/bin/env python3
"""Stage 1: reify retrieval on SWE-bench Verified.

For each instance: check the repo out at base_commit, incrementally reindex, and run
the reify-bench harness (grep / path-grep / reify / reify-iter3, all budget-matched)
with the issue text as the prompt and the gold patch's files as ground truth. Same
protocol as reify's own benchmark; the task set is just someone else's.

Usage: stage1.py <repo_dir> <instances.jsonl> <out_dir>
"""
import json, pathlib, re, subprocess, sys, time

R = "/Users/lambiengcode/Documents/reify/projects/reify/target/release"
repo_dir = pathlib.Path(sys.argv[1])
instances = [json.loads(l) for l in open(sys.argv[2])]
out_root = pathlib.Path(sys.argv[3]); out_root.mkdir(parents=True, exist_ok=True)

def sh(*args, cwd=None, timeout=1800):
    return subprocess.run(args, cwd=cwd, capture_output=True, text=True, timeout=timeout)

def gold_files(patch):
    return sorted({m.group(1) for m in re.finditer(r"^diff --git a/\S+ b/(\S+)", patch, re.M)})

# Chronological order minimises checkout churn between instances.
instances.sort(key=lambda r: r.get("created_at") or "")

done_path = out_root / "outcomes.jsonl"
done = set()
if done_path.exists():
    done = {json.loads(l)["instance_id"] for l in open(done_path) if l.strip()}

log = open(out_root / "driver.log", "a", buffering=1)
for i, inst in enumerate(instances):
    iid = inst["instance_id"]
    if iid in done:
        continue
    t0 = time.time()
    try:
        r = sh("git", "checkout", "-qf", inst["base_commit"], cwd=repo_dir)
        if r.returncode:
            raise RuntimeError(f"checkout: {r.stderr.strip()[:200]}")
        sh("git", "clean", "-fdq", "-e", ".reify", cwd=repo_dir)
        r = sh(f"{R}/reify", "index", "-C", str(repo_dir))
        if r.returncode:
            raise RuntimeError(f"index: {r.stderr.strip()[:200]}")

        gold = [g for g in gold_files(inst["patch"]) if (repo_dir / g).is_file()]
        if not gold:
            raise RuntimeError("no gold file exists at base")

        task_file = out_root / "task.json"
        task_file.write_text(json.dumps({
            "repository": str(repo_dir), "head": inst["base_commit"],
            "generated_from_commits": 1, "base": None,
            "tasks": [{"id": iid, "prompt": inst["problem_statement"],
                       "ground_truth": gold, "commit": inst["base_commit"],
                       "date": inst.get("created_at") or ""}],
        }))
        run_dir = out_root / "run"
        r = sh(f"{R}/reify-bench", "run", "--repo", str(repo_dir),
               "--tasks", str(task_file), "--out", str(run_dir))
        if r.returncode:
            raise RuntimeError(f"bench: {r.stderr.strip()[:300]}")
        rows = json.load(open(run_dir / "outcomes.json"))
        with open(done_path, "a") as f:
            for row in rows:
                row["instance_id"] = iid
                row["gold_n"] = len(gold)
                f.write(json.dumps(row) + "\n")
        log.write(f"{iid} ok {time.time()-t0:.0f}s ({i+1}/{len(instances)})\n")
    except Exception as e:
        with open(out_root / "errors.jsonl", "a") as f:
            f.write(json.dumps({"instance_id": iid, "error": str(e)[:400]}) + "\n")
        log.write(f"{iid} ERROR {str(e)[:120]}\n")
print("stage1 worker done:", out_root)
