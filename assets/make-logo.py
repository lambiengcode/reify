#!/usr/bin/env python3
"""Generate the Reify logo and mark, light and dark.

Written as a script rather than four hand-edited files so the palette lives in one
place: a colour changed in one variant and not the others is the usual way a logo set
drifts. Run `python3 assets/make-logo.py` after changing anything here.
"""

from pathlib import Path

# GitHub's own success greens, which is not a coincidence: these are the exact colours
# the benchmark charts already draw the reify bars in, so the logo wears the colour it
# was measured in. Each is chosen for contrast against its own background.
GREEN_LIGHT = "#2da44e"
GREEN_DARK = "#3fb950"

# The wordmark stays neutral. A green mark beside an ink wordmark reads at any size;
# an all-green lockup loses contrast on white, where the tagline sits right under it.
INK_LIGHT = "#111111"
INK_DARK = "#e6edf3"

DESC = (
    "Three rows of scattered fragments on the left - code, documents, history - become "
    "the same three rows aligned into one solid column on the right. Thin seams remain "
    "between them, because compiled knowledge keeps its sources traceable."
)

RATIONALE = """  <!--
    The mark is the word, drawn. The same three rows appear twice: loose and faint on
    the left, aligned and solid on the right. Nothing is added between them and nothing
    is lost - the seams stay, because Reify compiles knowledge without dissolving where
    it came from, and every claim it makes still carries its evidence.

    Green is the compiled half's colour and the fragments only borrow it, faintly: what
    Reify produces is the thing worth pointing at.
  -->"""

# (x, y, width) for each fragment, and the opacity ramp that carries the eye rightward.
FRAGMENTS = [
    (0, 0, 9, 0.16), (15, 0, 14, 0.34), (35, 0, 21, 0.58),
    (5, 36, 7, 0.16), (18, 36, 17, 0.36), (41, 36, 15, 0.58),
    (0, 72, 12, 0.18), (19, 72, 11, 0.34), (36, 72, 20, 0.58),
]
# The compiled model: the same three rows, aligned, solid, still separable.
COMPILED = [(72, 0, 56, 30), (72, 33, 56, 30), (72, 66, 56, 30)]


def mark_group(green: str, dx: int, dy: int) -> str:
    rows = [f'  <g transform="translate({dx} {dy})" fill="{green}">']
    for x, y, w, opacity in FRAGMENTS:
        rows.append(
            f'    <rect x="{x}" y="{y}" width="{w}" height="24" rx="2" opacity="{opacity}"/>'
        )
    for x, y, w, h in COMPILED:
        rows.append(f'    <rect x="{x}" y="{y}" width="{w}" height="{h}" rx="3"/>')
    rows.append("  </g>")
    return "\n".join(rows)


def logo(green: str, ink: str) -> str:
    return f"""<svg viewBox="0 0 566 140" xmlns="http://www.w3.org/2000/svg" role="img" aria-label="Reify">
  <title>Reify</title>
  <desc>{DESC}</desc>

{RATIONALE}
{mark_group(green, 26, 22)}

  <text x="188" y="93" font-family="-apple-system, 'Segoe UI', Helvetica, Arial, sans-serif"
        font-size="76" font-weight="700" letter-spacing="-2" fill="{ink}">reify</text>
</svg>
"""


def mark(green: str) -> str:
    return f"""<svg viewBox="0 0 180 180" xmlns="http://www.w3.org/2000/svg" role="img" aria-label="Reify">
  <title>Reify</title>
  <desc>The Reify mark: scattered fragments resolving into one aligned column.</desc>
{mark_group(green, 26, 42)}
</svg>
"""


def main() -> None:
    here = Path(__file__).parent
    written = {
        "logo.svg": logo(GREEN_LIGHT, INK_LIGHT),
        "logo-dark.svg": logo(GREEN_DARK, INK_DARK),
        "mark.svg": mark(GREEN_LIGHT),
        "mark-dark.svg": mark(GREEN_DARK),
    }
    for name, svg in written.items():
        (here / name).write_text(svg)
        print(f"wrote {name}")


if __name__ == "__main__":
    main()
