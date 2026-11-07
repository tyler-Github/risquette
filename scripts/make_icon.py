"""Generate the Risquette app icon (assets/risquette.ico and a PNG preview).

Run with Python 3 + Pillow:  python scripts/make_icon.py
"""
import os
from PIL import Image, ImageDraw, ImageFont

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, "assets")
BRAND_TOP = (139, 58, 255)
BRAND_BOTTOM = (90, 0, 205)
FONT_CANDIDATES = [r"C:\Windows\Fonts\segoeuib.ttf", r"C:\Windows\Fonts\arialbd.ttf", "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"]


def render(size: int) -> Image.Image:
    scale = 4  # supersample for smooth edges
    s = size * scale
    grad = Image.new("RGBA", (s, s))
    px = grad.load()
    for y in range(s):
        t = y / (s - 1)
        c = tuple(int(BRAND_TOP[i] + (BRAND_BOTTOM[i] - BRAND_TOP[i]) * t) for i in range(3))
        for x in range(s):
            px[x, y] = c + (255,)
    mask = Image.new("L", (s, s), 0)
    ImageDraw.Draw(mask).rounded_rectangle([0, 0, s - 1, s - 1], radius=int(s * 0.22), fill=255)
    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    img.paste(grad, (0, 0), mask)

    font_path = next(p for p in FONT_CANDIDATES if os.path.exists(p))
    font = ImageFont.truetype(font_path, int(s * 0.78))
    d = ImageDraw.Draw(img)
    box = d.textbbox((0, 0), "r", font=font)
    w, h = box[2] - box[0], box[3] - box[1]
    d.text(((s - w) / 2 - box[0] + s * 0.02, (s - h) / 2 - box[1] - s * 0.01), "r", font=font, fill=(255, 255, 255, 255))
    # A small accent dot, like a speech mark.
    r = s * 0.075
    cx, cy = s * 0.70, s * 0.70
    d.ellipse([cx - r, cy - r, cx + r, cy + r], fill=(255, 255, 255, 255))
    return img.resize((size, size), Image.LANCZOS)


def main() -> None:
    os.makedirs(OUT, exist_ok=True)
    sizes = [16, 24, 32, 48, 64, 128, 256]
    big = render(256)
    big.save(os.path.join(OUT, "risquette.png"))
    big.save(os.path.join(OUT, "risquette.ico"), sizes=[(n, n) for n in sizes], append_images=[render(n) for n in sizes[:-1]])
    print("wrote", os.path.join(OUT, "risquette.ico"))


if __name__ == "__main__":
    main()
