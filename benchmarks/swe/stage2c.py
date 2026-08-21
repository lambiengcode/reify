#!/usr/bin/env python3
"""Stage 2: retrieval-augmented patch generation on SWE-bench Verified.

The SWE-bench paper's own protocol: one model, one budget, and the *retriever* is the
only thing that changes between arms. Arm `reify` fills the context from
`reify context`; arm `bm25` fills it from a lexical baseline over the same repository,
both truncated to the same character budget. The model returns SEARCH/REPLACE edits
(far more reliably applied than a hand-written unified diff), which are applied to the
tree so `git diff` produces the prediction the official harness will judge.

Usage: stage2b.py <repo_dir> <instances.jsonl> <out_dir>
"""
import json, math, pathlib, re, subprocess, sys, time
from collections import Counter

R = "/Users/lambiengcode/Documents/reify/projects/reify/target/release"
CTX_CHARS = 48_000  # ~12k tokens of retrieved code, per arm, identical for both

repo_dir = pathlib.Path(sys.argv[1]).resolve()
instances = [json.loads(l) for l in open(sys.argv[2])]
out_root = pathlib.Path(sys.argv[3]); out_root.mkdir(parents=True, exist_ok=True)

def sh(args, cwd=None, timeout=900):
    return subprocess.run(args, cwd=cwd, capture_output=True, text=True, timeout=timeout)

def reset(commit):
    sh(["git", "checkout", "-qf", commit], cwd=repo_dir)
    sh(["git", "clean", "-fdq", "-e", ".reify"], cwd=repo_dir)

def source_files():
    r = sh(["git", "ls-files", "*.py"], cwd=repo_dir)
    return [p for p in r.stdout.split("\n") if p]

def bm25_files(problem, k=8):
    """Classic BM25 over whole files — the SWE-bench paper's own baseline retriever."""
    terms = [t.lower() for t in re.findall(r"[A-Za-z_][A-Za-z0-9_]{2,}", problem)]
    if not terms:
        return []
    q = Counter(terms)
    docs, lens = {}, {}
    for path in source_files():
        try:
            text = (repo_dir / path).read_text(errors="ignore").lower()
        except OSError:
            continue
        toks = re.findall(r"[a-z_][a-z0-9_]{2,}", text)
        if toks:
            docs[path] = Counter(toks)
            lens[path] = len(toks)
    if not docs:
        return []
    N = len(docs)
    avg = sum(lens.values()) / N
    df = Counter()
    for path, tf in docs.items():
        for t in q:
            if t in tf:
                df[t] += 1
    k1, b = 1.5, 0.75
    scored = []
    for path, tf in docs.items():
        s = 0.0
        for t, qf in q.items():
            if t not in tf:
                continue
            idf = math.log(1 + (N - df[t] + 0.5) / (df[t] + 0.5))
            f = tf[t]
            s += idf * f * (k1 + 1) / (f + k1 * (1 - b + b * lens[path] / avg))
        if s > 0:
            scored.append((s, path))
    scored.sort(key=lambda x: (-x[0], x[1]))
    return [p for _, p in scored[:k]]

def reify_files(problem, k=8):
    r = sh([f"{R}/reify", "context", problem[:4000], "--budget", "4000", "--json"],
           cwd=repo_dir, timeout=300)
    if r.returncode:
        return []
    data = json.loads(r.stdout)
    seen, out = set(), []
    for item in data.get("next_reads", []) + data.get("code", []):
        p = item.get("path", "")
        if p and p not in seen and (repo_dir / p).is_file():
            seen.add(p); out.append(p)
    return out[:k]

def context_block(files):
    """Same character budget for both arms, so the model's cost is matched."""
    parts, used = [], 0
    for path in files:
        try:
            text = (repo_dir / path).read_text(errors="ignore")
        except OSError:
            continue
        if used + len(text) > CTX_CHARS:
            text = text[: max(0, CTX_CHARS - used)]
        if not text:
            break
        numbered = "\n".join(f"{i:5d}| {l}" for i, l in enumerate(text.split("\n"), 1))
        parts.append(f"### File: {path}\n```python\n{numbered}\n```")
        used += len(text)
        if used >= CTX_CHARS:
            break
    return "\n\n".join(parts), used

PROMPT = """You are fixing a bug in the {repo} repository.

## Issue
{problem}

## Repository files (every line is prefixed with its line number)
{context}

## Task
Produce the minimal source change that resolves the issue. Reply with ONLY edit blocks
in exactly this format, and nothing else:

@@EDIT path/to/file.py 120 124
(the replacement lines, WITHOUT line-number prefixes)
@@END

`120 124` means: replace lines 120 through 124 inclusive, as numbered above. Rules:
- Use the line numbers exactly as shown in the listing; do not guess at code you cannot
  see, and do not rely on memory of this project — this checkout may differ.
- Give the replacement with correct indentation and no line-number prefixes.
- To insert without deleting, repeat the existing line inside your replacement.
- Do not edit tests. Emit several blocks if several edits are needed."""

EDIT_RE = re.compile(
    r"@@EDIT\s+(?P<path>\S+)\s+(?P<start>\d+)\s+(?P<end>\d+)\s*\n(?P<body>.*?)@@END",
    re.S,
)

# Strip a line-number prefix if the model echoed one back despite the instruction.
NUM_PREFIX = re.compile(r"^\s*\d+\s*\|\s?")

def apply_edits(reply):
    """Apply line-range replacements, furthest-down first so earlier ranges stay valid."""
    edits = []
    for m in EDIT_RE.finditer(reply):
        path = repo_dir / m.group("path").strip()
        if not path.is_file():
            continue
        body = m.group("body")
        if body.endswith("\n"):
            body = body[:-1]
        lines = [NUM_PREFIX.sub("", l) for l in body.split("\n")]
        edits.append((path, int(m.group("start")), int(m.group("end")), lines))

    applied = 0
    for path in {e[0] for e in edits}:
        text = path.read_text(errors="ignore").split("\n")
        for _, s0, e0, repl in sorted((e for e in edits if e[0] == path),
                                      key=lambda e: e[1], reverse=True):
            if not (1 <= s0 <= e0 <= len(text)):
                continue
            text[s0 - 1:e0] = repl
            applied += 1
        path.write_text("\n".join(text))
    return applied

def ask(prompt):
    r = subprocess.run(["deepseek-axi", "ask", prompt], capture_output=True,
                       text=True, timeout=900)
    return r.stdout if r.returncode == 0 else ""

done_file = out_root / "done.txt"
done = set(done_file.read_text().split()) if done_file.exists() else set()
log = open(out_root / "driver.log", "a", buffering=1)

for i, inst in enumerate(sorted(instances, key=lambda r: r.get("created_at") or "")):
    iid = inst["instance_id"]
    if iid in done:
        continue
    t0 = time.time()
    try:
        reset(inst["base_commit"])
        sh([f"{R}/reify", "index", "-C", str(repo_dir)], timeout=2400)
        picks = {"reify": reify_files(inst["problem_statement"]),
                 "bm25": bm25_files(inst["problem_statement"])}
        stats = {}
        # Alternate which arm the provider sees first, so cache warmth cannot favour one.
        order = ["reify", "bm25"] if i % 2 == 0 else ["bm25", "reify"]
        for arm in order:
            reset(inst["base_commit"])
            ctx, used = context_block(picks[arm])
            prompt = PROMPT.format(repo=inst["repo"], context=ctx,
                                   problem=inst["problem_statement"][:8000])
            reply = ask(prompt)
            n = apply_edits(reply)
            if n == 0:
                # One retry, offered identically to both arms, so it cannot bias the
                # comparison; a model that quoted from memory usually recovers when
                # told that is what went wrong.
                reply = ask(prompt + "\n\nYour previous SEARCH block did not match the "
                            "file. Copy the lines exactly as they appear above.")
                n = apply_edits(reply)
            diff = sh(["git", "diff"], cwd=repo_dir).stdout
            with open(out_root / f"preds-{arm}.jsonl", "a") as f:
                f.write(json.dumps({"instance_id": iid,
                                    "model_name_or_path": f"deepseek+{arm}",
                                    "model_patch": diff}) + "\n")
            stats[arm] = (len(picks[arm]), used, n, len(diff))
        reset(inst["base_commit"])
        with open(done_file, "a") as f:
            f.write(iid + "\n")
        log.write(f"{iid} ok {time.time()-t0:.0f}s "
                  f"reify(files={stats['reify'][0]},edits={stats['reify'][2]},diff={stats['reify'][3]}) "
                  f"bm25(files={stats['bm25'][0]},edits={stats['bm25'][2]},diff={stats['bm25'][3]})\n")
    except Exception as e:
        log.write(f"{iid} ERROR {str(e)[:200]}\n")
print("stage2b worker done:", out_root)
