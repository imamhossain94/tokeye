"""Turn captured frames/screenshots (scripts/capture.mjs) into README media: GIFs and a hero image.

    python scripts/make_media.py docs/images
"""
import json
import os
import shutil
import sys

from PIL import Image, ImageDraw, ImageFilter

IMG = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else "docs/images")


def wallpaper(w, h):
    """Soft blue-violet gradient with two glows, like a desktop wallpaper."""
    top, bottom = (24, 28, 54), (58, 34, 72)
    bg = Image.new("RGB", (w, h))
    px = bg.load()
    for y in range(h):
        for x in range(w):
            k = (y / max(1, h - 1)) * 0.75 + (x / max(1, w - 1)) * 0.25
            px[x, y] = tuple(int(top[i] + (bottom[i] - top[i]) * k) for i in range(3))
    glow = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    d = ImageDraw.Draw(glow)
    d.ellipse([w * 0.55, -h * 0.4, w * 1.25, h * 0.6], fill=(217, 119, 87, 70))
    d.ellipse([-w * 0.3, h * 0.45, w * 0.45, h * 1.4], fill=(110, 140, 255, 60))
    glow = glow.filter(ImageFilter.GaussianBlur(max(w, h) * 0.08))
    return Image.alpha_composite(bg.convert("RGBA"), glow)


def gif(frames_dir, out, fps=15, pad=14, scale=1.0, max_seconds=None):
    meta = json.load(open(os.path.join(frames_dir, "frames.json")))
    frames = [(Image.open(os.path.join(frames_dir, m["file"])).convert("RGBA"), m["t"]) for m in meta]
    # Crop to everything that ever becomes visible.
    box = None
    for im, _ in frames[:: max(1, len(frames) // 60)]:
        b = im.getchannel("A").point(lambda a: 255 if a > 8 else 0).getbbox()
        if b:
            box = b if box is None else (min(box[0], b[0]), min(box[1], b[1]), max(box[2], b[2]), max(box[3], b[3]))
    W, H = frames[0][0].size
    box = (max(0, box[0] - pad), max(0, box[1] - pad), min(W, box[2] + pad), min(H, box[3] + pad))
    bg = wallpaper(box[2] - box[0], box[3] - box[1])
    kept, last = [], None
    t0 = frames[0][1]
    for im, t in frames:
        if max_seconds and t - t0 > max_seconds:
            break
        if last is None or t - last >= 1 / fps:
            kept.append((im, t))
            last = t
    out_frames, durations = [], []
    for i, (im, t) in enumerate(kept):
        f = Image.alpha_composite(bg, im.crop(box)).convert("RGB")
        if scale != 1.0:
            f = f.resize((int(f.width * scale), int(f.height * scale)), Image.LANCZOS)
        out_frames.append(f)
        nxt = kept[i + 1][1] if i + 1 < len(kept) else t + 1.2
        durations.append(max(40, int((nxt - t) * 1000)))
    # One shared palette keeps colors stable across frames.
    sample = Image.new("RGB", (out_frames[0].width, out_frames[0].height * 6))
    for i in range(6):
        sample.paste(out_frames[(i * len(out_frames)) // 6], (0, out_frames[0].height * i))
    pal = sample.quantize(colors=160, method=Image.Quantize.MEDIANCUT)
    q = [f.quantize(palette=pal, dither=Image.Dither.NONE) for f in out_frames]
    q[0].save(out, save_all=True, append_images=q[1:], duration=durations, loop=0, optimize=True, disposal=1)
    print(f"{os.path.basename(out)}: {len(q)} frames, {os.path.getsize(out) / 1024:.0f} KB")


def card(name):
    """A surface screenshot cropped to its opaque card (drops the window's clipped shadow)."""
    im = Image.open(os.path.join(IMG, name)).convert("RGBA")
    box = im.getchannel("A").point(lambda a: 255 if a > 200 else 0).getbbox()
    return im.crop(box) if box else im


def place(canvas, im, xy, radius=16, shadow=True):
    x, y = xy
    if shadow:
        sh = Image.new("RGBA", (im.width + 120, im.height + 120), (0, 0, 0, 0))
        ImageDraw.Draw(sh).rounded_rectangle([60, 70, im.width + 60, im.height + 70], radius=radius + 8, fill=(0, 0, 0, 140))
        canvas.alpha_composite(sh.filter(ImageFilter.GaussianBlur(28)), (x - 60, y - 60))
    canvas.alpha_composite(im, (x, y))


def hero(out):
    """All surfaces on one fake desktop (assets are 2x)."""
    W, H = 2400, 1350
    canvas = wallpaper(W // 4, H // 4).resize((W, H), Image.LANCZOS)
    # Taskbar with the strip next to the clock
    canvas.alpha_composite(Image.new("RGBA", (W, 96), (18, 18, 24, 240)), (0, H - 96))
    strip = card("taskbar.png")
    place(canvas, strip, (W - strip.width - 330, H - 96 + (96 - strip.height) // 2), shadow=False)
    # Notch hanging from the top center
    notch = Image.open(os.path.join(IMG, "notch-expanded.png")).convert("RGBA")
    canvas.alpha_composite(notch, ((W - notch.width) // 2, 0))
    # Desktop widget and floating bubble on the left
    place(canvas, card("widget.png"), (120, 300))
    place(canvas, card("bubble.png"), (120, 880))
    # Dashboard flyout above the taskbar, right
    dash = Image.open(os.path.join(IMG, "dashboard-overview.png")).convert("RGBA")
    dash = dash.crop((0, 0, dash.width, min(dash.height, 820)))
    mask = Image.new("L", dash.size, 0)
    ImageDraw.Draw(mask).rounded_rectangle([0, 0, dash.width - 1, dash.height - 1], radius=16, fill=255)
    dash.putalpha(mask)
    place(canvas, dash, (W - dash.width - 110, H - 96 - dash.height - 36))
    canvas.convert("RGB").resize((W // 2, H // 2), Image.LANCZOS).save(out, optimize=True)
    print(f"{os.path.basename(out)}: {os.path.getsize(out) / 1024:.0f} KB")


if __name__ == "__main__":
    if len(sys.argv) > 2 and sys.argv[2] == "hero":
        hero(os.path.join(IMG, "hero.png"))
        sys.exit(0)
    if os.path.isdir(os.path.join(IMG, "_frames-notch")):
        gif(os.path.join(IMG, "_frames-notch"), os.path.join(IMG, "notch.gif"), fps=15)
    if os.path.isdir(os.path.join(IMG, "_frames-eyes")):
        gif(os.path.join(IMG, "_frames-eyes"), os.path.join(IMG, "companion.gif"), fps=15, pad=24)
    hero(os.path.join(IMG, "hero.png"))
    for d in ("_frames-notch", "_frames-eyes"):
        shutil.rmtree(os.path.join(IMG, d), ignore_errors=True)
