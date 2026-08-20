# ADR 0002 — The model provider is an external command

**Status:** accepted · **Date:** 2026-08-20

## Context

The plan called for optional LLM assistance with a configurable provider and local-model
support. The obvious implementation is an HTTP client behind a feature flag.

## Decision

No HTTP client. The provider is a command the user configures; Reify writes the prompt
to its stdin, or substitutes a `{prompt}` argument, and reads stdout.

## Why

A feature-flagged HTTP client still puts networking crates in `Cargo.lock`, which
would defeat `tests/offline.rs` — the test that makes the privacy claim verifiable
rather than merely stated. Deleting a guarantee for every user, including the majority
who never enable a model, to serve a minority feature is the wrong trade.

The external command additionally:

- makes local models (`ollama run`, `llama-cli`) work with no extra code;
- keeps every credential out of Reify;
- lets a user wrap the command to log, filter or refuse requests;
- makes `reify llm preview` trivially truthful — it prints the exact bytes.

## Consequences

- Users must have a CLI for their provider. For a hosted API that means a three-line
  wrapper script, which is the cost of the guarantee.
- No streaming, and no token-usage counts. The benchmark therefore reports *estimated*
  prompt tokens and says so.
- `Command::new(program)` appears in the source, so the offline test allowlists it with
  an explanation rather than banning subprocesses outright.
