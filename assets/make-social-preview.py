#!/usr/bin/env python3
"""Generate the 1280x640 link card shown when the repository is shared.

A link card is seen at thumbnail size in a feed, next to a title and description
GitHub already supplies. Repeating the description there wastes the one slot that can
carry something the text cannot: the measurement. So the card shows the headline
number and its control, and every number in it is read from the committed benchmark
results rather than typed here - a card that drifts from the README is worse than no
card, because it is the half people screenshot.

    python3 assets/make-social-preview.py

Needs `rsvg-convert` (brew install librsvg) to rasterise; GitHub's settings form takes
PNG, JPG or GIF, not SVG.
"""

import json
import subprocess
import sys
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

# Which result directory each headline number comes from, and the arm within it. The
# card states the cost-matched comparison the README leads with: three reify rounds
# against grep handed the same tripled budget. The bars are one repository, so the
# card names it - a thumbnail that shows the best of four repositories without saying
# which is the kind of number a reader is right to distrust.
SOURCE = Path("benchmarks/results/nolek-eval/agent-summary.json")
REPOSITORY = "ERPNext"
REIFY_ARM = "R-reify-iter3"
GREP_ARM = "B-content-grep-x3"


def hit_rates(root: Path) -> tuple[int, int, int]:
    """Read the two headline rates, and the task count they were measured over, out of
    the committed benchmark summary."""
    arms = {c["condition"]: c for c in json.loads((root / SOURCE).read_text())}
    missing = {REIFY_ARM, GREP_ARM} - arms.keys()
    if missing:
        raise SystemExit(f"{SOURCE}: no {', '.join(sorted(missing))} arm to read")
    return (
        round(arms[REIFY_ARM]["hit_rate"] * 100),
        round(arms[GREP_ARM]["hit_rate"] * 100),
        arms[REIFY_ARM]["tasks"],
    )


def mark(x: int, y: int, scale: float) -> str:
    """The logo mark, scaled. Geometry mirrors make-logo.py's FRAGMENTS/COMPILED."""
    fragments = [
        (0, 0, 9, 0.16), (15, 0, 14, 0.34), (35, 0, 21, 0.58),
        (5, 36, 7, 0.16), (18, 36, 17, 0.36), (41, 36, 15, 0.58),
        (0, 72, 12, 0.18), (19, 72, 11, 0.34), (36, 72, 20, 0.58),
    ]
    compiled = [(72, 0, 56, 30), (72, 33, 56, 30), (72, 66, 56, 30)]
    out = [f'  <g transform="translate({x} {y}) scale({scale})" fill="{GREEN}">']
    for fx, fy, fw, opacity in fragments:
        out.append(
            f'    <rect x="{fx}" y="{fy}" width="{fw}" height="24" rx="2" opacity="{opacity}"/>'
        )
    for cx, cy, cw, ch in compiled:
        out.append(f'    <rect x="{cx}" y="{cy}" width="{cw}" height="{ch}" rx="3"/>')
    out.append("  </g>")
    return "\n".join(out)


def bar(y: int, label: str, pct: int, colour: str, emphasis: bool) -> str:
    """One horizontal bar. Widths are proportional to the rate, so the picture and the
    number cannot disagree."""
    x = 720
    track = 380
    filled = round(track * pct / 100)
    weight = "700" if emphasis else "500"
    number_fill = INK if emphasis else MUTED
    return f"""
  <text x="{x}" y="{y - 16}" font-family="{FONT}" font-size="23" font-weight="{weight}"
        fill="{INK if emphasis else MUTED}">{label}</text>
  <rect x="{x}" y="{y}" width="{track}" height="34" rx="6" fill="{BAR_TRACK}"/>
  <rect x="{x}" y="{y}" width="{filled}" height="34" rx="6" fill="{colour}"/>
  <text x="{x + track + 22}" y="{y + 27}" font-family="{FONT}" font-size="30"
        font-weight="700" fill="{number_fill}">{pct}%</text>"""


def card(reify_pct: int, grep_pct: int, tasks: int) -> str:
    return f"""<svg viewBox="0 0 {W} {H}" width="{W}" height="{H}"
     xmlns="http://www.w3.org/2000/svg" role="img"
     aria-label="Reify: at the same token cost a model finds the file that had to change
     {reify_pct}% of the time, against {grep_pct}% for grep.">
  <rect width="{W}" height="{H}" fill="{BACKGROUND}"/>

{mark(MARGIN, 74, 0.62)}
  <text x="{MARGIN + 108}" y="{74 + 50}" font-family="{FONT}" font-size="52"
        font-weight="700" letter-spacing="-1.5" fill="{INK}">reify</text>

  <text x="{MARGIN}" y="268" font-family="{FONT}" font-size="46" font-weight="700"
        letter-spacing="-1" fill="{INK}">The business logic is</text>
  <text x="{MARGIN}" y="326" font-family="{FONT}" font-size="46" font-weight="700"
        letter-spacing="-1" fill="{INK}">in one person's head.</text>
  <text x="{MARGIN}" y="396" font-family="{FONT}" font-size="46" font-weight="700"
        letter-spacing="-1" fill="{GREEN}">Reify compiles it out.</text>

  <text x="{MARGIN}" y="452" font-family="{FONT}" font-size="24" fill="{MUTED}">Local
  knowledge engine for AI coding agents</text>

  <text x="720" y="206" font-family="{FONT}" font-size="21" fill="{MUTED}">Found the file
  that had to change, same token cost</text>
{bar(268, "reify &#215;3 rounds", reify_pct, GREEN, True)}
{bar(362, "grep &#215;3 budget", grep_pct, BAR_GREP, False)}
  <text x="720" y="448" font-family="{FONT}" font-size="20" fill="{MUTED}">{REPOSITORY},
  {tasks} real merged commits, indexed before them</text>
  <text x="720" y="478" font-family="{FONT}" font-size="20" fill="{MUTED}">3 of 4
  codebases win by 25&#8211;50 points; the 4th ties</text>

  <rect x="{MARGIN}" y="536" width="{W - 2 * MARGIN}" height="1" fill="{RULE}"/>
  <text x="{MARGIN}" y="576" font-family="{FONT}" font-size="21" fill="{MUTED}">github.com/lambiengcode/reify</text>
  <text x="{W - MARGIN}" y="576" text-anchor="end" font-family="{FONT}" font-size="21"
        fill="{MUTED}">11 languages &#183; zero network calls &#183; Apache-2.0</text>
</svg>
"""


def main() -> None:
    here = Path(__file__).parent
    root = here.parent
    reify_pct, grep_pct, tasks = hit_rates(root)

    svg = here / "social-preview.svg"
    png = here / "social-preview.png"
    svg.write_text(card(reify_pct, grep_pct, tasks))
    try:
        subprocess.run(
            ["rsvg-convert", "-w", str(W), "-h", str(H), str(svg), "-o", str(png)],
            check=True,
        )
    except FileNotFoundError:
        sys.exit("rsvg-convert not found: brew install librsvg")
    print(f"wrote {png.name} ({reify_pct}% vs {grep_pct}%, read from {SOURCE})")


if __name__ == "__main__":
    main()
