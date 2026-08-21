# Assets

| File | What it is | How it's made |
|---|---|---|
| `logo.svg` / `logo-dark.svg` | Wordmark, light and dark | **Generated** by `make-logo.py` |
| `mark.svg` / `mark-dark.svg` | Square mark, for avatars and favicons | **Generated** by `make-logo.py` |
| `social-preview.png` | 1280×640 link card | Rendered from `mark-dark` + the tagline |
| `benchmark-agent.svg` | The headline chart | **Generated** by `reify-bench chart` |
| `benchmark-retrieval.svg` | Retrieval-only chart | **Generated** by `reify-bench chart` |

## Palette

| | light | dark |
|---|---|---|
| mark | `#2da44e` | `#3fb950` |
| wordmark | `#111111` | `#e6edf3` |

The greens are GitHub's own success colours, one per background — and not by
coincidence: they are the exact greens the benchmark charts already draw the reify bars
in, so the logo wears the colour it was measured in.

The wordmark stays neutral. A green mark beside an ink wordmark reads at any size; an
all-green lockup loses contrast on white, where the tagline sits directly beneath it.

Change any of this in `make-logo.py` and rerun it — the palette lives in one place
because a colour changed in one variant and not the others is the usual way a logo set
drifts apart:

```bash
python3 assets/make-logo.py
```

## The mark

Three rows of scattered fragments — code, documents, history — become the same three
rows aligned into one column. Nothing is added between them and nothing is lost: the
seams stay, because Reify compiles knowledge without dissolving where it came from.

Every colour is a neutral that stays legible on GitHub's light and dark backgrounds,
and the SVGs reference nothing external, since GitHub strips scripts and remote refs
from rendered markdown.

## Regenerating the charts

Never edit the chart SVGs by hand. They are generated from committed benchmark results
so they cannot drift from the data:

```bash
reify-bench chart \
  --results "ERPNext (Python/JS)=benchmarks/results/nolek-20260820" \
            "OpenMRS (Java)=benchmarks/results/openmrs-20260820" \
  --out assets/
```
