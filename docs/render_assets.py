#!/usr/bin/env python3
"""Render the README assets for ShannonsPrism in docs/assets: the banner and the terminal screenshots.

The screenshot text is real output from `sprism`, trimmed where noted. Needs Pillow,
Source Code Pro and Adwaita Sans. Run: python3 docs/render_assets.py
"""
import re
from PIL import Image, ImageDraw, ImageFont, ImageFilter

import os
OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "assets")
MONO = "/usr/share/fonts/adobe-source-code-pro-fonts/SourceCodePro-Regular.otf"
MONO_B = "/usr/share/fonts/adobe-source-code-pro-fonts/SourceCodePro-Bold.otf"
MONO_M = "/usr/share/fonts/adobe-source-code-pro-fonts/SourceCodePro-Medium.otf"
SANS = "/usr/share/fonts/adwaita-sans-fonts/AdwaitaSans-Regular.ttf"

C = dict(
    bg="#0d1117", card="#0f141b", bar="#161c25", text="#d0d7de", dim="#6e7681", faint="#3d444d",
    white="#f0f6fc", green="#3fb950", amber="#d29922", red="#f85149", blue="#79c0ff",
    purple="#bc8cff", teal="#39c5cf", pink="#f778ba",
)
HEAT = {"░": "#3d6fd8", "▒": "#2fb9a9", "▓": "#e3a72f", "█": "#f0574a"}

def sans(size, weight="Regular"):
    f = ImageFont.truetype(SANS, size)
    try: f.set_variation_by_name(weight)
    except Exception: pass
    return f

# ---------------------------------------------------------------- tokenizer
VERDICT = {"hit": "green", "over-H": "amber", "recode": "blue", "expand": "dim", "BROKEN": "red"}
BAND = {"padding": "dim", "structured": "blue", "code/text": "teal", "compressed": "amber",
        "packed": "red", "encrypted": "purple", "random": "red"}

def spans(line):
    """Return list of (start, end, color, bold) covering emphasised parts."""
    out = []
    def add(m, color, bold=False, group=0):
        out.append((m.start(group), m.end(group), color, bold))
    if line.startswith("$ "):
        out.append((0, 1, "green", True)); out.append((2, len(line), "white", True))
        # flags
        for m in re.finditer(r"--[a-z-]+(?: [\w./:]+)?", line): add(m, "teal", True)
        return out
    if re.fullmatch(r"-{20,}", line.strip()):
        return [(0, len(line), "faint", False)]
    if re.match(r"^(codec|section|file|  offset|  piece|  window|  \^)\s", line) or line.startswith("  ^"):
        return [(0, len(line), "dim", False)]
    if re.match(r"^\s*(\(\d+ more|\.{3}|⋯)", line) or "more rows" in line or re.match(r"^\s+\(\d+ more\)", line):
        return [(0, len(line), "dim", False)]
    if line.startswith(("verified:", "C UNPACK", "wrote ")):
        return [(0, len(line), "green", True)]
    if re.match(r"^\s+as a (program|library):", line):
        return [(0, len(line), "dim", False)]
    if line.startswith(("best within", "best codec here", "per-section vs one blob", "sections with a codec",
                        "regions worth", "carve:", "identified as", "mean whole-file", "hit smaller")):
        out.append((0, len(line), "white", True))
    # hex dump
    m = re.match(r"^  ([0-9a-f]{8}) ((?:[0-9a-f]{2} ){16}) (.*)$", line)
    if m:
        return [(m.start(1), m.end(1), "dim", False), (m.start(2), m.end(2), "text", False),
                (m.start(3), m.end(3), "teal", False)]
    # table row: first column name
    m = re.match(r"^(\S+)\s+(hit|over-H|recode|expand|BROKEN)\s", line)
    if m:
        out.append((m.start(1), m.end(1), "white", True))
    m = re.match(r"^(\.\S+|gap[^ ]*|[\w.+-]+)\s+0x[0-9a-f]+\s", line)
    if m and not line.startswith("  "):
        out.append((m.start(1), m.end(1), "white", True))
    m = re.match(r"^  (\S+)\s+(\d[\d,]*)\s+(\d\.\d{3})\s+(\S+)\s+(\d+%)\s+(\S+)$", line)  # corpus row
    if m:
        out.append((m.start(1), m.end(1), "white", True)); out.append((m.start(4), m.end(4), "purple", False))
    for w, col in VERDICT.items():
        for mm in re.finditer(r"(?<![\w-])" + re.escape(w) + r"(?![\w-])", line): add(mm, col, True)
    for w, col in BAND.items():
        for mm in re.finditer(r"(?<![\w/])" + re.escape(w) + r"(?![\w/])", line): add(mm, col, False)
    for mm in re.finditer(r"0x[0-9a-f]+", line): add(mm, "blue", False)
    for mm in re.finditer(r"\bH \d\.\d+|\bH\s+\d\.\d{2,3}\b", line): add(mm, "purple", False)
    for mm in re.finditer(r"\((?:ELF|PE)[^)]*\)|ELF x86-64 executable|PE x86-64 executable|\btext\b(?=\s*$)", line): add(mm, "green", False)
    for mm in re.finditer(r"\bvia (\S+)", line): add(mm, "teal", False, 1)
    for mm in re.finditer(r"-> (\d[\d,]*(?:\.\d+)?)", line): add(mm, "green", True, 1)
    for mm in re.finditer(r"\b(one blob wins|per-section wins)[^:]*", line): add(mm, "amber", True)
    for mm in re.finditer(r"\d+ codecs in \d+\.\d+s", line): add(mm, "green", True)
    for mm in re.finditer(r"has (xz|gzip|zlib|zstd|lz4|bzip2|lzma|deflate) stream|inside the \w+ stream", line): add(mm, "amber", True)
    for mm in re.finditer(r"\b(xz|gzip|zlib|zstd|lz4|bzip2|lzma|deflate)\b(?=\s+\d)", line): add(mm, "amber", True)
    for mm in re.finditer(r"\[dependency-free codecs only\]", line): add(mm, "green", False)
    for mm in re.finditer(r"\b\d+ stream\(s\)", line): add(mm, "green", True)
    for mm in re.finditer(r"\b(\d[\d,]*) bytes -> (\d[\d,]*) bytes \((\d\.\d+)\)", line):
        add(mm, "white", True)
    return out

def colorize(line):
    """Resolve spans to a per-character (color, bold) list; earlier spans win."""
    n = len(line)
    col = [("text", False)] * n
    claimed = [False] * n
    for s, e, c, b in spans(line):
        for i in range(s, min(e, n)):
            if not claimed[i]:
                col[i] = (c, b); claimed[i] = True
    return col

# ---------------------------------------------------------------- terminal
def terminal(name, title, text, size=15, scale=2, width_chars=None, cols_min=96):
    lines = text.rstrip("\n").split("\n")
    fs = size * scale
    font = ImageFont.truetype(MONO, fs); bold = ImageFont.truetype(MONO_M, fs)
    adv = font.getlength("M")
    ncols = max(cols_min, width_chars or max(len(l) for l in lines))
    lh = int(fs * 1.5)
    pad = 26 * scale; barh = 40 * scale; margin = 28 * scale; radius = 14 * scale
    W = int(ncols * adv + 2 * pad); H = barh + pad + lh * len(lines) + pad
    img = Image.new("RGBA", (W + 2 * margin, H + 2 * margin), (0, 0, 0, 0))
    # shadow
    sh = Image.new("RGBA", img.size, (0, 0, 0, 0))
    ImageDraw.Draw(sh).rounded_rectangle([margin, margin + 10 * scale, margin + W, margin + H + 10 * scale], radius, fill=(0, 0, 0, 110))
    sh = sh.filter(ImageFilter.GaussianBlur(14 * scale)); img.alpha_composite(sh)
    d = ImageDraw.Draw(img)
    d.rounded_rectangle([margin, margin, margin + W, margin + H], radius, fill=C["card"], outline="#2a313c", width=scale)
    # title bar
    bar = Image.new("RGBA", img.size, (0, 0, 0, 0)); bd = ImageDraw.Draw(bar)
    bd.rounded_rectangle([margin, margin, margin + W, margin + barh], radius, fill=C["bar"])
    bd.rectangle([margin, margin + radius, margin + W, margin + barh], fill=C["bar"])
    img.alpha_composite(bar); d = ImageDraw.Draw(img)
    d.line([margin, margin + barh, margin + W, margin + barh], fill="#2a313c", width=scale)
    for i, c in enumerate(("#ff5f57", "#febc2e", "#28c840")):
        cx = margin + pad + i * 22 * scale; cy = margin + barh // 2
        d.ellipse([cx - 6 * scale, cy - 6 * scale, cx + 6 * scale, cy + 6 * scale], fill=c)
    tf = ImageFont.truetype(MONO, int(fs * 0.85))
    tw = tf.getlength(title)
    d.text((margin + (W - tw) / 2, margin + barh / 2 - fs * 0.85 / 2 - 2), title, font=tf, fill=C["dim"])
    # body
    y = margin + barh + pad
    x0 = margin + pad
    for line in lines:
        cols = colorize(line)
        for i, ch in enumerate(line):
            x = x0 + i * adv
            if ch in HEAT:
                d.rectangle([x, y + fs * 0.18, x + adv - scale, y + fs * 1.12], fill=HEAT[ch])
                continue
            if ch == " ": continue
            c, b = cols[i]
            d.text((x, y), ch, font=bold if b else font, fill=C[c])
        y += lh
    img.save(f"{OUT}/{name}.png", optimize=True)
    print("wrote", name, img.size)


# ---------------------------------------------------------------- texts
SWEEP = """$ sprism toy
15,368 bytes   H 2.806 bits/byte   233 distinct values   5,389 bytes of information
identified as: elf (application/x-executable)
entropy band [4.0, 6.0]   verify on   16 threads

codec                     verdict         size    ratio    infoR        H   H/max   print  uniq     time
--------------------------------------------------------------------------------------------------------
lz77_litrun                   hit        7,376    0.480    0.907    5.302   0.666   25.1%   249   0.000s
zero_rle                      hit        6,802    0.443    0.923    5.851   0.744   42.9%   233   0.000s
lz77_shortdist                hit        7,732    0.503    0.941    5.248   0.658   21.7%   252   0.001s
lz77_varint                   hit        7,844    0.510    0.943    5.184   0.649   20.8%   254   0.001s
rle_escape                    hit        7,291    0.474    0.947    5.603   0.712   40.0%   233   0.000s
lz77_tokens                   hit        8,584    0.559    0.949    4.766   0.597   19.5%   252   0.000s
--------------------------------------------------------------------------------------------------------
lz4ish                     over-H        5,577    0.363    0.799    6.178   0.776   35.4%   250   0.001s
lz4                        over-H        5,644    0.367    0.805    6.149   0.772   35.2%   250   0.000s
brotli11                   over-H        3,439    0.224    0.631    7.916   0.990   36.5%   256   0.021s
xz9                        over-H        3,468    0.226    0.639    7.940   0.992   37.3%   256   0.022s
xz6                        over-H        3,468    0.226    0.639    7.941   0.993   37.3%   256   0.019s
xz6_arm                    over-H        3,468    0.226    0.639    7.941   0.993   37.3%   256   0.022s
--------------------------------------------------------------------------------------------------------
(42 more rows; --limit 0 for all)

hit smaller, H in band   over-H smaller, H above 6.0   recode same information   expand not smaller   BROKEN failed round-trip
infoR = size x H relative to the original; ranking key

best within the entropy band:
  lz77_litrun: 7,376 bytes, ratio 0.480, infoR 0.907, H 5.302

83 codecs in 0.15s"""


CARVE = """$ sprism fw.bin --carve
21,903 bytes   H 6.569   identified as: unrecognised

  ▒▓▓▒▒▓▓▓▓▒▓▒▓▓▓▒▒▓▒▒   ░███████████▓▓▓▓▒▒▓▓▒▓▒▓▒░  ▒██████████▓▒▓▓▓▒▓▓▓▒
  ^                ^                 ^                 ^                 ^
  0x0              0x1563            0x2ac7            0x402b            0x558f
  window 64 step 64   343 blocks
regions worth a closer look (entropy/--at target):
  --at 0x1c40 (0x1c40-0x29c0, 3,456 bytes)        mean H 7.94  packed (chi2 286, p=0.09, but has a stream header)
    has xz stream at 0x1c20: 3.4 KB -> 15.0 KB (ELF x86-64 executable)
  --at 0x3e00 (0x3e00-0x4b00, 3,328 bytes)        mean H 7.90  packed (chi2 521, p<0.001, but has a stream header)
    inside the gzip stream at 0x3c6c: 435 B -> 92.6 KB (text)

carve: 2 stream(s) in 21 high-entropy range(s), 17,728 bytes scanned
  offset   codec       stream -> decoded          payload
  0x1c20 xz             3,468 -> 15,368 bytes   H 2.81 structured   ELF x86-64 executable
  0x3c6c gzip             435 -> 94,800 bytes   H 4.45 structured   text"""

EMITC = """$ sprism toy --sections --emit-c
15,368 bytes -> 4,281 bytes (0.279)  [dependency-free codecs only]
  xz6 on the whole file: 3,468 (0.226), but it needs a library to unpack
28 chunk(s), 0 verbatim region(s)
  piece                                    at      bytes     packed       epk@ codec
  gap 0x0                                 0x0        792        216       0x2a libdeflate12
  .note.gnu.build-id+.init+gap          0x318         72         65      0x117 zero_rle
  .plt                                  0x360         64         53      0x170 libdeflate6
  .text                                 0x3a0        323        236      0x1bd libdeflate6
  gap+.fini+gap                         0x4e3      2,845         38      0x2be zero_rle
  .interp+gap+.gnu.hash                0x1000         68         57      0x2f9 zero_rle
  gap+.dynsym                          0x1044        172         45      0x34a libdeflate6
  (20 more pieces)
  .shstrtab                            0x30fd        395        201     0x107e deflate6
  gap 0x3288                           0x3288      2,432        519     0x1160 libdeflate12
  decoders: deflate6 = C template zlib_decode(); libdeflate12 = C template zlib_decode(); zero_rle = C template zero_rle_decode()
verified: unpacking reproduces the input exactly
wrote toy_unpack.c
  as a program:  cc -O2 -DSPRISM_MAIN -o unpack toy_unpack.c && ./unpack > restored.bin
  as a library:  sprism_unpack(buf, sizeof buf) -> bytes written, 0 on failure

$ cc -O2 -DSPRISM_MAIN -o unpack toy_unpack.c && ./unpack > restored.bin && cmp restored.bin toy && echo OK
OK"""



if __name__ == "__main__":
    terminal("sweep", "sprism toy — codec sweep", SWEEP)
    terminal("carve", "sprism fw.bin --carve — embedded streams", CARVE)
    terminal("emit-c", "sprism toy --sections --emit-c — dependency-free C unpacker", EMITC)
