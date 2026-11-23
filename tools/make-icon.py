"""Generate res/multilang.ico (multi-size) and a preview PNG. Run: python tools/make-icon.py (needs Pillow)."""
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parent.parent
FONT = "C:/Windows/Fonts/NotoSansKR-Bold.ttf"
BLUE = (37, 99, 235, 255)
WHITE = (255, 255, 255, 255)


def glyph(d, text, size, center):
    f = ImageFont.truetype(FONT, size)
    l, t, r, b = d.textbbox((0, 0), text, font=f)
    d.text((center[0] - (l + r) / 2, center[1] - (t + b) / 2), text, font=f, fill=WHITE)


def render(n):
    s = 4  # supersample
    big = Image.new("RGBA", (n * s, n * s), (0, 0, 0, 0))
    d = ImageDraw.Draw(big)
    N = n * s
    d.rounded_rectangle((0, 0, N - 1, N - 1), radius=int(N * 0.2), fill=BLUE)
    if n <= 24:
        glyph(d, "가", int(N * 0.78), (N / 2, N / 2))
    else:
        glyph(d, "가", int(N * 0.46), (N * 0.32, N * 0.33))
        glyph(d, "あ", int(N * 0.42), (N * 0.70, N * 0.70))
    return big.resize((n, n), Image.LANCZOS)


sizes = [16, 20, 24, 32, 40, 48, 64, 256]
imgs = [render(n) for n in sizes]
out = ROOT / "res" / "multilang.ico"
imgs[-1].save(out, sizes=[(n, n) for n in sizes], append_images=imgs[:-1])
prev = Image.new("RGBA", (sum(sizes[:-1]) + 10 * 8 + 128, 140), (240, 240, 240, 255))
x = 10
for im in imgs[:-1] + [imgs[-1].resize((128, 128), Image.LANCZOS)]:
    prev.paste(im, (x, 6), im)
    x += im.width + 10
prev.save(ROOT / "target" / "icon-preview.png")
print(out, out.stat().st_size)
