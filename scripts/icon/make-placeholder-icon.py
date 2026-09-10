"""Placeholder app icon for sidebar-term: a tiny terminal with a sidebar and a robot peeking in.

Writes src-tauri/icons/app-icon.png: 1024x1024, full-bleed and fully opaque. macOS 26 applies its
own rounded mask to a full-bleed icon, but shrinks an icon with transparent margins onto a grey
plate. Replace that PNG with your own full-bleed design and run `pnpm app:icon`, then
`pnpm app:install`.
Run: python3 scripts/icon/make-placeholder-icon.py
"""
from pathlib import Path
from PIL import Image, ImageDraw, ImageFilter

S = 2048  # drawn at 2x, downsampled for antialiasing
OUT = Path(__file__).resolve().parents[2] / "src-tauri/icons/app-icon.png"


def lerp(a, b, t):
    return tuple(round(a[i] + (b[i] - a[i]) * t) for i in range(len(a)))


def hexc(h, a=255):
    h = h.lstrip("#")
    return (int(h[0:2], 16), int(h[2:4], 16), int(h[4:6], 16), a)


def gradient(size, c1, c2):
    """Diagonal gradient, top-left c1 to bottom-right c2."""
    w, h = size
    g = Image.new("RGBA", size)
    px = g.load()
    for y in range(h):
        for x in range(w):
            px[x, y] = lerp(c1, c2, (x + y) / (w + h - 2))
    return g


def rounded_mask(size, box, r):
    m = Image.new("L", size, 0)
    ImageDraw.Draw(m).rounded_rectangle(box, r, fill=255)
    return m


img = Image.new("RGBA", (S, S), (0, 0, 0, 0))

# Body: the region cropped out as the final full-bleed icon.
body = (200, 200, 1848, 1848)
grad = gradient((S, S), hexc("#7B5CFF"), hexc("#FF5FA2"))
img.paste(grad, (0, 0), rounded_mask((S, S), body, 0))
# Gloss on the top half.
gloss = Image.new("RGBA", (S, S), (0, 0, 0, 0))
ImageDraw.Draw(gloss).ellipse((-200, -900, 2248, 900), fill=(255, 255, 255, 38))
gloss = gloss.filter(ImageFilter.GaussianBlur(90))
img.alpha_composite(Image.composite(gloss, Image.new("RGBA", (S, S)), rounded_mask((S, S), body, 0)))

d = ImageDraw.Draw(img)

# Terminal window.
win = (360, 420, 1640, 1450)
wshadow = Image.new("RGBA", (S, S), (0, 0, 0, 0))
ImageDraw.Draw(wshadow).rounded_rectangle((win[0], win[1] + 30, win[2], win[3] + 30), 90, fill=(25, 5, 45, 140))
img.alpha_composite(wshadow.filter(ImageFilter.GaussianBlur(30)))
d.rounded_rectangle(win, 90, fill=hexc("#0F1115"), outline=hexc("#FFFFFF", 40), width=6)
# Sidebar (left panel of the window).
side = Image.new("RGBA", (S, S), (0, 0, 0, 0))
ImageDraw.Draw(side).rectangle((win[0], win[1], 800, win[3]), fill=hexc("#1B1E26"))
img.alpha_composite(Image.composite(side, Image.new("RGBA", (S, S)), rounded_mask((S, S), win, 90)))
d = ImageDraw.Draw(img)
d.line((800, win[1] + 6, 800, win[3] - 6), fill=hexc("#FFFFFF", 22), width=4)
# Traffic lights.
for i, c in enumerate(["#FF5F57", "#FEBC2E", "#28C840"]):
    cx = 450 + i * 70
    d.ellipse((cx - 22, 488 - 22, cx + 22, 488 + 22), fill=hexc(c))
# Tab rows: active row highlighted, a repo colour dot per row.
rows = [("#FFD166", 300, True), ("#FFD166", 230, False), ("#5EEAD4", 260, False), ("#FF8FAB", 200, False)]
for i, (dot, bar, active) in enumerate(rows):
    y = 610 + i * 118
    if active:
        d.rounded_rectangle((388, y - 44, 772, y + 44), 26, fill=hexc("#2A2E3A"))
    d.ellipse((430 - 16, y - 16, 430 + 16, y + 16), fill=hexc(dot))
    d.rounded_rectangle((472, y - 14, 472 + bar, y + 14), 14, fill=hexc("#E6E9F2" if active else "#5B6273"))
# Prompt: chevron + cursor.
d.line([(880, 580), (950, 630), (880, 680)], fill=hexc("#7EE787"), width=34, joint="curve")
d.rounded_rectangle((990, 590, 1040, 676), 8, fill=hexc("#E6E9F2"))
for i, w in enumerate([420, 320, 380]):
    y = 760 + i * 80
    d.rounded_rectangle((880, y, 880 + w, y + 26), 13, fill=hexc("#3A4050"))

# Robot head peeking over the bottom-right corner.
cx, cy = 1450, 1430
rshadow = Image.new("RGBA", (S, S), (0, 0, 0, 0))
ImageDraw.Draw(rshadow).rounded_rectangle((cx - 300, cy - 190, cx + 300, cy + 290), 140, fill=(40, 0, 60, 170))
img.alpha_composite(rshadow.filter(ImageFilter.GaussianBlur(34)))
d = ImageDraw.Draw(img)
# Antenna.
d.line((cx, cy - 230, cx, cy - 360), fill=hexc("#E6FFFA"), width=26)
d.ellipse((cx - 52, cy - 440, cx + 52, cy - 336), fill=hexc("#FFD166"), outline=hexc("#FFFFFF", 200), width=8)
# Ears.
for sx in (-1, 1):
    d.rounded_rectangle((cx + sx * 300 - 40, cy - 60, cx + sx * 300 + 40, cy + 90), 30, fill=hexc("#2DD4BF"))
# Head with a vertical sheen.
head_box = (cx - 290, cy - 240, cx + 290, cy + 250)
head = gradient((S, S), hexc("#8AF5E4"), hexc("#22B8A6"))
img.paste(head, (0, 0), rounded_mask((S, S), head_box, 150))
d = ImageDraw.Draw(img)
d.rounded_rectangle(head_box, 150, outline=hexc("#FFFFFF", 150), width=10)
# Face plate.
face = (cx - 215, cy - 150, cx + 215, cy + 160)
d.rounded_rectangle(face, 110, fill=hexc("#0F1115"))
# Eyes with glow.
glow = Image.new("RGBA", (S, S), (0, 0, 0, 0))
gd = ImageDraw.Draw(glow)
for ex in (cx - 100, cx + 100):
    gd.ellipse((ex - 80, cy - 70, ex + 80, cy + 90), fill=hexc("#5EEAD4", 170))
img.alpha_composite(glow.filter(ImageFilter.GaussianBlur(26)))
d = ImageDraw.Draw(img)
for ex in (cx - 100, cx + 100):
    d.ellipse((ex - 52, cy - 42, ex + 52, cy + 62), fill=hexc("#B8FFF4"))
    d.ellipse((ex - 12, cy - 22, ex + 22, cy + 12), fill=hexc("#FFFFFF"))
# Smile.
d.arc((cx - 70, cy + 40, cx + 70, cy + 130), 20, 160, fill=hexc("#5EEAD4"), width=16)
# Cheeks.
for ex in (cx - 170, cx + 170):
    d.ellipse((ex - 28, cy + 70, ex + 28, cy + 110), fill=hexc("#FF8FAB", 200))

img.crop(body).convert("RGB").resize((1024, 1024), Image.LANCZOS).save(OUT)
print(f"wrote {OUT}")
