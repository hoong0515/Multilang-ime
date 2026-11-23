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


# --- input indicator icons: current language (가 / A / あ), auto = three dots, locked = padlock ---
FONT_JA = "C:/Windows/Fonts/YuGothB.ttc"


def indicator(n, ch, locked):
    s = 4
    N = n * s
    big = Image.new("RGBA", (N, N), (0, 0, 0, 0))
    d = ImageDraw.Draw(big)
    d.rounded_rectangle((0, 0, N - 1, N - 1), radius=int(N * 0.2), fill=BLUE)
    f = ImageFont.truetype(FONT_JA if ch == "あ" else FONT, int(N * 0.70))
    l, t, r, b = d.textbbox((0, 0), ch, font=f)
    cx, cy = N * 0.43, N * 0.48
    d.text((cx - (l + r) / 2, cy - (t + b) / 2), ch, font=f, fill=WHITE)
    if locked:
        # padlock at the right edge: shackle arc + body
        bx0, bx1 = N * 0.74, N * 0.97
        by0, by1 = N * 0.52, N * 0.86
        w = max(2, int(N * 0.05))
        d.arc((N * 0.78, N * 0.30, N * 0.93, N * 0.62), 180, 360, fill=WHITE, width=w)
        d.line((N * 0.78 + w / 2, N * 0.46, N * 0.78 + w / 2, by0), fill=WHITE, width=w)
        d.line((N * 0.93 - w / 2, N * 0.46, N * 0.93 - w / 2, by0), fill=WHITE, width=w)
        d.rounded_rectangle((bx0, by0, bx1, by1), radius=int(N * 0.04), fill=WHITE)
    else:
        r0 = N * 0.075
        for i, a in enumerate([255, 190, 120]):
            x, y = N * 0.86, N * 0.30 + i * N * 0.2
            d.ellipse((x - r0, y - r0, x + r0, y + r0), fill=(255, 255, 255, a))
    return big.resize((n, n), Image.LANCZOS)


ind_sizes = [16, 20, 24, 32, 48]
sheet = Image.new("RGBA", (6 * 60 + 10, 60), (32, 32, 32, 255))
for i, (code, ch) in enumerate([("ko", "가"), ("en", "A"), ("ja", "あ")]):
    for j, locked in enumerate([False, True]):
        ims = [indicator(n, ch, locked) for n in ind_sizes]
        name = f"lang-{code}-{'lock' if locked else 'auto'}.ico"
        ims[-1].save(ROOT / "res" / name, sizes=[(n, n) for n in ind_sizes], append_images=ims[:-1])
        prev = ims[2].resize((48, 48), Image.NEAREST)
        sheet.paste(prev, (10 + (i * 2 + j) * 60, 6), prev)
sheet.save(ROOT / "target" / "indicator-preview.png")
print("indicator icons written")
