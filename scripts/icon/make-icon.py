"""App icon for sidebar-term: an iridescent blob drawn as halftone dots, on the terminal's background.

Writes src-tauri/icons/app-icon.png: 1024x1024, full-bleed and fully opaque. macOS 26 applies its
own rounded mask to a full-bleed icon, but shrinks an icon with transparent margins onto a grey
plate, so the ground is the terminal's own #0f1115 rather than transparency.
Then run `pnpm app:icon` and `pnpm app:install`.
Run: python3 scripts/icon/make-icon.py   (needs Pillow and numpy)
"""
import math
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFilter

OUT = Path(__file__).resolve().parents[2] / "src-tauri/icons/app-icon.png"
SIZE = 1024
SS = 4  # drawn at 4x, downsampled for antialiasing
GROUND = "#0f1115"  # TERMINAL_BACKGROUND in src/lib/terminal/theme.ts

# Metaballs (x, y, radius) in unit coordinates; their merged surface is the blob.
BALLS = [(0.41, 0.41, 0.23), (0.68, 0.67, 0.165)]
# Iridescent ramp the surface colour is picked from.
RAMP = ["#fff0b3", "#ff8ac8", "#a86bff", "#4f7dff", "#52e3ee"]
LIGHT = np.array([-0.45, -0.6, 0.66]) / np.linalg.norm([-0.45, -0.6, 0.66])
SPACING = 0.032  # halftone cell, as a fraction of the icon


def hexrgb(h):
    return tuple(int(h.lstrip("#")[i:i + 2], 16) for i in (0, 2, 4))


def ramp(t):
    t = min(max(t, 0.0), 1.0) * (len(RAMP) - 1)
    i = min(int(t), len(RAMP) - 2)
    a, b = hexrgb(RAMP[i]), hexrgb(RAMP[i + 1])
    return tuple(a[k] + (b[k] - a[k]) * (t - i) for k in range(3))


def field(x, y):
    return sum(r * r / ((x - bx) ** 2 + (y - by) ** 2 + 1e-9) for bx, by, r in BALLS)


def height(x, y):
    """Dome over the blob: exactly a hemisphere for a lone ball, blended where balls merge."""
    return 0.22 * math.sqrt(max(0.0, 1 - 1 / max(field(x, y), 1e-9)))


def surface(x, y):
    """Normal, diffuse and specular light at a point inside the blob."""
    e = 1e-4
    dx = (height(x + e, y) - height(x - e, y)) / (2 * e)
    dy = (height(x, y + e) - height(x, y - e)) / (2 * e)
    n = np.array([-dx, -dy, 1.0])
    n /= np.linalg.norm(n)
    diffuse = max(0.0, float(n @ LIGHT))
    half = LIGHT + np.array([0, 0, 1.0])
    spec = max(0.0, float(n @ (half / np.linalg.norm(half)))) ** 90
    return n, diffuse, spec


def render(max_dot=0.66, glow=0.22):
    """max_dot: largest dot radius in cells (above ~0.58 lit dots merge into a solid surface).
    glow: opacity of a soft coloured bloom behind the dots."""
    S = SIZE * SS
    img = Image.new("RGB", (S, S), hexrgb(GROUND))
    dots = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    d = ImageDraw.Draw(dots)
    row, y = 0, 0.0
    while y <= 1:
        x = (SPACING / 2) if row % 2 else 0.0
        while x <= 1:
            if field(x, y) > 1:
                n, diffuse, spec = surface(x, y)
                lit = min(1.0, 0.28 + 0.72 * diffuse ** 1.2 + 0.3 * spec)
                r = SPACING * max_dot * lit
                if r * SIZE > 0.7:
                    t = 0.5 + 0.55 * n[0] + 0.35 * n[1] + 0.35 * (y - 0.5)
                    c = np.array(ramp(t)) * (0.7 + 0.35 * diffuse) + 255 * spec * 0.45
                    c = tuple(int(min(255, v)) for v in c)
                    d.ellipse(((x - r) * S, (y - r) * S, (x + r) * S, (y + r) * S), fill=c + (255,))
            x += SPACING
        y += SPACING * math.sqrt(3) / 2
        row += 1
    if glow:
        bloom = dots.filter(ImageFilter.GaussianBlur(S * 0.05))
        bloom.putalpha(bloom.getchannel("A").point(lambda a: int(a * glow)))
        img.paste(bloom, (0, 0), bloom)
    img.paste(dots, (0, 0), dots)
    return img.resize((SIZE, SIZE), Image.LANCZOS)


if __name__ == "__main__":
    render().save(OUT)
    print(f"wrote {OUT}")
