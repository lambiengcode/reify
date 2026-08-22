#!/usr/bin/env python3
"""Generate the 1280x640 link card shown when the repository is shared.

A link card is seen at thumbnail size in a feed, next to a title and description
GitHub already supplies. Repeating the description there wastes the one slot that can
carry something the text cannot: the measurement. So the card shows the headline
number and its control, and every number in it is read from the committed benchmark
results rather than typed here - a card that drifts from the README is worse than no
card, because it is the half people screenshot.

    python3 assets/make-social-preview.py            # static PNG and SVG
    python3 assets/make-social-preview.py --gif      # animated GIF as well

GitHub accepts an animated GIF for the social preview, so the bars grow and the
numbers count up. Two constraints shape how that is built. GitHub caps the image at
about 1 MB, and many places that unfurl a link render only the first frame. So frame
one is already the finished card with the bars at full length, and the animation runs
after it - a reader who never sees it animate still sees the whole result, and a
reader who does gets the bars drawn for them.

Needs `rsvg-convert` (brew install librsvg) to rasterise; GitHub's settings form takes
PNG, JPG or GIF, not SVG.
"""

import argparse
import re
import subprocess
import sys
import tempfile
from base64 import b64encode
from pathlib import Path

# Shared with make-logo.py. Kept as literals rather than imported because these two
# scripts are the palette's only two consumers and an import would invert which one
# owns it.
GREEN = "#3fb950"
INK = "#e6edf3"
MUTED = "#8b949e"
BACKGROUND = "#0d1117"
BAR_TRACK = "#21262d"   # the unfilled remainder, so the scale is visible
BAR_GREP = "#8b949e"    # the control bar: grey, but never the track colour, or it vanishes
RULE = "#30363d"

W, H = 1280, 640
MARGIN = 88
FONT = "'Helvetica Neue', Helvetica, 'Segoe UI', Arial, sans-serif"

# The card leads with SWE-bench Verified rather than this project's own benchmark:
# it is someone else's dataset, of real GitHub issues, and the index for each one is
# built at that issue's own base commit. A number a reader can check beats a bigger
# number they have to take on trust. Both arms get the same token budget.
SOURCE = Path("benchmarks/swe/results/stage1-retrieval.txt")
REIFY_ARM = "R-reify-iter3"
GREP_ARM = "B-content-grep"

FRAMES, HOLD_MS, STEP_MS = 22, 2600, 45


def hit_rates(root: Path) -> tuple[float, float, int, int, int]:
    """Read the headline rates out of the committed SWE-bench retrieval table.

    Returns the two hit rates, the instance count, and how many of the per-repository
    rows reify wins - the last of these being the answer to "was this one lucky
    repository?", which is the first thing a sceptical reader should ask.
    """
    text = (root / SOURCE).read_text()

    def arm(name: str) -> tuple[float, int]:
        m = re.search(rf"^{re.escape(name)}\s+([\d.]+)%.*?n=(\d+)", text, re.M)
        if not m:
            raise SystemExit(f"{SOURCE}: no {name} row to read")
        return float(m.group(1)), int(m.group(2))

    reify, n = arm(REIFY_ARM)
    grep, _ = arm(GREP_ARM)
    rows = re.findall(r"grep\s+(\d+)%\s+reify.\d\s+(\d+)%", text)
    if not rows:
        raise SystemExit(f"{SOURCE}: no per-repository rows to read")
    won = sum(1 for g, r in rows if int(r) > int(g))
    return reify, grep, n, won, len(rows)


def mark(x: int, y: int, size: int) -> str:
    """The mascot, embedded rather than linked.

    rsvg-convert resolves a relative href against its own working directory, not the
    SVG's, so a plain path renders here and silently vanishes when the card is built
    from anywhere else. Inlining it removes the question. The dark variant is the one
    used: the card background is near-black, and the mascot's own outline is black.
    """
    data = b64encode((Path(__file__).parent / "mascot-dark.png").read_bytes()).decode()
    return (f'  <image x="{x}" y="{y}" width="{size}" height="{size}" '
            f'xlink:href="data:image/png;base64,{data}"/>')


def bar(y: int, label: str, pct: float, shown: float, colour: str, emphasis: bool) -> str:
    """One horizontal bar. Widths are proportional to the rate, so the picture and the
    number cannot disagree. `shown` is the animated value; `pct` is the true one."""
    x, track = 700, 400
    filled = round(track * shown / 100)
    weight = "700" if emphasis else "500"
    text = f"{shown:.1f}%" if pct % 1 else f"{shown:.0f}%"
    return f"""
  <text x="{x}" y="{y - 16}" font-family="{FONT}" font-size="23" font-weight="{weight}"
        fill="{INK if emphasis else MUTED}">{label}</text>
  <rect x="{x}" y="{y}" width="{track}" height="38" rx="7" fill="{BAR_TRACK}"/>
  <rect x="{x}" y="{y}" width="{filled}" height="38" rx="7" fill="{colour}"/>
  <text x="{x + track + 20}" y="{y + 29}" font-family="{FONT}" font-size="32"
        font-weight="700" fill="{INK if emphasis else MUTED}">{text}</text>"""


def card(reify: float, grep: float, n: int, won: int, repos: int, t: float = 1.0) -> str:
    """The whole card at animation progress `t`, where 1.0 is the finished state."""
    ease = 1 - (1 - t) ** 3          # ease-out: quick to start, settles into the value
    return f"""<svg viewBox="0 0 {W} {H}" width="{W}" height="{H}"
     xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink"
     role="img"
     aria-label="Reify: on SWE-bench Verified it puts a file the fix touched in front of
     the model {reify}% of the time, against {grep}% for grep, over {n} real issues.">
  <rect width="{W}" height="{H}" fill="{BACKGROUND}"/>

{mark(MARGIN - 10, 56, 120)}
  <text x="{MARGIN + 122}" y="{74 + 54}" font-family="{FONT}" font-size="52"
        font-weight="700" letter-spacing="-1.5" fill="{INK}">reify</text>

  <text x="{MARGIN}" y="268" font-family="{FONT}" font-size="46" font-weight="700"
        letter-spacing="-1" fill="{INK}">The business logic is</text>
  <text x="{MARGIN}" y="326" font-family="{FONT}" font-size="46" font-weight="700"
        letter-spacing="-1" fill="{INK}">in one person's head.</text>
  <text x="{MARGIN}" y="396" font-family="{FONT}" font-size="46" font-weight="700"
        letter-spacing="-1" fill="{GREEN}">Reify compiles it out.</text>

  <text x="{MARGIN}" y="452" font-family="{FONT}" font-size="24" fill="{MUTED}">Local
  knowledge engine for AI coding agents</text>

  <text x="700" y="196" font-family="{FONT}" font-size="21" fill="{MUTED}">Found the file
  that had to change, same token budget</text>
{bar(240, "reify", reify, reify * ease, GREEN, True)}
{bar(344, "grep", grep, grep * ease, BAR_GREP, False)}
  <text x="700" y="432" font-family="{FONT}" font-size="20" fill="{MUTED}">SWE-bench
  Verified &#183; {n} real GitHub issues</text>
  <text x="700" y="461" font-family="{FONT}" font-size="20" fill="{MUTED}">Each index
  built at that issue's own base commit</text>
  <text x="700" y="490" font-family="{FONT}" font-size="20" fill="{MUTED}">reify wins on
  all {won} of {repos} repositories, not one lucky codebase</text>

  <rect x="{MARGIN}" y="536" width="{W - 2 * MARGIN}" height="1" fill="{RULE}"/>
  <text x="{MARGIN}" y="576" font-family="{FONT}" font-size="21" fill="{MUTED}">github.com/lambiengcode/reify</text>
  <text x="{W - MARGIN}" y="576" text-anchor="end" font-family="{FONT}" font-size="21"
        fill="{MUTED}">11 languages &#183; zero network calls &#183; Apache-2.0</text>
</svg>
"""


def rasterise(svg_text: str, out: Path) -> None:
    with tempfile.NamedTemporaryFile("w", suffix=".svg", delete=False) as f:
        f.write(svg_text)
        tmp = Path(f.name)
    try:
        run(["rsvg-convert", "-w", str(W), "-h", str(H), str(tmp), "-o", str(out)])
    finally:
        tmp.unlink(missing_ok=True)


def animate(numbers: tuple, out: Path) -> None:
    """Render the growth animation, finished frame first.

    Frame one is the completed card because most link unfurlers show only that frame;
    the animation is a bonus for the places that play it, never the way the result is
    delivered.
    """
    from PIL import Image

    with tempfile.TemporaryDirectory() as tmp:
        tmpdir = Path(tmp)
        steps = [1.0] + [i / (FRAMES - 1) for i in range(FRAMES)]
        frames, durations = [], []
        for i, t in enumerate(steps):
            png = tmpdir / f"f{i:03d}.png"
            rasterise(card(*numbers, t=t), png)
            frames.append(Image.open(png).convert("RGB"))
            durations.append(HOLD_MS if i in (0, len(steps) - 1) else STEP_MS)

        staged = tmpdir / "staged.gif"
        frames[0].save(staged, save_all=True, append_images=frames[1:],
                       duration=durations, loop=0, optimize=False)
        # Flat colour compresses hard, and only the bars change between frames, so
        # gifsicle's frame differencing keeps this inside GitHub's ~1 MB ceiling.
        run(["gifsicle", "-O3", "--lossy=40", "--colors", "64",
             str(staged), "-o", str(out)])


def run(cmd: list[str]) -> None:
    try:
        subprocess.run(cmd, check=True)
    except FileNotFoundError:
        sys.exit(f"{cmd[0]} not found (brew install librsvg gifsicle)")


def main() -> None:
    ap = argparse.ArgumentParser(description="Build the repository's link card.")
    ap.add_argument("--gif", action="store_true", help="also write an animated GIF")
    args = ap.parse_args()

    here = Path(__file__).parent
    root = here.parent
    numbers = hit_rates(root)

    svg = here / "social-preview.svg"
    svg.write_text(card(*numbers))
    rasterise(svg.read_text(), here / "social-preview.png")
    reify, grep, n, won, repos = numbers
    print(f"wrote social-preview.png ({reify}% vs {grep}% over {n}, "
          f"{won}/{repos} repositories, read from {SOURCE})")

    if args.gif:
        gif = here / "social-preview.gif"
        animate(numbers, gif)
        print(f"wrote {gif.name} ({gif.stat().st_size / 1024:.0f} KB)")


if __name__ == "__main__":
    main()
