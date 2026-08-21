#!/usr/bin/env python3
"""Why did an arm resolve or not? Cross-reference Stage-1 retrieval with Stage-2 outcomes."""
import json, glob
SB="/private/tmp/claude-501/-Users-lambiengcode-Documents-reify/025dbc2c-af5b-4475-b2a8-20daab4cac22/scratchpad/swe"
# Stage 1: did reify offer a gold file at 4k budget?
offered = {}
for f in glob.glob(f"{SB}/out/*/outcomes.jsonl"):
    for l in open(f):
        r = json.loads(l)
        if r["condition"] == "R-reify":
            offered[r["instance_id"]] = r.get("first_hit_rank") is not None
def collect(arm):
    res, unres = set(), set()
    for f in glob.glob(f"{SB}/*b[0-9]*-{arm}.json"):
        d = json.load(open(f))
        res |= set(d.get("resolved_ids", [])); unres |= set(d.get("unresolved_ids", []))
    return res, unres
R, Ru = collect("reify"); B, Bu = collect("bm25")
both = (R|Ru) & (B|Bu)
print(f"jointly graded: {len(both)}")
print(f"  reify resolved {len(R&both)}, bm25 resolved {len(B&both)}")
only_b = (B&both) - R
print(f"\n{len(only_b)} instances bm25 solved and reify did not:")
hit = sum(1 for i in only_b if offered.get(i))
print(f"  reify HAD offered a gold file in {hit}/{len(only_b)} of them (Stage-1, 4k budget)")
print("  -> so the loss is not retrieval; it is what the context contained/ordered")
only_r = (R&both) - B
print(f"\n{len(only_r)} instances reify solved and bm25 did not")
# how many files each arm fed
import re
counts = {"reify": [], "bm25": []}
for f in glob.glob(f"{SB}/out2c/*/driver.log"):
    for l in open(f):
        m = re.search(r"reify\(files=(\d+).*bm25\(files=(\d+)", l)
        if m:
            counts["reify"].append(int(m.group(1))); counts["bm25"].append(int(m.group(2)))
for k, v in counts.items():
    if v: print(f"\nmedian files fed to the model, {k}: {sorted(v)[len(v)//2]}")
