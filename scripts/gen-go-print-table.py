#!/usr/bin/env python3
"""Writes the table of characters Go's `unicode.IsPrint` takes, which
`strconv.Quote` writes as they are, to crates/domain/src/secrets/go_print.rs
(technical design §10.2).

Go prints a letter, mark, number, punctuation or symbol (general categories
L*, M*, N*, P*, S*) and the space U+0020, by the categories of the Unicode
version its `unicode` package was generated from (`unicode.Version`). Run this
with that version whenever the Go that rclone is built with moves to a new one.

Usage: python3 scripts/gen-go-print-table.py <unicode-version> [UnicodeData.txt]

Without a file it downloads UnicodeData.txt for that version from unicode.org.
"""

import sys
import urllib.request
from pathlib import Path

OUTPUT = Path(__file__).resolve().parent.parent / "crates/domain/src/secrets/go_print.rs"
PRINTED_CATEGORIES = ("L", "M", "N", "P", "S")
SPACE = 0x20


def source_url(version):
    return f"https://www.unicode.org/Public/{version}/ucd/UnicodeData.txt"


def printed_ranges(lines):
    """The sorted, merged inclusive ranges of the code points Go prints.

    A line is `code;name;category;…`. A block of code points sharing one
    category (CJK ideographs, Hangul syllables, Tangut…) is a pair of lines
    named `<…, First>` and `<…, Last>`.
    """
    ranges = []
    first = None
    for line in lines:
        line = line.strip()
        if not line:
            continue
        fields = line.split(";")
        code, name, category = int(fields[0], 16), fields[1], fields[2]
        if name.endswith(", First>"):
            first = code
            continue
        start = code
        if name.endswith(", Last>"):
            if first is None:
                sys.exit(f"{name} at U+{code:04X} without its First line")
            start, first = first, None
        if not (category.startswith(PRINTED_CATEGORIES) or code == SPACE):
            continue
        if ranges and start <= ranges[-1][1]:
            sys.exit(f"U+{start:04X} out of order")
        if ranges and start == ranges[-1][1] + 1:
            ranges[-1][1] = code
        else:
            ranges.append([start, code])
    if first is not None:
        sys.exit("a First line without its Last line")
    return ranges


def render(version, ranges):
    rows = "".join(f"    (0x{first:04x}, 0x{last:04x}),\n" for first, last in ranges)
    return (
        "//! The characters Go's `strconv.Quote` writes as they are (`\"` and `\\`\n"
        "//! aside): those `unicode.IsPrint` takes, the letters, marks, numbers,\n"
        "//! punctuation and symbols, and the space (technical design §10.2).\n"
        "//!\n"
        f"//! Generated from Unicode {version}'s character database,\n"
        f"//! <{source_url(version)}>, by\n"
        f"//! `python3 scripts/gen-go-print-table.py {version}`. Don't edit it by hand.\n"
        "\n"
        "/// The inclusive ranges of the code points Go prints, sorted and merged.\n"
        f"pub(super) const GO_PRINT: [(u32, u32); {len(ranges)}] = [\n"
        f"{rows}"
        "];\n"
    )


def main(args):
    if len(args) not in (1, 2):
        sys.exit(__doc__)
    version = args[0]
    if len(args) == 2:
        text = Path(args[1]).read_text(encoding="utf-8")
    else:
        with urllib.request.urlopen(source_url(version)) as response:
            text = response.read().decode("utf-8")
    OUTPUT.write_text(render(version, printed_ranges(text.splitlines())), encoding="utf-8")


if __name__ == "__main__":
    main(sys.argv[1:])
