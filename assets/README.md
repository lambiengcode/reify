# Assets

| File | What it is | How it's made |
|---|---|---|
| `logo.svg` / `logo-dark.svg` | Wordmark, light and dark | **Generated** by `make-logo.py` |
| `mark.svg` / `mark-dark.svg` | Square mark, for avatars and favicons | **Generated** by `make-logo.py` |
| `social-preview.png` / `.svg` | 1280×640 link card | **Generated** by `make-social-preview.py` |
| `demo.gif` / `demo.tg` | The README's feature tour | **Recorded** by `termgif record assets/demo.tg` |
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

The link card reads its two headline percentages out of
`benchmarks/results/nolek-eval/agent-summary.json` rather than having them typed in,
for the same reason the charts are generated: the card is the half people screenshot,
so it must not be able to drift from the results. Regenerate it after a new benchmark
run:

```bash
python3 assets/make-social-preview.py   # needs rsvg-convert (brew install librsvg)
```

Setting it on the repository is a manual step — GitHub exposes no API for the social
preview, only the image picker in **Settings → General → Social preview**.

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

## Regenerating the demo

```bash
pip install termgif numpy pillow          # numpy/pillow are needed by the GIF exporter
cd /path/to/an/indexed/repository          # the tape's commands run here, for real
PATH=/path/to/reify/target/release:$PATH termgif record /path/to/assets/demo.tg

# The raw render is ~34 MB. Re-time and quantise it before it touches the README.
ffmpeg -i demo.gif -vf "fps=4,scale=860:-1:flags=lanczos,palettegen=max_colors=32:stats_mode=diff" pal.png
ffmpeg -i demo.gif -i pal.png \
  -lavfi "fps=4,scale=860:-1:flags=lanczos[x];[x][1:v]paletteuse=dither=none:diff_mode=rectangle" ff.gif
gifsicle -O3 --lossy=200 ff.gif -o assets/demo.gif
```

The tape is a **feature tour**, not a story: `index`, `context`, `why`, `impact`,
`explain`, and `context --toon`, each run for real against a real ERPNext index. Record
from inside the indexed repository — `@cwd` is not honoured by every termgif build, and
a recording made in the wrong directory silently produces a demo where every command
answers "nothing indexed".

## Regenerating the charts

Never edit the chart SVGs by hand. They are generated from committed benchmark results
so they cannot drift from the data:

```bash
reify-bench chart \
  --results "ERPNext (Python/JS)=benchmarks/results/nolek-20260820" \
            "OpenMRS (Java)=benchmarks/results/openmrs-20260820" \
  --out assets/
```
