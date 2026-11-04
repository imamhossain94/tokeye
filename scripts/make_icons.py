"""Render Tokeye's app and tray icons (two glowing eyes on a dark rounded square)."""
import os, sys
from PIL import Image, ImageDraw, ImageFilter

OUT = sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(__file__), "..", "src-tauri", "icons")
os.makedirs(OUT, exist_ok=True)


def logo(size, eye=(154, 247, 255), bg=(24, 24, 30)):
    s = size * 4
    im = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    ImageDraw.Draw(im).rounded_rectangle([0, 0, s - 1, s - 1], radius=int(s * 0.24), fill=bg + (255,))
    ew, eh, cy = int(s * 0.19), int(s * 0.34), int(s * 0.5)
    boxes = [[cx - ew // 2, cy - eh // 2, cx + ew // 2, cy + eh // 2] for cx in (int(s * 0.34), int(s * 0.66))]
    glow = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    gd = ImageDraw.Draw(glow)
    for b in boxes:
        gd.rounded_rectangle(b, radius=int(ew * 0.46), fill=eye + (190,))
    im = Image.alpha_composite(im, glow.filter(ImageFilter.GaussianBlur(s * 0.035)))
    d = ImageDraw.Draw(im)
    for b in boxes:
        d.rounded_rectangle(b, radius=int(ew * 0.46), fill=eye + (255,))
    return im.resize((size, size), Image.LANCZOS)


big = logo(512)
big.save(os.path.join(OUT, "icon.png"))
logo(256).save(os.path.join(OUT, "128x128@2x.png"))
logo(128).save(os.path.join(OUT, "128x128.png"))
logo(32).save(os.path.join(OUT, "32x32.png"))
logo(256).save(os.path.join(OUT, "icon.ico"), sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
logo(32).save(os.path.join(OUT, "tray.png"))
logo(32, eye=(255, 197, 107)).save(os.path.join(OUT, "tray-warn.png"))
logo(32, eye=(255, 107, 107)).save(os.path.join(OUT, "tray-over.png"))
print("icons written to", os.path.abspath(OUT))
