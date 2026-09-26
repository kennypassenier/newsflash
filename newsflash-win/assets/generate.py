#!/usr/bin/env python3
"""Regenerates the bundled toast images (stdlib only — no Pillow):

  app.png          256x256  toast identity icon (header, Settings)
  info/warning/critical.png  96x96  per-priority logo (AR11's Windows twin)
  demo-hero.png    728x360  hero image for `newsflash demo`

Run from this directory: python3 generate.py
"""
import math
import struct
import zlib


def png(path, w, h, px):
    raw = b"".join(b"\x00" + bytes(px[y * w * 4:(y + 1) * w * 4]) for y in range(h))

    def chunk(tag, data):
        c = tag + data
        return struct.pack(">I", len(data)) + c + struct.pack(">I", zlib.crc32(c) & 0xFFFFFFFF)

    with open(path, "wb") as f:
        f.write(b"\x89PNG\r\n\x1a\n")
        f.write(chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0)))
        f.write(chunk(b"IDAT", zlib.compress(raw, 9)))
        f.write(chunk(b"IEND", b""))


class Canvas:
    def __init__(self, w, h, ss=4):
        self.w, self.h, self.ss = w, h, ss
        self.px = [0.0] * (w * h * 4)  # premultiplied float RGBA

    def fill(self, inside, color, bbox=None):
        """inside(x, y) -> bool in pixel units; color(x, y) -> (r, g, b, a) 0..255."""
        x0, y0, x1, y1 = bbox or (0, 0, self.w, self.h)
        x0, y0 = max(0, int(x0)), max(0, int(y0))
        x1, y1 = min(self.w, int(math.ceil(x1))), min(self.h, int(math.ceil(y1)))
        n = self.ss
        offs = [(i + 0.5) / n for i in range(n)]
        for y in range(y0, y1):
            for x in range(x0, x1):
                hits = sum(1 for oy in offs for ox in offs if inside(x + ox, y + oy))
                if not hits:
                    continue
                cov = hits / (n * n)
                r, g, b, a = color(x + 0.5, y + 0.5)
                a = a / 255 * cov
                i = (y * self.w + x) * 4
                inv = 1 - a
                self.px[i] = r * a + self.px[i] * inv
                self.px[i + 1] = g * a + self.px[i + 1] * inv
                self.px[i + 2] = b * a + self.px[i + 2] * inv
                self.px[i + 3] = 255 * a + self.px[i + 3] * inv

    def save(self, path):
        out = []
        for i in range(0, len(self.px), 4):
            a = self.px[i + 3]
            if a <= 0:
                out += [0, 0, 0, 0]
            else:
                k = 255 / a
                out += [min(255, round(self.px[i] * k)), min(255, round(self.px[i + 1] * k)),
                        min(255, round(self.px[i + 2] * k)), min(255, round(a))]
        png(path, self.w, self.h, out)


def circle(cx, cy, r):
    return (lambda x, y: (x - cx) ** 2 + (y - cy) ** 2 <= r * r), (cx - r, cy - r, cx + r, cy + r)


def rrect(x0, y0, x1, y1, r):
    def inside(x, y):
        if not (x0 <= x <= x1 and y0 <= y <= y1):
            return False
        cx = min(max(x, x0 + r), x1 - r)
        cy = min(max(y, y0 + r), y1 - r)
        return (x - cx) ** 2 + (y - cy) ** 2 <= r * r
    return inside, (x0, y0, x1, y1)


def poly(points):
    def inside(x, y):
        c = False
        j = len(points) - 1
        for i in range(len(points)):
            xi, yi = points[i]
            xj, yj = points[j]
            if (yi > y) != (yj > y) and x < (xj - xi) * (y - yi) / (yj - yi) + xi:
                c = not c
            j = i
        return c
    xs, ys = [p[0] for p in points], [p[1] for p in points]
    return inside, (min(xs), min(ys), max(xs), max(ys))


def solid(rgb, a=255):
    return lambda x, y: (*rgb, a)


def lerp(c0, c1, t):
    t = max(0.0, min(1.0, t))
    return tuple(a + (b - a) * t for a, b in zip(c0, c1))


def hexrgb(h):
    return tuple(int(h[i:i + 2], 16) for i in (1, 3, 5))


def draw(c, shape, color):
    inside, bbox = shape
    c.fill(inside, color, bbox)


def app_icon():
    s = 256
    c = Canvas(s, s)
    p0, p1 = hexrgb("#6d28d9"), hexrgb("#06b6d4")
    draw(c, rrect(8, 8, s - 8, s - 8, 56), lambda x, y: (*lerp(p0, p1, (x + y) / (2 * s)), 255))
    # the "flash": a lightning bolt
    bolt = [(148, 28), (70, 142), (122, 142), (100, 228), (188, 106), (134, 106), (160, 28)]
    draw(c, poly(bolt), solid((255, 255, 255)))
    c.save("app.png")


def badge(name, bg, fg, shape_kind):
    s = 96
    c = Canvas(s, s)
    if shape_kind == "triangle":
        draw(c, poly([(48, 6), (92, 86), (4, 86)]), solid(bg))
        draw(c, rrect(43, 30, 53, 64, 5), solid(fg))
        draw(c, circle(48, 74, 6), solid(fg))
    else:
        draw(c, circle(48, 48, 44), solid(bg))
        if shape_kind == "i":
            draw(c, circle(48, 26, 7), solid(fg))
            draw(c, rrect(42, 40, 54, 74, 5), solid(fg))
        else:  # "!"
            draw(c, rrect(42, 20, 54, 56, 5), solid(fg))
            draw(c, circle(48, 70, 7), solid(fg))
    c.save(name)


def hero():
    w, h = 728, 360
    c = Canvas(w, h, ss=2)
    top, bottom = hexrgb("#0f172a"), hexrgb("#312e81")
    draw(c, ((lambda x, y: True), (0, 0, w, h)), lambda x, y: (*lerp(top, bottom, y / h), 255))
    for sx, sy in [(60, 40), (140, 90), (250, 30), (520, 60), (640, 35), (690, 110), (400, 50)]:
        draw(c, circle(sx, sy, 2.2), solid((226, 232, 240)))
    draw(c, circle(610, 80, 34), solid(hexrgb("#fde68a")))
    draw(c, circle(596, 70, 30), lambda x, y: (*lerp(top, bottom, y / h), 255))
    draw(c, poly([(0, 330), (728, 300), (728, 360), (0, 360)]), solid(hexrgb("#1e1b4b")))
    house = hexrgb("#111827")
    draw(c, poly([(214, 312), (214, 190), (364, 96), (514, 190), (514, 312)]), solid(house))
    draw(c, rrect(338, 214, 390, 312, 4), solid(hexrgb("#fbbf24")))
    draw(c, rrect(250, 210, 300, 250, 3), solid(hexrgb("#f59e0b"), 200))
    draw(c, rrect(428, 210, 478, 250, 3), solid(hexrgb("#f59e0b"), 120))
    # a figure at the door
    draw(c, circle(314, 238, 10), solid(hexrgb("#0b1020")))
    draw(c, rrect(302, 250, 326, 312, 9), solid(hexrgb("#0b1020")))
    # camera frame corners
    cyan = solid(hexrgb("#22d3ee"))
    x0, y0, x1, y1, t, l = 272, 206, 410, 322, 4, 26
    for (ax, ay, dx, dy) in [(x0, y0, 1, 1), (x1, y0, -1, 1), (x0, y1, 1, -1), (x1, y1, -1, -1)]:
        draw(c, rrect(min(ax, ax + dx * l), min(ay, ay + dy * t), max(ax, ax + dx * l), max(ay, ay + dy * t), 1), cyan)
        draw(c, rrect(min(ax, ax + dx * t), min(ay, ay + dy * l), max(ax, ax + dx * t), max(ay, ay + dy * l), 1), cyan)
    draw(c, circle(40, 36, 8), solid(hexrgb("#ef4444")))
    c.save("demo-hero.png")


if __name__ == "__main__":
    app_icon()
    badge("info.png", hexrgb("#2563eb"), (255, 255, 255), "i")
    badge("warning.png", hexrgb("#f59e0b"), hexrgb("#1f2937"), "triangle")
    badge("critical.png", hexrgb("#dc2626"), (255, 255, 255), "!")
    hero()
