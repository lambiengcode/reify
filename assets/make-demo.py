#!/usr/bin/env python3
"""Turn a raw screen-capture recording into the README's demo GIF.

The demo is recorded with `termgif --terminal`, which screen-captures a real terminal
window instead of drawing a simulated one. That is the only mode that produces COLOUR:
termgif's normal recorder forces NO_COLOR=1 and TERM=dumb onto every command and
captures through a pipe, so reify - which colours only when a terminal is attached -
prints plain text. Screen-capture mode runs each command with a real TTY, so
`[confirmed]` is green and `[inferred]` amber, as a user actually sees them.

The cost of that mode is that the frame contains the host terminal's own chrome: a
title bar carrying "screencapture < run2.sh - 104x32" and a tab bar. Terminal.app
appends the running process to the window title no matter which title flags are set,
so this script crops the chrome off and draws the header itself. That also makes the
header independent of whoever's terminal recorded it.

    python3 assets/make-demo.py <raw.gif> [-o assets/demo.gif]

Needs ffmpeg and gifsicle. See assets/README.md for the recording procedure.
"""

import argparse
import subprocess
import sys
import tempfile
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

HERE = Path(__file__).parent

# Rows 0..CHROME are the host terminal's title and tab bars; PAD trims the window's
# rounded border, which would otherwise leave grey corner crumbs after cropping.
CHROME, PAD = 136, 4
BAR = 76                      # height of the header this script draws instead
BG = (40, 42, 54)             # Dracula base, matching the recorded terminal
RULE = (58, 61, 76)
LABEL = (226, 232, 240)
LIGHTS = [(255, 95, 87), (254, 188, 46), (40, 200, 64)]

FONTS = [
    "/System/Library/Fonts/Supplemental/Arial Rounded Bold.ttf",
    "/System/Library/Fonts/SFNSRounded.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
]

# 64, not 32: the frame is almost all monochrome text, so a small palette spends
# itself on antialiasing greys and quantises the few coloured pixels - the status
# tags and the window dots - to grey. That is the whole point of this recording.
COLOURS, FPS, WIDTH, LOSSY = 64, 4, 860, 120


def font(size: int) -> ImageFont.FreeTypeFont:
    for path in FONTS:
        if Path(path).exists():
            return ImageFont.truetype(path, size)
    raise SystemExit("no usable font found; tried:\n  " + "\n  ".join(FONTS))


def restyle(frame: Image.Image, mascot: Image.Image, face: ImageFont.FreeTypeFont) -> Image.Image:
    """Crop the host terminal's chrome and draw our own header in its place."""
    body = frame.crop((PAD, CHROME, frame.width - PAD, frame.height - PAD))
    out = Image.new("RGB", (body.width, BAR + body.height), BG)
    out.paste(body, (0, BAR))

    draw = ImageDraw.Draw(out)
    for i, colour in enumerate(LIGHTS):
        x = 30 + i * 38
        draw.ellipse([x, BAR // 2 - 11, x + 22, BAR // 2 + 11], fill=colour)

    text_w = draw.textlength("reify", font=face)
    start = int(out.width / 2 - (mascot.width + 10 + text_w) / 2)
    out.paste(mascot, (start, BAR // 2 - mascot.height // 2), mascot)
    draw.text((start + mascot.width + 10, BAR // 2 - 19), "reify", font=face, fill=LABEL)
    draw.line([(0, BAR), (out.width, BAR)], fill=RULE, width=2)
    return out


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("raw", type=Path, help="raw recording from termgif --terminal")
    ap.add_argument("-o", "--output", type=Path, default=HERE / "demo.gif")
    args = ap.parse_args()

    src = Image.open(args.raw)
    mascot = Image.open(HERE / "icon-64.png").convert("RGBA").resize((46, 46), Image.LANCZOS)
    face = font(30)

    frames, durations = [], []
    for i in range(src.n_frames):
        src.seek(i)
        frames.append(restyle(src.convert("RGB"), mascot, face))
        durations.append(src.info.get("duration", 125))

    with tempfile.TemporaryDirectory() as tmp:
        staged = Path(tmp) / "staged.gif"
        frames[0].save(staged, save_all=True, append_images=frames[1:],
                       duration=durations, loop=0, optimize=False)
        palette = Path(tmp) / "pal.png"
        scaled = Path(tmp) / "scaled.gif"
        chain = f"fps={FPS},scale={WIDTH}:-1:flags=lanczos"
        run(["ffmpeg", "-v", "error", "-y", "-i", str(staged),
             "-vf", f"{chain},palettegen=max_colors={COLOURS}:stats_mode=diff", str(palette)])
        run(["ffmpeg", "-v", "error", "-y", "-i", str(staged), "-i", str(palette),
             "-lavfi", f"{chain}[x];[x][1:v]paletteuse=dither=none:diff_mode=rectangle",
             str(scaled)])
        run(["gifsicle", "-O3", f"--lossy={LOSSY}", str(scaled), "-o", str(args.output)])

    size = args.output.stat().st_size / 1024 / 1024
    final = Image.open(args.output)
    print(f"{args.output.name}: {final.size[0]}x{final.size[1]}, "
          f"{final.n_frames} frames, {size:.1f} MB")


def run(cmd: list[str]) -> None:
    try:
        subprocess.run(cmd, check=True)
    except FileNotFoundError:
        sys.exit(f"{cmd[0]} not found (brew install ffmpeg gifsicle)")


if __name__ == "__main__":
    main()
