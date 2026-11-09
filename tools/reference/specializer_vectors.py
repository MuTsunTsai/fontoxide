"""Generates crates/fontoxide-ttlib/tests/data/specializer_vectors.txt.

Runs generalizeProgram and specializeProgram of fontTools on hand-picked and random
charstring programs, to check paths of the specializer that the font corpus doesn't reach.

    uv run --project tools/reference python tools/reference/specializer_vectors.py [COUNT [OUT]]

Each line has, separated by tabs:

- G (generalizeProgram) or S (specializeProgram);
- the regions of each vsindex, separated by commas, or "-" for getNumRegions=None;
- vsIndex;
- the options: t (preserveTopology), n (generalizeFirst=False), m (maxstack=513), or "-";
- the program;
- the result; or !V if it raises ValueError, !E if it raises anything else.
"""

import pathlib
import random
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "upstream" / "fonttools" / "Lib"))
sys.dont_write_bytecode = True

from fontTools.cffLib.specializer import (
    generalizeProgram,
    programToString,
    specializeProgram,
    stringToProgram,
)

OUT = ROOT / "crates" / "fontoxide-ttlib" / "tests" / "data" / "specializer_vectors.txt"

HAND_PICKED = [
    # Nothing, and stray arguments.
    (None, ""),
    (None, "1 2 3"),
    (None, "endchar"),
    (None, "100 endchar"),
    (None, "1 2 3 4 endchar"),
    # Hints.
    (None, "1 2 hstem 3 4 vstem 5 6 rmoveto"),
    (None, "9 1 2 hstemhm 3 4 hintmask 12 5 6 cntrmask 34 7 8 rmoveto"),
    (None, "hintmask"),
    (None, "hintmask rmoveto"),
    (None, "1 2 hintmask cntrmask 3 4 rmoveto"),
    # Blends that aren't what they should be.
    ([1], "blend"),
    ([1], "1 blend"),
    ([1], "5 blend rmoveto"),
    ([1], "1 2 0 blend rmoveto"),
    ([1], "1 2 -1 blend rmoveto"),
    ([1], "1 2 3 -2 blend 4 rmoveto"),
    ([1], "1 2 1.5 blend rmoveto"),
    ([2], "1 2 3 4 5 6 7 8 2 blend rmoveto"),
    ([2], "1 2 3 1 blend 4 rmoveto 5 6 7 1 blend 8 rmoveto"),
    ([2], "1 2 3 1 blend 4 5 6 1 blend rlineto"),
    ([1, 2], "1 vsindex 10 20 30 1 blend 40 rmoveto"),
    ([1, 2], "1.5 vsindex 10 20 1 blend 40 rmoveto"),
    ([1, 2], "vsindex 10 20 1 blend 40 rmoveto"),
    ([1, 2], "1.5 1 vsindex 10 20 30 1 blend 40 rmoveto"),
    ([1, 2], "2 vsindex 10 20 1 blend 40 rmoveto"),
    ([0], "100 200 1 blend 2 blend rmoveto"),
    # Merging lines with different blends.
    ([1], "10 1 1 blend 0 rlineto 20 2 1 blend 0 rlineto 30 0 rlineto"),
    ([1], "0 10 1 1 blend rlineto 0 20 2 1 blend rlineto"),
    ([1], "10 0 rmoveto 20 1 1 blend 0 rmoveto 5 2 1 blend 0 rmoveto"),
    # Flex.
    (None, "1 2 3 4 5 0 6 0 7 8 9 10 50 flex"),
    (None, "1 2 3 4 5 0 6 0 7 8 9 -18 51 flex"),
    (None, "1 2 3 flex"),
    (None, "1 0 2 3 4 0 5 0 6 -3 7 0 50 flex"),
    (None, "10 20 30 0 40 -20 50 20 60 0 70 -20 50 flex"),
    (None, "1 2 3 4 5 hflex"),
    (None, "1 2 3 4 5 6 7 hflex"),
    (None, "1 2 3 4 5 6 7 8 9 hflex1"),
    (None, "1 2 3 4 5 6 7 8 9 10 11 flex1"),
    (None, "11 2 3 4 5 6 7 8 9 10 11 flex1"),
    ([1], "1 2 1 blend 3 4 5 6 7 8 9 10 11 12 flex1"),
    ([1], "10 20 1 blend 1 2 1 blend 0 0 0 0 0 0 0 0 0 flex1"),
    ([1], "10 -20 1 blend 5 -5 1 blend 0 0 0 0 0 0 0 0 0 flex1"),
    ([1], "1 1 1 blend 0 2 3 4 0 5 0 6 -3 7 0 50 flex"),
    # The other options.
    (None, "1 2 3 4 rlineto 5 0 0 6 rlineto"),
    (None, "1 2 3 rmoveto 4 5 rmoveto"),
    (None, "1 rlineto"),
    (None, "1 2 3 rcurveline"),
    (None, "0 0 1 2 0 0 rrcurveto 1 2 rrcurveto"),
    (None, "1 2 rrcurveto 3 rrcurveto"),
    (None, "1 2 3 4 5 6 7 8 rrcurveto"),
    (None, "0 0 0 0 0 0 rrcurveto 0 0 0 0 0 0 rrcurveto"),
    (None, "0 0 rmoveto 0 0 rlineto 0 0 rlineto"),
]

CURVE_OPERATORS = {
    "rrcurveto": lambda r: 6 * r.randint(1, 3),
    "hhcurveto": lambda r: 4 * r.randint(1, 3) + r.randint(0, 1),
    "vvcurveto": lambda r: 4 * r.randint(1, 3) + r.randint(0, 1),
    "hvcurveto": lambda r: 4 * r.randint(1, 4) + r.randint(0, 1),
    "vhcurveto": lambda r: 4 * r.randint(1, 4) + r.randint(0, 1),
    "rcurveline": lambda r: 6 * r.randint(1, 2) + 2,
    "rlinecurve": lambda r: 2 * r.randint(1, 3) + 6,
}

OTHER_OPERATORS = {
    "rmoveto": lambda r: 2,
    "hmoveto": lambda r: 1,
    "vmoveto": lambda r: 1,
    "rlineto": lambda r: 2 * r.randint(1, 4),
    "hlineto": lambda r: r.randint(1, 5),
    "vlineto": lambda r: r.randint(1, 5),
    "flex": lambda r: 13,
    "hflex": lambda r: 7,
    "hflex1": lambda r: 9,
    "flex1": lambda r: 11,
    "hstem": lambda r: 2 * r.randint(1, 2),
    "vstemhm": lambda r: 2 * r.randint(1, 2),
    "callsubr": lambda r: 1,
    "xtra": lambda r: r.randint(0, 2),
}

OPERATORS = list(CURVE_OPERATORS.items()) + list(OTHER_OPERATORS.items())


def number(r):
    kind = r.random()
    if kind < 0.45:
        return 0
    if kind < 0.8:
        return r.randint(-10, 10)
    if kind < 0.9:
        return r.choice([107, 108, -107, -108, 300, 1131, 1132, -2000, 32767, 40000])
    return r.choice([0.5, -0.5, 1.25, 0.1, -2.75, 0.0, 3.0])


def blended(r, regions):
    """The tokens of some numbers that are blended together."""
    count = r.choice([1, 1, 1, 2, 3])
    tokens = [number(r) for _ in range(count)]
    tokens += [number(r) for _ in range(count * regions)]
    return tokens + [count, "blend"], count


def arguments(r, count, regions, blends):
    tokens = []
    left = count
    while left > 0:
        if blends and r.random() < blends:
            new, n = blended(r, regions)
            tokens += new
            left -= n
        else:
            tokens.append(number(r))
            left -= 1
    return tokens


def random_program(r):
    regions = r.choice([None, None, 0, 1, 1, 2, 3])
    blends = 0 if regions is None else r.choice([0, 0.1, 0.5, 1])
    program = []
    if r.random() < 0.3:
        program.append(number(r))
    mode = r.random()
    if mode < 0.35:
        # Only curves or only lines, to merge them.
        operators = r.choice([["rrcurveto"], ["rlineto"], ["rrcurveto", "rlineto"]])
        for _ in range(r.randint(1, 12)):
            operator = r.choice(operators)
            count = 6 if operator == "rrcurveto" else 2
            program += arguments(r, count, regions, blends) + [operator]
    else:
        if r.random() < 0.15 and regions is not None:
            program += [r.randint(0, 1), "vsindex"]
        for _ in range(r.randint(1, 5)):
            operator, count = r.choice(OPERATORS)
            count = count(r)
            if r.random() < 0.08:
                count += r.choice([-2, -1, 1, 2])
            program += arguments(r, max(count, 0), regions, blends) + [operator]
            if r.random() < 0.05:
                program += [r.choice(["hintmask", "cntrmask"]), r.randint(1, 255)]
    return regions if regions is None else [regions, r.randint(0, 3)], program


def run(function, program, **kwargs):
    try:
        result = function(program, **kwargs)
    except ValueError:
        return "!V"
    except Exception:
        return "!E"
    if any(isinstance(token, list) for token in result):
        # A list in a program: it can't be represented.
        return None
    return programToString(result)


def vectors(regions, program, r):
    text = programToString(program)
    if regions is None:
        get_num_regions = None
        regions_text = "-"
    else:
        get_num_regions = dict(enumerate(regions)).get
        regions_text = ",".join(str(n) for n in regions)
    vs_index = 1 if regions and len(regions) > 1 and r.random() < 0.1 else 0
    common = dict(getNumRegions=get_num_regions, vsIndex=vs_index)
    configurations = [
        ("G", {}, "-", generalizeProgram),
        ("S", {}, "-", specializeProgram),
        ("S", dict(preserveTopology=True), "t", specializeProgram),
        ("S", dict(generalizeFirst=False), "n", specializeProgram),
        ("S", dict(maxstack=513), "m", specializeProgram),
    ]
    for kind, kwargs, flags, function in configurations:
        result = run(function, list(program), **common, **kwargs)
        if result is not None:
            yield "\t".join([kind, regions_text, str(vs_index), flags, text, result])


def main(count, out):
    r = random.Random(20241109)
    lines = []
    for regions, text in HAND_PICKED:
        lines.extend(vectors(regions, stringToProgram(text), r))
    for _ in range(count):
        regions, program = random_program(r)
        lines.extend(vectors(regions, program, r))
    with open(out, "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(lines) + "\n")
    print(f"{len(lines)} vectors in {out}")


if __name__ == "__main__":
    main(
        int(sys.argv[1]) if len(sys.argv) > 1 else 250,
        pathlib.Path(sys.argv[2]) if len(sys.argv) > 2 else OUT,
    )
