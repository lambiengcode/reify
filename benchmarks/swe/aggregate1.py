#!/usr/bin/env python3
"""Stage-1 aggregate: reify vs grep retrieval on SWE-bench Verified."""
import json, glob, pathlib
from math import sqrt
SB = pathlib.Path("/private/tmp/claude-501/-Users-lambiengcode-Documents-reify/025dbc2c-af5b-4475-b2a8-20daab4cac22/scratchpad/swe")
rows = []
for f in glob.glob(str(SB/"out/*/outcomes.jsonl")):
    rows += [json.loads(l) for l in open(f)]
def wilson(k,n,z=1.96):
    if not n: return (0,0)
    p=k/n; d=1+z*z/n; c=(p+z*z/(2*n))/d; h=z*sqrt(p*(1-p)/n+z*z/(4*n*n))/d
    return (max(0,c-h), min(1,c+h))
by = {}
for r in rows: by.setdefault(r["condition"], []).append(r)
n_inst = len({r["instance_id"] for r in rows})
print(f"SWE-bench Verified retrieval — {n_inst} instances, budget 4000 tok\n")
print(f"{'condition':16} {'hit (any gold file offered)':>28} {'MRR':>6} {'full recall':>12}")
for cond in ["B-content-grep","C-path-grep","R-reify","R-reify-iter3"]:
    rs = by.get(cond, [])
    n = len(rs)
    hits = sum(1 for r in rs if r.get("first_hit_rank"))
    mrr = sum(1/r["first_hit_rank"] for r in rs if r.get("first_hit_rank"))/max(n,1)
    full = sum(1 for r in rs if r.get("recall")==1.0)
    lo,hi = wilson(hits,n)
    print(f"{cond:16} {hits/max(n,1):8.1%} [{lo:.1%}-{hi:.1%}] n={n:<4} {mrr:6.2f} {full/max(n,1):11.1%}")
# per-repo reify-iter3 vs grep
print("\nper-repo, reify×3 vs grep (hit rate):")
per = {}
for r in rows:
    repo = r["instance_id"].split("__")[0]
    per.setdefault(repo, {}).setdefault(r["condition"], []).append(bool(r.get("first_hit_rank")))
for repo in sorted(per):
    g = per[repo].get("B-content-grep", []); ri = per[repo].get("R-reify-iter3", [])
    if ri: print(f"  {repo:14} grep {sum(g)/len(g):5.0%}  reify×3 {sum(ri)/len(ri):5.0%}  (n={len(ri)})")
