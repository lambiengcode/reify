# SWE-bench Verified

Reify's own benchmark is built from four repositories we chose. This one is not ours:
[SWE-bench Verified](https://openai.com/index/introducing-swe-bench-verified/) is 500
real GitHub issues across twelve Python projects, human-validated by OpenAI, each
pinned to the `base_commit` the issue was filed against.

That pinning is the reason it fits: it is the same index-before-the-change protocol
Reify's own tasks use, written by other people, so the leakage question is settled by
the dataset rather than by us.

## Stage 1 — retrieval

For each instance: check the repository out at `base_commit`, incrementally reindex,
and score whether the files the accepted fix touched are offered, against grep
baselines at the same token budget. No model is involved.

```bash
python3 stage1.py <repo_dir> <shard.jsonl> <out_dir>
python3 aggregate1.py
```

## Stage 2 — patch generation

The protocol from the original SWE-bench paper: one model, one context budget, and the
**retriever is the only thing that changes** between arms — Reify against a BM25
baseline over the same repository. The model returns SEARCH/REPLACE edit blocks, which
are applied so `git diff` yields a prediction the official harness can judge.

```bash
STAGE2_MODEL=sonnet python3 stage2.py <repo_dir> <shard.jsonl> <out_dir>  # both arms
./eval_batched.sh                                       # grade them, batched
python3 diagnose2.py                                    # paired result + why
```

`stage2.py` numbers every line of the retrieved context and asks for line-range
replacements. That detail is load-bearing: asked for exact SEARCH text instead, the model
reproduces these famous repositories from memory and ~45% of patches fail to apply. With
line ranges it is ~1%.

`eval_batched.sh` pulls each batch's images, grades both arms, then deletes them, so peak
disk stays near 18 GB rather than the ~200 GB a full pre-pull needs. It runs through
`run_eval_x86.py`, which applies `force_x86.py`.

## Apple Silicon

`swebench==3.0.17` matches this dataset's schema (5.x expects a newer one), and Docker
works through [colima](https://github.com/abiosoft/colima).

About 40 Verified instances have **no arm64 image and cannot get one** — their conda
specs pin packages like `setuptools==38.2.4` that were never built for `aarch64`, so
building locally fails for the same reason pulling does. `force_x86.py` widens the
harness's own `USE_X86` escape hatch to every instance; `eval_batched.sh` then pre-pulls
each image with `--platform linux/amd64`, because the harness otherwise pulls with the
daemon's native platform and 404s. With both in place there are zero environment errors.

Run the *gold* patches through the harness first — it is a cheap way to prove the grader
works before trusting any arm.

## Results

`results/stage1-retrieval.txt` and `stage1-outcomes.jsonl` hold the retrieval numbers for
all 500 instances; `results/stage2-endtoend.json` holds the per-instance resolve outcomes
for both arms.
