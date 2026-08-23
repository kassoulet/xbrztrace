#!/usr/bin/env python3
"""Build the BRZtracer demo page.

For every PNG in ``demo/images/`` this script runs the release binary at the
configured scale factors and regenerates ``demo/index.html`` with an embedded
data table (dimensions, color/path counts, file sizes per scale).

Requires:
    - a release build: ``cargo build --release``
    - only the Python standard library

Usage:
    python3 scripts/build_demo.py
"""

from __future__ import annotations

import json
import os
import struct
import subprocess
import sys
import zlib

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
IMAGES_DIR = os.path.join(REPO_ROOT, "demo", "images")
OUTPUT_DIR = os.path.join(REPO_ROOT, "demo", "output")
INDEX_PATH = os.path.join(REPO_ROOT, "demo", "index.html")
BINARY = os.path.join(REPO_ROOT, "target", "release", "xbrztrace")
# xBRZ factors to generate per image. Large/noisy sources produce much
# bigger SVGs, so they get fewer (and smaller) scales.
IMAGE_SCALES: dict[str, list[int]] = {
    "ship-blue.png": [2, 4, 6],
    "ship-red.png": [2, 4, 6],
    "ship-green.png": [2, 4, 6],
    "asteroid.png": [2, 4, 6],
    "gem.png": [2, 4, 6],
    "character.png": [2, 4, 6],
    "creature.png": [2, 4, 6],
    "hero.png": [2, 4, 6],
    "ghost.png": [2, 4, 6],
    "tilemap.png": [2, 4],
    # Big game screenshot; 2x is plenty for on-page display.
    "sample_platformer.png": [2],
}

DEFAULT_SCALES = [2, 4, 6]

# Card order on the page. Images not listed sort alphabetically after these.
CARD_ORDER: list[str] = [
    "ship-blue.png",
    "ship-red.png",
    "ship-green.png",
    "asteroid.png",
    "gem.png",
    "character.png",
    "creature.png",
    "hero.png",
    "ghost.png",
    "tilemap.png",
    "sample_platformer.png",
]

PAGE_TITLE = "xBRZtrace — pixel art to crisp SVG"
TAGLINE = (
    "xBRZ upscaling + boundary tracing: every shape below is a vector path, "
    "not a pile of rectangles."
)


# ---------------------------------------------------------------------------
# Minimal PNG reader (standard library only)
# ---------------------------------------------------------------------------

def png_dimensions(path: str) -> tuple[int, int]:
    with open(path, "rb") as f:
        head = f.read(24)
    assert head[:8] == b"\x89PNG\r\n\x1a\n", f"not a PNG: {path}"
    w, h = struct.unpack(">II", head[16:24])
    return w, h


def png_distinct_colors(path: str) -> int:
    """Count distinct RGBA colors of a PNG (8-bit RGB/RGBA or indexed with
    any bit depth)."""
    with open(path, "rb") as f:
        data = f.read()
    pos = 8
    idat = b""
    bd = ct = None
    palette: list[bytes] = []
    trns = b""
    while pos < len(data):
        (length,) = struct.unpack(">I", data[pos:pos + 4])
        typ = data[pos + 4:pos + 8]
        payload = data[pos + 8:pos + 8 + length]
        if typ == b"IHDR":
            w, h, bd, ct, _, _, _ = struct.unpack(">IIBBBBB", payload[:13])
        elif typ == b"PLTE":
            palette = [payload[i:i + 3] for i in range(0, len(payload), 3)]
        elif typ == b"tRNS":
            trns = payload
        elif typ == b"IDAT":
            idat += payload
        pos += 12 + length
    assert bd in (1, 2, 4, 8), f"unsupported bit depth {bd}: {path}"
    assert ct in (0, 2, 3, 6), f"unsupported PNG color type {ct}: {path}"

    bpp = 4 if ct == 6 else (3 if ct == 2 else 1)
    raw = zlib.decompress(idat)
    scan_stride = (w * bd * bpp + 7) // 8
    prev = bytearray(scan_stride)
    samples = bytearray(w)
    colors = set()
    mask = (1 << bd) - 1
    for y in range(h):
        ftype = raw[y * (scan_stride + 1)]
        line = bytearray(raw[y * (scan_stride + 1) + 1:(y + 1) * (scan_stride + 1)])
        if ftype == 1:
            for i in range(bpp, scan_stride):
                line[i] = (line[i] + line[i - bpp]) & 0xFF
        elif ftype == 2:
            for i in range(scan_stride):
                line[i] = (line[i] + prev[i]) & 0xFF
        elif ftype == 3:
            for i in range(scan_stride):
                a = line[i - bpp] if i >= bpp else 0
                line[i] = (line[i] + ((a + prev[i]) >> 1)) & 0xFF
        elif ftype == 4:
            for i in range(scan_stride):
                a = line[i - bpp] if i >= bpp else 0
                b = prev[i]
                c = prev[i - bpp] if i >= bpp else 0
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                pr = a if (pa <= pb and pa <= pc) else (b if pb <= pc else c)
                line[i] = (line[i] + pr) & 0xFF
        for x in range(w):
            byte_idx = x * bd // 8
            bit_off = 8 - bd - (x * bd) % 8
            samples[x] = (line[byte_idx] >> bit_off) & mask
        for x in range(w):
            if ct == 3:
                idx = samples[x]
                r, g, b = palette[idx]
                a = trns[idx] if idx < len(trns) else 255
            elif ct == 0:
                v = samples[x] * 255 // mask
                r = g = b = v
                a = 255
            else:
                j = x * bpp
                r, g, b = (line[j], line[j + 1], line[j + 2])
                a = line[j + 3] if ct == 6 else 255
            colors.add((r, g, b, a))
        prev = line
    return len(colors)


# ---------------------------------------------------------------------------
# SVG stats
# ---------------------------------------------------------------------------

def svg_stats(path: str) -> dict:
    with open(path, "r", encoding="utf-8") as f:
        svg = f.read()
    width = height = 0
    for attr in ("width", "height"):
        val = svg.split(f'{attr}="', 1)[1].split('"', 1)[0]
        if attr == "width":
            width = int(val)
        else:
            height = int(val)
    return {
        "out_w": width,
        "out_h": height,
        "paths": svg.count("<path "),
        "loops": svg.count("Z"),
        "bytes": len(svg.encode("utf-8")),
    }


# ---------------------------------------------------------------------------
# Page generation
# ---------------------------------------------------------------------------

def find_binary() -> str | None:
    exe = "xbrztrace.exe" if os.name == "nt" else "xbrztrace"
    candidates = [os.path.join(REPO_ROOT, "target", "release", exe)]
    if "CARGO_TARGET_DIR" in os.environ:
        candidates.append(os.path.join(os.environ["CARGO_TARGET_DIR"], "release", exe))
    try:
        meta = subprocess.run(
            ["cargo", "metadata", "--format-version", "1", "--no-deps"],
            capture_output=True,
            text=True,
            cwd=REPO_ROOT,
        )
        if meta.returncode == 0:
            td = json.loads(meta.stdout)["target_directory"]
            candidates.append(os.path.join(td, "release", exe))
    except (OSError, json.JSONDecodeError):
        pass
    return next((c for c in candidates if os.path.exists(c)), None)


def build() -> None:
    binary = find_binary()
    if binary is None:
        sys.exit(
            "release binary not found; run `cargo build --release` first"
        )

    os.makedirs(OUTPUT_DIR, exist_ok=True)
    # Clear stale outputs from previous runs.
    for old in os.listdir(OUTPUT_DIR):
        os.remove(os.path.join(OUTPUT_DIR, old))
    images = [
        f for f in CARD_ORDER if os.path.exists(os.path.join(IMAGES_DIR, f))
    ]
    images += sorted(
        f for f in os.listdir(IMAGES_DIR) if f.lower().endswith(".png") and f not in images
    )
    if not images:
        sys.exit(f"no PNG images found in {IMAGES_DIR}")

    data = []
    for name in images:
        stem = os.path.splitext(name)[0]
        src = os.path.join(IMAGES_DIR, name)
        src_w, src_h = png_dimensions(src)
        src_colors = png_distinct_colors(src)

        scales = {}
        factors = IMAGE_SCALES.get(name, DEFAULT_SCALES)
        for factor in factors:
            out_name = f"{stem}_{factor}x.svg"
            out_path = os.path.join(OUTPUT_DIR, out_name)
            subprocess.run(
                [binary, "-i", src, "-o", out_path, "-s", f"{factor}x"],
                check=True,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            )
            scales[f"{factor}x"] = svg_stats(out_path)

        data.append(
            {
                "name": stem,
                "file": name,
                "src_w": src_w,
                "src_h": src_h,
                "src_colors": src_colors,
                "scales": scales,
            }
        )

    with open(INDEX_PATH, "w", encoding="utf-8") as f:
        f.write(render_page(data))

    total_svg_bytes = sum(
        s["bytes"] for entry in data for s in entry["scales"].values()
    )
    print(f"built {INDEX_PATH}: {len(data)} images, "
          f"{total_svg_bytes / 1024:.1f} KB of SVG")


def render_page(data: list[dict]) -> str:
    payload = json.dumps(data, separators=(",", ":"))
    cards = "\n".join(_render_card(entry) for entry in data)
    return (
        PAGE_HTML.replace("__TITLE__", PAGE_TITLE)
        .replace("__TAGLINE__", TAGLINE)
        .replace("/*__CARDS__*/", cards)
        .replace('"__DATA__"', payload)
    )


def _render_card(entry: dict) -> str:
    name = entry["name"]
    badge = (
        f'{entry["src_w"]}\u00d7{entry["src_h"]}px \u00b7 '
        f'{entry["src_colors"]} colors'
    )
    # Default scale the JS selects first: 4x when available, else the smallest.
    keys = list(entry["scales"])
    default = "4x" if "4x" in keys else keys[0]
    return f"""
      <section class="card">
        <header class="card-head">
          <h2>{name}</h2>
          <span class="badge">{badge}</span>
        </header>
        <div class="compare">
          <img class="layer orig" src="images/{entry['file']}" alt="{name} original">
          <div class="layer svgclip">
            <img class="svgimg" src="output/{name}_{default}.svg" alt="{name} as SVG">
          </div>
          <div class="divider" aria-hidden="true"><span></span></div>
        </div>
        <div class="controls">
          <span class="ctl-label">scale</span>
          <span class="scale-btns"></span>
          <span class="out-size"></span>
        </div>
        <ul class="stats">
          <li><b class="st-paths"></b><span>paths</span></li>
          <li><b class="st-loops"></b><span>loops</span></li>
          <li><b class="st-size"></b><span>svg size</span></li>
          <li><b class="st-ratio"></b><span>vs &lt;rect&gt;s</span></li>
        </ul>
      </section>"""


PAGE_HTML = """<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>__TITLE__</title>
<style>
  :root {
    --bg: #0d1117;
    --bg-2: #161b22;
    --bg-3: #1f2630;
    --border: #30363d;
    --fg: #e6edf3;
    --muted: #8b949e;
    --accent: #ffb86b;
    --accent-2: #56d364;
    --mono: "SFMono-Regular", Consolas, "Liberation Mono", Menlo, monospace;
    --sans: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
  }
  * { box-sizing: border-box; }
  html { color-scheme: dark; }
  body {
    margin: 0;
    background: var(--bg);
    color: var(--fg);
    font-family: var(--sans);
    line-height: 1.5;
  }
  a { color: var(--accent); text-decoration: none; }
  a:hover { text-decoration: underline; }

  header.hero {
    padding: 3.5rem 1.5rem 2.5rem;
    text-align: center;
    border-bottom: 1px solid var(--border);
    background:
      radial-gradient(1200px 400px at 50% -100px, rgba(86, 211, 100, 0.10), transparent),
      var(--bg);
  }
  .brand {
    font-family: var(--mono);
    font-size: 2.4rem;
    font-weight: 700;
    letter-spacing: -0.04em;
  }
  .brand b { color: var(--accent); }
  header.hero h1 { font-size: 1.15rem; font-weight: 600; margin: 0.4rem 0 0; }
  .tagline { color: var(--muted); max-width: 56ch; margin: 0.6rem auto 1.2rem; }
  .usage {
    display: inline-block;
    font-family: var(--mono);
    font-size: 0.85rem;
    background: var(--bg-2);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 0.55rem 0.9rem;
    color: var(--fg);
  }
  .usage .dollar { color: var(--muted); }

  main { max-width: 1200px; margin: 0 auto; padding: 2rem 1.5rem 3rem; }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(340px, 1fr));
    gap: 1.5rem;
  }

  .card {
    background: var(--bg-2);
    border: 1px solid var(--border);
    border-radius: 12px;
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }
  .card-head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.75rem;
    padding: 0.9rem 1.1rem 0.7rem;
  }
  .card-head h2 {
    margin: 0;
    font-family: var(--mono);
    font-size: 1.05rem;
    font-weight: 600;
  }
  .badge {
    font-size: 0.75rem;
    color: var(--muted);
    font-family: var(--mono);
    white-space: nowrap;
  }

  /* -------- before/after compare -------- */
  .compare {
    position: relative;
    width: 100%;
    height: 230px;
    margin: 0 auto;
    background:
      repeating-conic-gradient(#1a2029 0% 25%, #141a22 0% 50%) 0 0 / 22px 22px;
    cursor: col-resize;
    touch-action: none;
    user-select: none;
    -webkit-user-select: none;
  }
  .compare .layer {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }
  .compare .orig {
    image-rendering: pixelated;
    image-rendering: crisp-edges;
    object-fit: contain;
    padding: 0.75rem;
  }
  .compare .svgclip {
    clip-path: inset(0 0 0 50%);
  }
  .compare .svgimg {
    width: 100%;
    height: 100%;
    object-fit: contain;
    padding: 0.75rem;
  }
  .compare .divider {
    position: absolute;
    top: 0;
    bottom: 0;
    left: 50%;
    width: 2px;
    background: var(--accent);
    transform: translateX(-1px);
    pointer-events: none;
  }
  .compare .divider::before,
  .compare .divider::after {
    content: "\\25C0 \\25B6";
    position: absolute;
    left: 50%;
    transform: translateX(-50%);
    top: 0.5rem;
    font-size: 0.7rem;
    color: var(--bg);
    background: var(--accent);
    border-radius: 999px;
    padding: 0.15rem 0.5rem;
    letter-spacing: 2px;
  }
  .compare .divider::after { content: none; }
  .compare-label {
    position: absolute;
    bottom: 0.4rem;
    font-size: 0.66rem;
    font-family: var(--mono);
    background: rgba(13, 17, 23, 0.85);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 0.15rem 0.5rem;
    color: var(--muted);
    pointer-events: none;
  }
  .label-orig { left: 0.5rem; }
  .label-svg { right: 0.5rem; }

  /* -------- controls & stats -------- */
  .controls {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.8rem 1.1rem;
    border-top: 1px solid var(--border);
    border-bottom: 1px solid var(--border);
  }
  .ctl-label {
    font-size: 0.72rem;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--muted);
    margin-right: 0.35rem;
  }
  .scale-btns { display: flex; gap: 0.4rem; }
  .scale-btn {
    appearance: none;
    background: var(--bg-3);
    color: var(--fg);
    border: 1px solid var(--border);
    border-radius: 6px;
    font-family: var(--mono);
    font-size: 0.8rem;
    padding: 0.28rem 0.7rem;
    cursor: pointer;
  }
  .scale-btn:hover { border-color: var(--muted); }
  .scale-btn.active {
    background: var(--accent);
    border-color: var(--accent);
    color: #14181f;
    font-weight: 700;
  }
  .out-size { margin-left: auto; font-family: var(--mono); font-size: 0.78rem; color: var(--muted); }

  .stats {
    list-style: none;
    margin: 0;
    padding: 0.85rem 1.1rem 1rem;
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 0.5rem;
  }
  .stats li { text-align: center; }
  .stats b { display: block; font-family: var(--mono); font-size: 1.05rem; font-weight: 600; }
  .stats span { font-size: 0.68rem; color: var(--muted); text-transform: uppercase; letter-spacing: 0.05em; }

  footer {
    border-top: 1px solid var(--border);
    padding: 2rem 1.5rem 3rem;
    color: var(--muted);
    font-size: 0.85rem;
    text-align: center;
    max-width: 760px;
    margin: 0 auto;
  }
  footer p { margin: 0.4rem 0; }
  code { font-family: var(--mono); background: var(--bg-2); padding: 0.1rem 0.35rem; border-radius: 4px; font-size: 0.85em; }
  @media (max-width: 560px) {
    .compare { height: 190px; }
    .stats b { font-size: 0.9rem; }
  }
</style>
</head>
<body>

<header class="hero">
  <div class="brand"><b>xBRZ</b>trace</div>
  <h1>Pixel art &rarr; crisp SVG</h1>
  <p class="tagline">__TAGLINE__</p>
  <div class="usage"><span class="dollar">$ </span>xbrztrace -i sprite.png -o sprite.svg -s 4x</div>
</header>

<main>
  <div class="grid">
/*__CARDS__*/
  </div>
</main>

<footer>
  <p>Every SVG on this page was produced by the <code>xbrztrace</code> release binary
     from the PNG beside it &mdash; no hand editing.</p>
  <p>Drag the divider to compare the original pixels against the traced vectors;
     use the scale buttons to switch xBRZ factors.</p>
  <p>Demo images: <a href="https://kenney.nl/assets/pixel-shmup">Kenney “Pixel Shmup”</a>
     (<code>ship-blue</code>, <code>ship-red</code>, <code>ship-green</code>, <code>asteroid</code>, <code>gem</code>),
     <a href="https://kenney.nl/assets/pixel-platformer">Kenney “Pixel Platformer”</a>,
     <a href="https://kenney.nl/assets/1-bit-pack">Kenney “1-Bit Pack”</a> — all CC0,
     public domain — plus the in-house <code>ghost</code> test sprite. See <code>demo/README.md</code>.</p>
  <p>xBRZtrace is MIT licensed.</p>
</footer>

<script>
const DATA = "__DATA__";
const grid = document.querySelector(".grid");

function fmtBytes(n) {
  return n >= 1024 ? (n / 1024).toFixed(1) + " KB" : n + " B";
}

function ratioFor(entry, scaleKey) {
  const s = entry.scales[scaleKey];
  const rects = s.out_w * s.out_h;
  return (rects / s.paths).toFixed(0) + "\u00d7";
}

const cards = grid.querySelectorAll(".card");
cards.forEach((card, idx) => {
  const entry = DATA[idx];
  const cmp = card.querySelector(".compare");
  const svgImg = card.querySelector(".svgimg");
  const outSize = card.querySelector(".out-size");
  const stPaths = card.querySelector(".st-paths");
  const stLoops = card.querySelector(".st-loops");
  const stSize = card.querySelector(".st-size");
  const stRatio = card.querySelector(".st-ratio");
  const labels = document.createElement("div");
  labels.innerHTML = '<span class="compare-label label-orig">pixels</span><span class="compare-label label-svg">svg</span>';
  cmp.appendChild(labels);

  const scaleKeys = Object.keys(entry.scales);
  let scale = scaleKeys.includes("4x") ? "4x" : scaleKeys[0];

  function applyScale() {
    const s = entry.scales[scale];
    svgImg.src = `output/${entry.name}_${scale}.svg`;
    outSize.textContent = `${s.out_w}\u00d7${s.out_h}`;
    stPaths.textContent = s.paths;
    stLoops.textContent = s.loops;
    stSize.textContent = fmtBytes(s.bytes);
    stRatio.textContent = ratioFor(entry, scale);
  }

  const btnWrap = card.querySelector(".scale-btns");
  for (const key of scaleKeys) {
    const btn = document.createElement("button");
    btn.className = "scale-btn" + (key === scale ? " active" : "");
    btn.dataset.scale = key;
    btn.textContent = key;
    btn.addEventListener("click", () => {
      btnWrap.querySelectorAll(".scale-btn").forEach(b => b.classList.remove("active"));
      btn.classList.add("active");
      scale = btn.dataset.scale;
      applyScale();
    });
    btnWrap.appendChild(btn);
  }

  // before/after divider drag
  function setSplit(pct) {
    pct = Math.max(4, Math.min(96, pct));
    cmp.querySelector(".svgclip").style.clipPath = `inset(0 0 0 ${pct}%)`;
    cmp.querySelector(".divider").style.left = pct + "%";
  }
  cmp.addEventListener("pointerdown", e => {
    cmp.setPointerCapture(e.pointerId);
    const move = ev => {
      const r = cmp.getBoundingClientRect();
      setSplit(((ev.clientX - r.left) / r.width) * 100);
    };
    move(e);
    cmp.addEventListener("pointermove", move);
    cmp.addEventListener("pointerup", () => cmp.removeEventListener("pointermove", move), { once: true });
  });

  applyScale();
});
</script>
</body>
</html>
"""


if __name__ == "__main__":
    build()
