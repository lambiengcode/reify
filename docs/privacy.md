# Privacy and security

Reify indexes proprietary source code and internal business documents. This document
states what it does with them, and — more importantly — how each statement is
enforced, because a privacy claim is worth exactly as much as the check behind it.

## The guarantees

**1. Reify opens no network connection.**
Not "by default" — at all. There is no HTTP client in the dependency tree.
*Enforced by* `crates/reify/tests/offline.rs`, which fails the build if a networking
crate appears in `Cargo.lock` or if any source file references a socket type, and by
`deny.toml`, which bans those crates outright. CI additionally runs the whole suite
with outbound network blocked.

**2. Indexing and querying work entirely offline.**
Every command produces a useful answer with no network and no model. Deterministic
mode is a supported configuration, not a degraded one.

**3. Reify never executes anything from your repository.**
tree-sitter parses; it does not run. There is no plugin system, no `eval`, no build
step, no test execution. Exactly three external programs are invoked, each named in
the offline test so that adding a fourth is a reviewed act:

| Program | Why | Reaches the network? |
|---|---|---|
| `git` | History. Nothing else can produce it | No |
| `pdftotext` | PDF text extraction, absent a usable pure-Rust option | No |
| your model provider | Only if you configure one | **Yes, by your choice** |

**4. Model assistance is off, and there is no default provider.**
See below.

**5. The store never leaves your machine.**
`.reify/` is written into `.gitignore` by `reify init`.

## The model provider

Reify does not embed an HTTP client, because doing so would delete guarantee 1 for
every user including the majority who never enable a model. Instead the provider is
**a command you configure**:

```toml
# .reify/llm.toml
command = ["ollama", "run", "llama3"]
```

Reify writes the prompt to that command's stdin — or substitutes it for a `{prompt}`
argument — and reads the completion from stdout. Consequences, all deliberate:

- A local model works with no additional code.
- No credential ever passes through Reify.
- You can wrap the command to log, filter or refuse any request.
- `REIFY_OFFLINE=1` makes the provider unreachable regardless of configuration, and
  outranks the config file: someone who set the variable has stated an intent that
  beats a file they may have forgotten about.

Before anything is sent:

```bash
reify llm status                    # is a provider configured, and what would run
reify llm preview "<your task>"     # the exact bytes that would be sent. Nothing is sent
```

`reify llm preview` prints the identical string that would reach the provider's stdin —
not a summary of it. If those could differ, the promise would be unverifiable.

Every call is appended to `.reify/llm.log` with the input hash, byte counts, elapsed
time and outcome, including failures.

## What a model is allowed to do

Phrase. Not assert.

The synthesis prompt receives retrieved facts and is instructed to use only those. Its
output is recorded as `INFERRED` — [`llm::DERIVED_STATUS`] is a constant and there is
no code path to a stronger status — with the retrieved facts as its evidence.

## Threat model

| # | Risk | Status |
|---|---|---|
| T1 | Proprietary source reaches a model provider | Mitigated: off by default, previewable, logged, `REIFY_OFFLINE=1` |
| T2 | Reify phones home | **Eliminated**: no networking crate can enter the tree |
| T3 | Secrets swept into the store | **Partly mitigated**: the store holds identifiers, spans and hashes rather than file contents, but a secret in a symbol name or docstring would be indexed. Secret-pattern redaction is not yet implemented |
| T4 | `.reify/` committed to a public repo | Mitigated: gitignored at `init` |
| T5 | Malicious repo content executes during indexing | **Eliminated**: nothing from the repository is executed |
| T6 | Prompt injection through repository content | **Partly mitigated**: output is structurally typed and every claim carries a status, so a `.md` file saying "ignore previous instructions" enters as a `DocSection` and cannot become an instruction field. Reify does not yet scan for injection patterns |
| T7 | Store tampering by a local attacker | **Out of scope.** An attacker who can write `.reify/store.db` can feed your agent false claims. Defending against local write access is not attempted |
| T8 | Cross-tenant leakage in a monorepo | Mitigated: `.reifyignore` is honoured |

T3, T6 and T7 are stated as open rather than solved. A threat model that claims total
coverage is not a threat model.

## Reporting a problem

Open an issue for anything non-sensitive. For a vulnerability, please report privately
before disclosing.
