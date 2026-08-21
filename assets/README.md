# Assets

| File | What it is | How it's made |
|---|---|---|
| `logo.svg` / `logo-dark.svg` | Wordmark, light and dark | **Generated** by `make-logo.py` |
| `mark.svg` / `mark-dark.svg` | Square mark, for avatars and favicons | **Generated** by `make-logo.py` |
| `social-preview.png` / `.svg` | 1280×640 link card | **Generated** by `make-social-preview.py` |
| `demo.gif` | The README's terminal demo | **Recorded** by terminalizer from `demo-script.sh` + `terminalizer.yml` |
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
# terminalizer needs Node 20 (its node-pty dependency has no prebuilt for Node 24),
# and a real TTY, which `script` provides in a non-interactive shell.
nvm use 20 && npm i -g terminalizer
export REIFY_DEMO_REPO=/path/to/an/indexed/erpnext
export REIFY_DEMO_BIN=target/release
script -q /dev/null terminalizer record demo --config assets/terminalizer.yml --skip-sharing
script -q /dev/null terminalizer render demo -o demo-raw.gif

# The raw render is huge; re-time and compress it before it touches the README.
ffmpeg -i demo-raw.gif -vf "fps=7,scale=1062:-1:flags=lanczos,palettegen=max_colors=96" pal.png
ffmpeg -i demo-raw.gif -i pal.png \
  -lavfi "fps=7,scale=1062:-1:flags=lanczos[x];[x][1:v]paletteuse=dither=bayer:bayer_scale=4" demo-ff.gif
gifsicle -O3 --lossy=90 demo-ff.gif -o assets/demo.gif
```

Every command in `demo-script.sh` runs for real against the named index — the same
generated-not-drawn rule the charts follow. If the GIF ever disagrees with the tool,
re-record the GIF, not the README.

## Regenerating the charts

Never edit the chart SVGs by hand. They are generated from committed benchmark results
so they cannot drift from the data:

```bash
reify-bench chart \
  --results "ERPNext (Python/JS)=benchmarks/results/nolek-20260820" \
            "OpenMRS (Java)=benchmarks/results/openmrs-20260820" \
  --out assets/
```
