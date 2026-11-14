"""Build the local Material Symbols font from the names in the UI source."""

import argparse
import re
from io import BytesIO
from pathlib import Path

from fontTools import subset
from fontTools.ttLib import TTFont


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "scripts/assets/material-symbols-outlined.ttf"
OUTPUT = ROOT / "src/assets/fonts/material-symbols-outlined.woff2"
SOURCE_EXTENSIONS = {".html", ".js", ".css"}
LITERAL_ICON = re.compile(
    r'<span\b[^>]*\bmaterial-symbols-outlined\b[^>]*>\s*([A-Za-z0-9_]+)\s*</span>'
)


def build_font():
    # Match complete tokens against the font's actual ligatures. This includes
    # names in HTML, JavaScript templates, icon pickers and status mappings.
    tokens = set()
    literal_icons = {}
    for path in (ROOT / "src").rglob("*"):
        if path.suffix in SOURCE_EXTENSIONS:
            source = path.read_text(encoding="utf-8")
            tokens.update(re.findall(r"\b[a-z0-9_]+\b", source))
            for match in LITERAL_ICON.finditer(source):
                literal_icons.setdefault(match.group(1), path.relative_to(ROOT))

    with TTFont(SOURCE, recalcTimestamp=False) as font:
        letters = {glyph: chr(code) for code, glyph in font.getBestCmap().items() if code < 128}
        names = set()
        for lookup in font["GSUB"].table.LookupList.Lookup:
            for subtable in lookup.SubTable:
                table = getattr(subtable, "ExtSubTable", subtable)
                if not hasattr(table, "ligatures"):
                    continue
                for first, entries in list(table.ligatures.items()):
                    kept = []
                    for entry in entries:
                        name = "".join(letters.get(glyph, "?") for glyph in (first, *entry.Component))
                        if name in tokens:
                            kept.append(entry)
                            names.add(name)
                    if kept:
                        table.ligatures[first] = kept
                    else:
                        del table.ligatures[first]

        missing = literal_icons.keys() - names
        if missing:
            details = "\n".join(f"{literal_icons[name]}: {name}" for name in sorted(missing))
            raise SystemExit(f"Unknown Material Symbols ligatures (use lowercase names):\n{details}")

        # Keep character glyphs so the browser can shape the retained names.
        options = subset.Options()
        options.layout_features = ["*"]
        builder = subset.Subsetter(options=options)
        builder.populate(text="".join(sorted(names)))
        builder.subset(font)
        font.flavor = "woff2"
        output = BytesIO()
        font.save(output)
    return output.getvalue(), len(names)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="Verify the checked-in font")
    args = parser.parse_args()
    data, count = build_font()
    if args.check:
        if not OUTPUT.is_file() or OUTPUT.read_bytes() != data:
            parser.exit(1, "Material Symbols font is out of date. Regenerate it without --check.\n")
    else:
        OUTPUT.write_bytes(data)
    print(f"{count} Material Symbols ligatures, {len(data):,} bytes")


if __name__ == "__main__":
    main()
