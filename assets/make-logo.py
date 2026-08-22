#!/usr/bin/env python3
"""Generate the Reify logo set from the mascot drawing.

`mascot.png` is the master and the only hand-made file here: a transparent, optically
centred drawing of the mascot. Everything else - the light and dark lockups, the icon
ladder, the favicon - is derived from it by this script, so the set cannot drift the
way four hand-edited files do.

    python3 assets/make-logo.py

Two things in here are corrections rather than decoration, and both were found by
rendering the result and looking at it:

  - The dark variant repaints the near-black outline to the light ink colour. A
    black-outlined mascot has no outline at all against a dark README; it reads as a
    green smudge.
  - That repaint deliberately spares the pupils. Flipping every dark pixel turns the
    black pupils the same cream as the eye-whites they sit in, and the mascot loses
    its gaze entirely - a defect invisible in the code and obvious the moment the dark
    lockup is rendered and looked at.
  - The lockup carries no tagline. The first draft had one, and rendering it showed
    both why not: it was nearly as wide as the wordmark itself, and every README that
    uses this puts its own tagline two lines underneath.
"""

from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter, ImageFont

HERE = Path(__file__).parent
MASTER = HERE / "mascot.png"

# The wordmark's rounded sans echoes the mascot's hand-drawn wobble; a grotesque beside
# it reads as two logos that happened to meet. First one present wins, so the script
# still runs off macOS - the committed PNGs are what ships, this only regenerates them.
FONTS = [
    "/System/Library/Fonts/Supplemental/Arial Rounded Bold.ttf",
    "/System/Library/Fonts/SFNSRounded.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
    "/Library/Fonts/Arial Bold.ttf",
]

INK_LIGHT = (17, 17, 17)
INK_DARK = (230, 237, 243)          # matches the charts' dark-mode text

# The largest thing derived from the master is a 512px icon and a 400px lockup, so
# anything past 1024 is weight with nothing to show for it.
MASTER_MAX = 1024
ICON_SIZES = (512, 256, 180, 128, 64, 48, 32, 16)
FAVICON_SIZES = [(16, 16), (32, 32), (48, 48)]


def font(size: int) -> ImageFont.FreeTypeFont:
    for path in FONTS:
        if Path(path).exists():
            return ImageFont.truetype(path, size)
    raise SystemExit(f"no usable font found; tried:\n  " + "\n  ".join(FONTS))


EYE_DILATE = 9   # px, enough to reach the pupil from the cream ring around it


def eye_mask(im: Image.Image) -> Image.Image:
    """Where the eyes are.

    Cream appears nowhere on this drawing except inside the eyes, so the eye-whites
    locate themselves; dilating that mask a little swallows the pupil sitting in the
    middle of them.
    """
    px = im.load()
    w, h = im.size
    mask = Image.new("L", (w, h), 0)
    mp = mask.load()
    for y in range(h):
        for x in range(w):
            r, g, b, a = px[x, y]
            if a > 128 and r > 205 and g > 200 and b > 175 and abs(r - b) < 60:
                mp[x, y] = 255
    return mask.filter(ImageFilter.MaxFilter(EYE_DILATE))


def flip_ink(im: Image.Image, to: tuple[int, int, int] = INK_DARK) -> Image.Image:
    """Repaint the near-black outline so it survives on a dark background.

    Blended rather than replaced, so the drawing's anti-aliased edge stays soft instead
    of turning into a hard stair-stepped line. Pixels inside the eyes are left alone,
    or the pupils disappear into the whites around them.
    """
    im = im.copy()
    px = im.load()
    eyes = eye_mask(im).load()
    w, h = im.size
    for y in range(h):
        for x in range(w):
            r, g, b, a = px[x, y]
            if a and r < 80 and g < 80 and b < 80 and not eyes[x, y]:
                k = max(r, g, b) / 80.0
                px[x, y] = (
                    int(to[0] * (1 - k) + r * k),
                    int(to[1] * (1 - k) + g * k),
                    int(to[2] * (1 - k) + b * k),
                    a,
                )
    return im


def compress(im: Image.Image, colours: int = 48) -> Image.Image:
    """Quantise to a small adaptive palette, keeping the alpha channel intact.

    The drawing is flat colour with anti-aliased edges, so a few dozen colours is
    visually lossless - and it also scrubs the JPEG ringing the source came with.
    Without this the master alone is over a megabyte, which is not a thing to commit.
    """
    alpha = im.getchannel("A")
    rgb = im.convert("RGB").quantize(colors=colours, method=Image.MEDIANCUT).convert("RGB")
    rgb.putalpha(alpha)
    return rgb


def lockup(mark: Image.Image, ink, path: Path, height: int = 400) -> None:
    """Mascot beside the wordmark, at 2x for retina; the READMEs display it at half.

    The wordmark is centred on the mascot's optical middle using its cap height rather
    than its full bounding box, so the descender of the y does not drag it low.
    """
    mark = mark.resize((height, height), Image.LANCZOS)
    word = font(int(height * 0.46))
    gap = int(height * 0.10)
    text_w = ImageDraw.Draw(Image.new("RGB", (1, 1))).textlength("reify", font=word)
    im = Image.new("RGBA", (int(height + gap + text_w + height * 0.05), height), (0, 0, 0, 0))
    im.paste(mark, (0, 0), mark)

    left, top, _, bottom = word.getbbox("reify")
    cap = word.getbbox("R")[3] - word.getbbox("R")[1]
    ImageDraw.Draw(im).text(
        (height + gap - left, height // 2 - cap // 2 - top), "reify", font=word, fill=ink)
    compress(im).save(path, optimize=True)


def main() -> None:
    if not MASTER.exists():
        raise SystemExit(f"{MASTER.name} missing: it is the hand-made master, not output")
    mascot = Image.open(MASTER).convert("RGBA")
    if mascot.width > MASTER_MAX:
        mascot = mascot.resize((MASTER_MAX, MASTER_MAX), Image.LANCZOS)

    compress(mascot).save(MASTER, optimize=True)
    dark = flip_ink(mascot)
    compress(dark).save(HERE / "mascot-dark.png", optimize=True)

    lockup(mascot, INK_LIGHT, HERE / "logo.png")
    lockup(dark, INK_DARK, HERE / "logo-dark.png")

    for size in ICON_SIZES:
        compress(mascot.resize((size, size), Image.LANCZOS)).save(
            HERE / f"icon-{size}.png", optimize=True)
    mascot.resize((256, 256), Image.LANCZOS).save(HERE / "favicon.ico", sizes=FAVICON_SIZES)

    for name in ("mascot.png", "mascot-dark.png", "logo.png", "logo-dark.png"):
        print(f"  {name:20} {(HERE / name).stat().st_size // 1024:>4} KB")
    print(f"  icon-*.png, favicon.ico")


if __name__ == "__main__":
    main()
