# Assets

| File | What it is | How it's made |
|---|---|---|
| `mascot.png` | The mascot drawing — the only hand-made file here | **Hand-made**; everything under it is derived |
| `mascot-dark.png` | Mascot with a light outline, for dark backgrounds | **Generated** by `make-logo.py` |
| `logo.png` / `logo-dark.png` | Horizontal lockup, light and dark | **Generated** by `make-logo.py` |
| `icon-*.png`, `favicon.ico` | Icon ladder, 512px down to 16px | **Generated** by `make-logo.py` |
| `social-preview.gif` / `.png` / `.svg` | 1280×640 link card, animated | **Generated** by `make-social-preview.py --gif` |
| `demo.gif` / `demo.tape` | The README's feature tour | **Recorded** by `vhs`, then re-timed and quantised |
| `demo-shell/.zshrc` | The prompt and highlighting the demo records with | Hand-written; sourced by the tape |
| `benchmark-agent.svg` | The headline chart | **Generated** by `reify-bench chart` |
| `benchmark-retrieval.svg` | Retrieval-only chart | **Generated** by `reify-bench chart` |

## Palette

| | light | dark |
|---|---|---|
| mascot | `#2da44e` | `#2da44e` |
| outline | `#111111` | `#e6edf3` |
| wordmark | `#111111` | `#e6edf3` |

The green is GitHub's own success colour, and not by coincidence: it is the green the
benchmark charts already draw the reify bars in, so the logo wears the colour it was
measured in.

The outline flips on dark. A black-outlined mascot has no outline at all against a dark
README — it reads as a green smudge — so `make-logo.py` repaints the near-black line to
the light ink colour, blending rather than replacing it so the drawn edge stays soft.

The wordmark stays neutral. A green mascot beside an ink wordmark reads at any size; an
all-green lockup loses contrast on white.

The link card reads its headline percentages out of
`benchmarks/swe/results/stage1-retrieval.txt` rather than having them typed in, for the
same reason the charts are generated: the card is the half people screenshot, so it must
not be able to drift from the results. It leads with SWE-bench Verified rather than this
project's own benchmark, because a number measured on someone else's dataset of real
issues is worth more than a larger number a reader has to take on trust. Regenerate it
after a new benchmark run:

```bash
python3 assets/make-social-preview.py --gif   # needs rsvg-convert and gifsicle
```

GitHub accepts an animated GIF here, so the bars grow and the numbers count up. Two
things shape how that is built. GitHub caps the image near 1 MB, and many services that
unfurl a link render only the first frame — so **frame one is the finished card**, with
the bars already at full length, and the animation replays after it. A reader who never
sees it move still sees the whole result.

Setting it on the repository is a manual step — GitHub exposes no API for the social
preview, only the image picker in **Settings → General → Social preview**. Upload
`social-preview.gif` for the animated card, or `social-preview.png` for a static one.

Change any of this in `make-logo.py` and rerun it — the palette lives in one place
because a colour changed in one variant and not the others is the usual way a logo set
drifts apart. It also re-quantises the master in place, which is what keeps a flat
drawing from committing as a megabyte:

```bash
python3 assets/make-logo.py
```

## The mascot

A small green thing that has just finished pulling itself together, with the grey puffs
it condensed out of still hanging in the air beside it. That is the word drawn: to
reify is to make a scattered, formless thing concrete. It is unbothered, because the
work it does is the boring, mechanical kind that only looks impressive from outside.

It was picked over six other drawings on one test rather than on taste: shrink each
candidate to 64, 32 and 16 pixels and see which ones survive. A mole, an owl and an
archaeologist all dissolved into mush below 48px — they were illustrations, not marks.
This one keeps a single readable silhouette the whole way down, which is why the
concept lives in the puffs, where losing them at favicon size costs nothing.

`mascot.png` is the master and the only file here drawn by hand. The lockups, the dark
variants and the whole icon ladder come out of `make-logo.py`, so the set cannot drift
the way four separately hand-edited files do.

## Regenerating the demo

Recorded with [vhs](https://github.com/charmbracelet/vhs), which runs each command in
a real terminal. That is what makes the output **coloured**: reify prints colour only
when a terminal is attached, so any recorder that captures through a pipe — termgif's
ordinary mode among them, which additionally forces `NO_COLOR=1` and `TERM=dumb` —
gets flat text and nothing in the tape can change that.

The shell is zsh, configured by `demo-shell/.zshrc`: a green prompt, and the command
word highlighted green as it is typed. That highlighting is a dozen lines of `zle`
rather than a syntax-highlighting plugin, so recording the demo does not depend on
what the recorder happens to have installed. The tape sources it **explicitly** rather
than through `ZDOTDIR`, because vhs installs its own prompt after the shell starts and
would otherwise overwrite it.

```bash
brew install vhs
cd /path/to/an/indexed/repository        # the tape's commands run here, for real
REIFY_ASSETS=/path/to/reify/assets \
  PATH=/path/to/reify/target/release:$PATH \
  vhs $REIFY_ASSETS/demo.tape -o /tmp/raw.gif
```

The raw render is ~1.4 MB. Re-time and quantise it before it touches the README. Use
64 colours, not 32: the frame is almost entirely monochrome text, so a small palette
spends itself on antialiasing greys and quantises away the coloured status tags, which
are the point of recording this way at all.

```bash
ffmpeg -i /tmp/raw.gif -vf "fps=8,scale=920:-1:flags=lanczos,palettegen=max_colors=64:stats_mode=diff" /tmp/pal.png
ffmpeg -i /tmp/raw.gif -i /tmp/pal.png \
  -lavfi "fps=8,scale=920:-1:flags=lanczos[x];[x][1:v]paletteuse=dither=none:diff_mode=rectangle" /tmp/ff.gif
gifsicle -O3 --lossy=80 /tmp/ff.gif -o assets/demo.gif
```

The tape is a **feature tour**, not a story: `index`, `context`, `why`, `impact`,
`explain`, and `context --toon`, each run for real against a real ERPNext index. Record
from inside the indexed repository — a recording made in the wrong directory silently
produces a demo where every command answers "nothing indexed".

## Regenerating the charts

Never edit the chart SVGs by hand. They are generated from committed benchmark results
so they cannot drift from the data:

```bash
reify-bench chart \
  --results "ERPNext (Python/JS)=benchmarks/results/nolek-20260820" \
            "OpenMRS (Java)=benchmarks/results/openmrs-20260820" \
  --out assets/
```
