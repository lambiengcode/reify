# Assets

| File | What it is | How it's made |
|---|---|---|
| `logo.svg` / `logo-dark.svg` | Wordmark, light and dark | Hand-authored SVG |
| `mark.svg` / `mark-dark.svg` | Square mark, for avatars and favicons | Hand-authored SVG |
| `social-preview.png` | 1280×640 link card | Rendered from `mark-dark` + the tagline |
| `benchmark-agent.svg` | The headline chart | **Generated** by `reify-bench chart` |
| `benchmark-retrieval.svg` | Retrieval-only chart | **Generated** by `reify-bench chart` |

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
