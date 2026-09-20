"""Render the benchmark charts as committed SVG files.

Two files per chart, light and dark, because GitHub cannot restyle an embedded
image. Markdown picks between them with <picture> and prefers-color-scheme.

Palette slots 1-3 of the validated categorical default. Colour carries the
engine; every bar is also named on the axis and labelled with its value, so no
meaning rests on colour alone. Each panel has its own scale and unit, which is
why the value sits on every bar rather than on an axis.
"""
import pathlib

OUT = pathlib.Path(__file__).resolve().parent.parent / "docs" / "images"

THEMES = {
    "light": {
        "surface": "#fcfcfb",
        "primary": "#0b0b0b",
        "secondary": "#52514e",
        "grid": "#e4e3df",
        "series": ["#2a78d6", "#eb6834", "#1baf7a"],
    },
    "dark": {
        "surface": "#1a1a19",
        "primary": "#ffffff",
        "secondary": "#c3c2b7",
        "grid": "#383835",
        "series": ["#3987e5", "#d95926", "#199e70"],
    },
}

FONT = (
    "-apple-system,BlinkMacSystemFont,'Segoe UI',Helvetica,Arial,"
    "sans-serif"
)
ENGINES = ["This engine", "Snowstorm Lite", "Snowstorm"]
# What each was given for the run. Shown under the name on the latency chart,
# because a speed comparison means little without the hardware behind it.
ALLOCATIONS = ["1 CPU, 256 MiB", "1 CPU, 2 GiB", "8 CPUs, 12 GiB"]

BAR = 22          # <= 24px: never fill the band
RADIUS = 4        # rounded data-end
ROW = 34
LEFT = 150        # room for the engine names and their allocations
RIGHT = 96        # room for the value label
WIDTH = 720


def wrap(text, width=112):
    """Break a note into lines that fit the canvas at 11px."""
    lines, current = [], ""
    for word in text.split():
        candidate = f"{current} {word}".strip()
        if len(candidate) > width and current:
            lines.append(current)
            current = word
        else:
            current = candidate
    if current:
        lines.append(current)
    return lines


def esc(text):
    return text.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


def bar_path(x, y, length, height, radius):
    """A bar squared at the baseline and rounded at the data-end."""
    radius = min(radius, length)
    return (
        f"M{x},{y} H{x + length - radius} "
        f"A{radius},{radius} 0 0 1 {x + length},{y + radius} "
        f"V{y + height - radius} "
        f"A{radius},{radius} 0 0 1 {x + length - radius},{y + height} "
        f"H{x} Z"
    )


def panels_svg(theme_name, title, subtitle, panels, note, allocations=False):
    """Small multiples: one panel per measure, one bar per engine."""
    t = THEMES[theme_name]
    head = 58 if subtitle else 38
    panel_head = 30
    row_height = ROW + (8 if allocations else 0)
    note_lines = wrap(note)
    height = (head + sum(panel_head + row_height * len(p["values"]) + 18 for p in panels)
              + 20 + 14 * len(note_lines))
    out = [
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{WIDTH}" height="{height}" '
        f'viewBox="0 0 {WIDTH} {height}" font-family="{FONT}" '
        f'role="img" aria-label="{esc(title)}">',
        f'<rect width="{WIDTH}" height="{height}" fill="{t["surface"]}"/>',
        f'<text x="24" y="28" font-size="16" font-weight="600" '
        f'fill="{t["primary"]}">{esc(title)}</text>',
    ]
    if subtitle:
        out.append(
            f'<text x="24" y="48" font-size="12" fill="{t["secondary"]}">'
            f"{esc(subtitle)}</text>"
        )
    y = head
    for panel in panels:
        out.append(
            f'<text x="24" y="{y + 16}" font-size="12.5" font-weight="600" '
            f'fill="{t["primary"]}">{esc(panel["title"])}</text>'
        )
        y += panel_head
        row = ROW + (8 if allocations else 0)
        widest = max(v for v, _ in panel["values"])
        span = WIDTH - LEFT - RIGHT
        for index, (value, label) in enumerate(panel["values"]):
            row_y = y + index * row
            centre = row_y + row / 2
            length = max(2.0, span * value / widest)
            if allocations:
                out.append(
                    f'<text x="{LEFT - 12}" y="{centre - 1}" font-size="12" '
                    f'text-anchor="end" fill="{t["secondary"]}">'
                    f"{esc(ENGINES[index])}</text>"
                )
                out.append(
                    f'<text x="{LEFT - 12}" y="{centre + 12}" font-size="10" '
                    f'text-anchor="end" fill="{t["secondary"]}">'
                    f"{esc(ALLOCATIONS[index])}</text>"
                )
            else:
                out.append(
                    f'<text x="{LEFT - 12}" y="{centre + 4}" font-size="12" '
                    f'text-anchor="end" fill="{t["secondary"]}">'
                    f"{esc(ENGINES[index])}</text>"
                )
            out.append(
                f'<path d="{bar_path(LEFT, centre - BAR / 2, length, BAR, RADIUS)}" '
                f'fill="{t["series"][index]}"/>'
            )
            out.append(
                f'<text x="{LEFT + length + 10}" y="{centre + 4}" font-size="12" '
                f'font-weight="600" fill="{t["primary"]}">{esc(label)}</text>'
            )
        baseline = y + row * len(panel["values"])
        out.append(
            f'<line x1="{LEFT}" y1="{y - 2}" x2="{LEFT}" y2="{baseline - 2}" '
            f'stroke="{t["grid"]}" stroke-width="1"/>'
        )
        y = baseline + 18
    for offset, line in enumerate(note_lines):
        baseline_y = height - 14 - 14 * (len(note_lines) - 1 - offset)
        out.append(
            f'<text x="24" y="{baseline_y}" font-size="11" '
            f'fill="{t["secondary"]}">{esc(line)}</text>'
        )
    out.append("</svg>")
    return "\n".join(out)


def latency_svg(theme_name, title, subtitle, rows, note):
    """Median dot with a line to p95: one row per engine, one shared scale."""
    t = THEMES[theme_name]
    head = 58
    height = head + ROW * len(rows) + 52
    widest = max(p95 for _, _, p95 in rows)
    span = WIDTH - LEFT - RIGHT - 24
    out = [
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{WIDTH}" height="{height}" '
        f'viewBox="0 0 {WIDTH} {height}" font-family="{FONT}" '
        f'role="img" aria-label="{esc(title)}">',
        f'<rect width="{WIDTH}" height="{height}" fill="{t["surface"]}"/>',
        f'<text x="24" y="28" font-size="16" font-weight="600" '
        f'fill="{t["primary"]}">{esc(title)}</text>',
        f'<text x="24" y="48" font-size="12" fill="{t["secondary"]}">'
        f"{esc(subtitle)}</text>",
    ]
    for index, (label, median, p95) in enumerate(rows):
        centre = head + index * ROW + ROW / 2
        x_median = LEFT + span * median / widest
        x_p95 = LEFT + span * p95 / widest
        colour = t["series"][index]
        out.append(
            f'<text x="{LEFT - 12}" y="{centre + 4}" font-size="12" '
            f'text-anchor="end" fill="{t["secondary"]}">{esc(label)}</text>'
        )
        out.append(
            f'<line x1="{x_median}" y1="{centre}" x2="{x_p95}" y2="{centre}" '
            f'stroke="{colour}" stroke-width="2" stroke-linecap="round"/>'
        )
        # A 2px surface ring keeps the median dot legible where it meets the line.
        out.append(
            f'<circle cx="{x_median}" cy="{centre}" r="6" fill="{colour}" '
            f'stroke="{t["surface"]}" stroke-width="2"/>'
        )
        out.append(
            f'<text x="{x_p95 + 12}" y="{centre + 4}" font-size="12" '
            f'font-weight="600" fill="{t["primary"]}">'
            f"{median:.2f} / {p95:.2f} ms</text>"
        )
    baseline = head + ROW * len(rows)
    out.append(
        f'<line x1="{LEFT}" y1="{baseline}" x2="{LEFT + span}" y2="{baseline}" '
        f'stroke="{t["grid"]}" stroke-width="1"/>'
    )
    out.append(
        f'<text x="{LEFT}" y="{baseline + 18}" font-size="11" '
        f'fill="{t["secondary"]}">dot = median, line end = p95</text>'
    )
    out.append(
        f'<text x="24" y="{height - 12}" font-size="11" fill="{t["secondary"]}">'
        f"{esc(note)}</text>"
    )
    out.append("</svg>")
    return "\n".join(out)


def scaling_svg(theme_name, title, subtitle, series, note):
    """Log-log lines: cost against the number of concepts returned."""
    import math
    t = THEMES[theme_name]
    left, right, top = 64, 168, 74
    plot_w = WIDTH - left - right
    plot_h = 290
    plot_bottom = top + plot_h
    note_lines = wrap(note)
    # Room under the plot for tick labels, the axis title and the note.
    height = plot_bottom + 52 + 13 * len(note_lines)
    xs = [x for s in series for x, _ in s["points"]]
    ys = [y for s in series for _, y in s["points"]]
    x0, x1 = 0.0, math.ceil(math.log10(max(xs)))
    y0, y1 = math.floor(math.log10(min(ys))), math.ceil(math.log10(max(ys)))

    def px(v):
        return left + plot_w * (math.log10(v) - x0) / (x1 - x0)

    def py(v):
        return top + plot_h * (1 - (math.log10(v) - y0) / (y1 - y0))

    def ms(v):
        return f"{v:g} ms" if v < 1000 else f"{v / 1000:g} s"

    out = [
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{WIDTH}" height="{height}" '
        f'viewBox="0 0 {WIDTH} {height}" font-family="{FONT}" '
        f'role="img" aria-label="{esc(title)}">',
        f'<rect width="{WIDTH}" height="{height}" fill="{t["surface"]}"/>',
        f'<text x="24" y="28" font-size="16" font-weight="600" '
        f'fill="{t["primary"]}">{esc(title)}</text>',
        f'<text x="24" y="48" font-size="12" fill="{t["secondary"]}">{esc(subtitle)}</text>',
    ]
    for decade in range(int(y0), int(y1) + 1):
        y = py(10 ** decade)
        out.append(f'<line x1="{left}" y1="{y:.1f}" x2="{left + plot_w}" y2="{y:.1f}" '
                   f'stroke="{t["grid"]}" stroke-width="1"/>')
        out.append(f'<text x="{left - 8}" y="{y + 4:.1f}" font-size="10" text-anchor="end" '
                   f'fill="{t["secondary"]}">{ms(10 ** decade)}</text>')
    for decade in range(int(x0), int(x1) + 1):
        x = px(10 ** decade)
        label = f"{10 ** decade:,}" if decade < 6 else "1M"
        out.append(f'<text x="{x:.1f}" y="{plot_bottom + 16}" font-size="10" '
                   f'text-anchor="middle" fill="{t["secondary"]}">{label}</text>')
    out.append(f'<text x="{left + plot_w / 2:.0f}" y="{plot_bottom + 34}" font-size="11" '
               f'text-anchor="middle" fill="{t["secondary"]}">concepts returned</text>')

    for index, s in enumerate(series):
        colour = t["series"][index]
        points = " ".join(f"{px(x):.1f},{py(y):.1f}" for x, y in s["points"])
        out.append(f'<polyline points="{points}" fill="none" stroke="{colour}" '
                   f'stroke-width="2" stroke-linejoin="round" stroke-linecap="round"/>')
        for x, y in s["points"]:
            out.append(f'<circle cx="{px(x):.1f}" cy="{py(y):.1f}" r="4" fill="{colour}" '
                       f'stroke="{t["surface"]}" stroke-width="2"/>')
        last_x, last_y = s["points"][-1]
        out.append(f'<text x="{px(last_x) + 12:.1f}" y="{py(last_y) + 4:.1f}" font-size="11" '
                   f'font-weight="600" fill="{t["primary"]}">{esc(s["label"])}</text>')

    for offset, line in enumerate(note_lines):
        out.append(f'<text x="24" y="{plot_bottom + 54 + 13 * offset}" font-size="11" '
                   f'fill="{t["secondary"]}">{esc(line)}</text>')
    out.append("</svg>")
    return "\n".join(out)


def write(name, builder, *args, **options):
    OUT.mkdir(parents=True, exist_ok=True)
    for theme in THEMES:
        path = OUT / f"{name}-{theme}.svg"
        path.write_text(builder(theme, *args, **options) + "\n", encoding="utf-8")
        print("wrote", path.relative_to(OUT.parent.parent))


def expansion_scaling():
    """Read the ladder results so the chart cannot drift from the measurements."""
    import json
    path = OUT.parent.parent / "validation" / "expansion-size-results.json"
    if not path.exists():
        print("skipping scaling chart:", path, "not found")
        return
    rungs = json.loads(path.read_text(encoding="utf-8"))["rungs"]
    # The engine has no result cache, so its warm figure is its steady state.
    # Taking it also excludes the one-off index open from the first rung.
    write(
        "expansion-scaling",
        scaling_svg,
        "What a complete expansion costs as the result grows",
        "Every concept returned, one CPU and 256 MiB for the engine. Both axes are logarithmic.",
        [
            {"label": "This engine",
             "points": [(r["total"], r["engine_warm_ms"]) for r in rungs]},
            {"label": "Snowstorm, first ask",
             "points": [(r["total"], r["snowstorm_cold_ms"]) for r in rungs]},
            {"label": "Snowstorm, cached",
             "points": [(r["total"], r["snowstorm_warm_ms"]) for r in rungs]},
        ],
        "Snowstorm caches an expansion once asked, so its second answer is far quicker than "
        "its first. This engine has no result cache; every ask costs the same. Opening the "
        "index is a separate one-off of 116 to 293 ms.",
    )


if __name__ == "__main__":
    write(
        "footprint",
        panels_svg,
        "Setting up and serving one UK release",
        "One RF2 Snapshot, 1.15 million concepts. Each panel has its own scale.",
        [
            {
                "title": "Index on disk",
                "values": [(289.5, "290 MiB"), (483.3, "483 MiB"), (6256.6, "6.11 GiB")],
            },
            {
                "title": "Reading the release and building the indexes (one-off)",
                "values": [(119.8, "2.0 min"), (1057.0, "17.6 min"), (4360.0, "72.7 min")],
            },
            {
                "title": "Memory allocated to answer queries",
                "values": [(256, "256 MiB"), (2048, "2 GiB"), (12288, "12 GiB")],
            },
        ],
        "Allocations used in these runs, not measured minimums. Snowstorm counts "
        "both its service and Elasticsearch.",
    )
    write(
        "latency",
        panels_svg,
        "Asking for a total, and asking for every code",
        "Median over the 1,000-expression corpus, on each server's matched cohort.",
        [
            {
                "title": "Warm count — total only",
                "values": [(2.20, "2.20 ms"), (4.56, "4.56 ms"), (13.19, "13.19 ms")],
            },
            {
                "title": "Complete enumeration — every code returned",
                "values": [(2.29, "2.29 ms"), (7.15, "7.15 ms"), (36.40, "36.40 ms")],
            },
        ],
        "Engine cost is flat between the two; server cost is not. Snowstorm's "
        "allocation covers its service and Elasticsearch together. Includes "
        "transport: JSONL to a child process, or loopback HTTP with paging.",
        allocations=True,
    )
    expansion_scaling()
