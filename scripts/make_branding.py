"""Generate the images in branding/ from the icon and the app screenshots.

    python scripts/make_branding.py

Inputs:  branding/screenshots/app-dark.png, app-light.png (window captures)
Outputs: branding/banner-dark.png, banner-light.png   README header
         branding/social-preview.png                   GitHub social preview (1280x640)
         branding/logo-{64,128,256,512}.png            app logo
         branding/headers/<doc>-{dark,light}.png       header for each page in docs/
         branding/framed/<shot>.png                    screenshots with rounded corners and a shadow
Also refreshes assets/risquette.ico and assets/risquette.png (see make_icon.py).
"""
import os
import sys

from PIL import Image, ImageDraw, ImageFilter, ImageFont

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import make_icon  # noqa: E402

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BRANDING = os.path.join(ROOT, "branding")
SHOTS = os.path.join(BRANDING, "screenshots")
BRAND = (107, 2, 242)
FONT_BOLD = [r"C:\Windows\Fonts\segoeuib.ttf", r"C:\Windows\Fonts\arialbd.ttf", "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"]
FONT_REGULAR = [r"C:\Windows\Fonts\segoeui.ttf", r"C:\Windows\Fonts\arial.ttf", "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"]
FONT_SEMIBOLD = [r"C:\Windows\Fonts\seguisb.ttf"] + FONT_BOLD

THEMES = {
    "dark": {"bg": (14, 9, 28), "glow": (107, 2, 242), "title": (255, 255, 255), "text": (196, 188, 222), "pill_bg": (255, 255, 255, 22), "pill_fg": (225, 218, 250)},
    "light": {"bg": (248, 246, 255), "glow": (150, 90, 255), "title": (24, 16, 44), "text": (84, 76, 110), "pill_bg": (107, 2, 242, 22), "pill_fg": (80, 20, 180)},
}


def font(candidates, size):
    return ImageFont.truetype(next(p for p in candidates if os.path.exists(p)), size)


def trim_black(img):
    """Drop the invisible black frame PrintWindow leaves around a window."""
    rgb = img.convert("RGB")
    w, h = rgb.size

    def mostly_black(pixels):
        pixels = list(pixels)
        return sum(p == (0, 0, 0) for p in pixels) >= 0.8 * len(pixels)

    def black_row(y):
        return mostly_black(rgb.getpixel((x, y)) for x in range(0, w, max(1, w // 50)))

    def black_col(x):
        return mostly_black(rgb.getpixel((x, y)) for y in range(0, h, max(1, h // 50)))

    top, bottom, left, right = 0, h, 0, w
    while top < h - 1 and black_row(top):
        top += 1
    while bottom > top + 1 and black_row(bottom - 1):
        bottom -= 1
    while left < w - 1 and black_col(left):
        left += 1
    while right > left + 1 and black_col(right - 1):
        right -= 1
    return img.crop((left, top, right, bottom))


def rounded(img, radius):
    mask = Image.new("L", img.size, 0)
    ImageDraw.Draw(mask).rounded_rectangle([0, 0, img.size[0] - 1, img.size[1] - 1], radius=radius, fill=255)
    out = Image.new("RGBA", img.size, (0, 0, 0, 0))
    out.paste(img.convert("RGBA"), (0, 0), mask)
    return out


def background(size, theme):
    w, h = size
    bg = Image.new("RGBA", size, theme["bg"] + (255,))
    glow = Image.new("RGBA", size, (0, 0, 0, 0))
    d = ImageDraw.Draw(glow)
    g = theme["glow"]
    d.ellipse([int(w * 0.48), int(-h * 0.5), int(w * 1.15), int(h * 1.1)], fill=g + (120,))
    d.ellipse([int(-w * 0.2), int(h * 0.55), int(w * 0.35), int(h * 1.6)], fill=g + (60,))
    glow = glow.filter(ImageFilter.GaussianBlur(int(h * 0.28)))
    return Image.alpha_composite(bg, glow)


def place_screenshot(canvas, shot, box_w, x, y, radius):
    """Paste a window screenshot with rounded corners and a soft shadow."""
    scale = box_w / shot.width
    shot = shot.resize((box_w, int(shot.height * scale)), Image.LANCZOS)
    shot = rounded(shot, radius)
    shadow = Image.new("RGBA", canvas.size, (0, 0, 0, 0))
    ImageDraw.Draw(shadow).rounded_rectangle([x, y + 16, x + shot.width, y + 16 + shot.height], radius=radius, fill=(0, 0, 0, 120))
    shadow = shadow.filter(ImageFilter.GaussianBlur(28))
    canvas = Image.alpha_composite(canvas, shadow)
    canvas.alpha_composite(shot, (x, y))
    return canvas


def pill(draw, x, y, text, f, theme, pad_x, pad_y):
    box = draw.textbbox((0, 0), text, font=f)
    tw, th = box[2] - box[0], box[3] - box[1]
    draw.rounded_rectangle([x, y, x + tw + pad_x * 2, y + th + pad_y * 2], radius=(th + pad_y * 2) // 2, fill=theme["pill_bg"])
    draw.text((x + pad_x - box[0], y + pad_y - box[1]), text, font=f, fill=theme["pill_fg"])
    return x + tw + pad_x * 2


def compose(size, theme_name, shot, s):
    """`s` scales every measurement (banner is drawn at 2x for crispness)."""
    theme = THEMES[theme_name]
    w, h = size
    canvas = background(size, theme)
    # Screenshot on the right, running off the bottom edge.
    shot_w = int(w * 0.50)
    canvas = place_screenshot(canvas, shot, shot_w, w - shot_w - int(48 * s), int(h * 0.16), int(14 * s))

    layer = Image.new("RGBA", size, (0, 0, 0, 0))
    d = ImageDraw.Draw(layer)
    left = int(72 * s)
    logo_size = int(96 * s)
    logo = make_icon.render(logo_size)
    title_f = font(FONT_BOLD, int(76 * s))
    text_f = font(FONT_REGULAR, int(28 * s))
    pill_f = font(FONT_SEMIBOLD, int(19 * s))

    block_h = logo_size + int(22 * s) + int(112 * s) + int(76 * s) + int(22 * s) + int(40 * s)
    y = (h - block_h) // 2
    layer.alpha_composite(logo, (left, y))
    y += logo_size + int(22 * s)
    d.text((left, y), "Risquette", font=title_f, fill=theme["title"])
    y += int(112 * s)
    for line in ["A fast, native desktop client", "for Discuit."]:
        d.text((left, y), line, font=text_f, fill=theme["text"])
        y += int(38 * s)
    y += int(22 * s)
    x = left
    for label in ["Rust", "GPUI", "Windows", "Open source"]:
        x = pill(d, x, y, label, pill_f, theme, int(14 * s), int(7 * s)) + int(10 * s)
    return Image.alpha_composite(canvas, layer)


DOC_HEADERS = {
    "features": ("Features", "Everything Risquette can do"),
    "development": ("Development", "Build, run and test Risquette"),
    "releasing": ("Releasing", "Installers, updates and the update server"),
}


def doc_header(theme_name, title, subtitle, s=2.0):
    """A slim banner: logo, page title and a subtitle, with the brand glow on the right."""
    theme = THEMES[theme_name]
    w, h = int(1280 * s), int(240 * s)
    canvas = background((w, h), theme)
    layer = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    d = ImageDraw.Draw(layer)
    left = int(56 * s)
    logo_size = int(88 * s)
    layer.alpha_composite(make_icon.render(logo_size), (left, (h - logo_size) // 2))
    x = left + logo_size + int(32 * s)
    small = font(FONT_SEMIBOLD, int(20 * s))
    big = font(FONT_BOLD, int(56 * s))
    sub = font(FONT_REGULAR, int(24 * s))
    total = int(24 * s) + int(70 * s) + int(30 * s)
    y = (h - total) // 2 - int(6 * s)
    d.text((x, y), "RISQUETTE", font=small, fill=theme["pill_fg"])
    y += int(26 * s)
    d.text((x, y), title, font=big, fill=theme["title"])
    y += int(74 * s)
    d.text((x, y), subtitle, font=sub, fill=theme["text"])
    return Image.alpha_composite(canvas, layer)


def framed(shot, radius=12, margin=28):
    """A screenshot with rounded corners and a soft shadow, on a transparent background."""
    w, h = shot.size
    canvas = Image.new("RGBA", (w + margin * 2, h + margin * 2), (0, 0, 0, 0))
    shadow = Image.new("RGBA", canvas.size, (0, 0, 0, 0))
    ImageDraw.Draw(shadow).rounded_rectangle([margin, margin + 8, margin + w, margin + 8 + h], radius=radius, fill=(20, 0, 60, 110))
    canvas = Image.alpha_composite(canvas, shadow.filter(ImageFilter.GaussianBlur(14)))
    canvas.alpha_composite(rounded(shot, radius), (margin, margin))
    return canvas


def main():
    os.makedirs(BRANDING, exist_ok=True)
    make_icon.main()
    shots = {}
    for theme in ("dark", "light"):
        path = os.path.join(SHOTS, f"app-{theme}.png")
        img = trim_black(Image.open(path))
        img.save(path)  # keep the trimmed version
        shots[theme] = img

    for theme in ("dark", "light"):
        banner = compose((2560, 1000), theme, shots[theme], 2.0)
        banner.convert("RGB").save(os.path.join(BRANDING, f"banner-{theme}.png"), optimize=True)
    compose((1280, 640), "dark", shots["dark"], 0.9).convert("RGB").save(os.path.join(BRANDING, "social-preview.png"), optimize=True)
    for n in (64, 128, 256, 512):
        make_icon.render(n).save(os.path.join(BRANDING, f"logo-{n}.png"), optimize=True)

    headers = os.path.join(BRANDING, "headers")
    os.makedirs(headers, exist_ok=True)
    for key, (title, subtitle) in DOC_HEADERS.items():
        for theme in ("dark", "light"):
            doc_header(theme, title, subtitle).convert("RGB").save(os.path.join(headers, f"{key}-{theme}.png"), optimize=True)

    frames = os.path.join(BRANDING, "framed")
    os.makedirs(frames, exist_ok=True)
    for name in sorted(os.listdir(SHOTS)):
        if name.endswith(".png"):
            shot = trim_black(Image.open(os.path.join(SHOTS, name)))
            framed(shot).save(os.path.join(frames, name), optimize=True)
    print("wrote", BRANDING)


if __name__ == "__main__":
    main()
