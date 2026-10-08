"""Generate tests/data/python_compat_cases.json from real Python and polib.

odoo-lint emulates Python's ``%`` and ``str.format`` errors and polib's PO
writer. This script records what the real implementations do on generated
inputs, so ``tests/python_compat.rs`` can check odoo-lint against them.

    uv run --with polib==1.2.0 python scripts/gen_python_compat_cases.py
"""

from __future__ import annotations

import json
import random
from pathlib import Path

import polib

OUT = (
    Path(__file__).resolve().parent.parent
    / "tests"
    / "data"
    / "python_compat_cases.json"
)
SEED = 20261008

PERCENT_PIECES = [
    "%s",
    "%d",
    "%f",
    "%r",
    "%x",
    "%c",
    "%5.2f",
    "%-3s",
    "%(a)s",
    "%(b)d",
    "%(a",
    "%%",
    "%y",
    "%*d",
    "%.*f",
    "% d",
    "%ld",
    "%",
    "text ",
    "100%% ",
    "%(a)s%(a)d",
]
FORMAT_PIECES = [
    "{}",
    "{0}",
    "{1}",
    "{a}",
    "{b}",
    "{0.real}",
    "{0.x}",
    "{a.imag}",
    "{0[0]}",
    "{!r}",
    "{0!s}",
    "{0!x}",
    "{:d}",
    "{:s}",
    "{0:.2f}",
    "{0:.2d}",
    "{:,}",
    "{:_x}",
    "{:,s}",
    "{0:>{1}}",
    "{ 'v'}",
    "{{",
    "}}",
    "{",
    "}",
    "{0:z}",
    "{:=5}",
    "{:#x}",
    "text ",
]
WORDS = [
    "Odoo",
    "invoice",
    "partner",
    "the",
    "a",
    "well-known",
    "e-mail",
    "--",
    "x" * 30,
    "ünïcödé",
    "tab\there",
]


def py_error(func, *args, **kwargs):
    """``repr()`` of the exception ``func(*args, **kwargs)`` raises, or "ok"."""
    try:
        func(*args, **kwargs)
    except Exception as exc:  # every exception is a result
        return repr(exc)
    return "ok"


def percent_cases(rng: random.Random) -> list[dict]:
    cases = []
    for _ in range(400):
        fmt = "".join(rng.choice(PERCENT_PIECES) for _ in range(rng.randint(1, 4)))
        if rng.random() < 0.5:
            values = [rng.choice(["s", 0]) for _ in range(rng.randint(0, 3))]
            args = tuple("" if v == "s" else 0 for v in values)
            spec = {"tuple": values}
        else:
            values = {
                k: rng.choice(["s", 0])
                for k in rng.sample(["a", "b"], rng.randint(1, 2))
            }
            args = {k: "" if v == "s" else 0 for k, v in values.items()}
            spec = {"dict": values}
        cases.append(
            {"format": fmt, "args": spec, "expected": py_error(str.__mod__, fmt, args)}
        )
    return cases


def format_cases(rng: random.Random) -> list[dict]:
    cases = []
    for _ in range(400):
        fmt = "".join(rng.choice(FORMAT_PIECES) for _ in range(rng.randint(1, 3)))
        args = list(range(rng.randint(0, 2)))
        kwargs = {k: 0 for k in rng.sample(["a", "b"], rng.randint(0, 2))}
        cases.append(
            {
                "format": fmt,
                "args": args,
                "kwargs": sorted(kwargs),
                "expected": py_error(fmt.format, *args, **kwargs),
            }
        )
    return cases


def random_text(rng: random.Random, words: int) -> str:
    text = " ".join(rng.choice(WORDS) for _ in range(words))
    if rng.random() < 0.2:
        text += '"quoted" \\back'
    if rng.random() < 0.15:
        text = text.replace(" ", "\n", 1)
    return text


def po_cases(rng: random.Random) -> list[dict]:
    cases = []
    for _ in range(60):
        po = polib.POFile()
        po.header = random_text(rng, rng.randint(1, 6))
        po.metadata = {
            "Project-Id-Version": "Odoo Server 17.0",
            "Content-Type": "text/plain; charset=UTF-8",
            "Language": "nl",
            "X-Custom-2": "b",
            "X-Custom-10": "a",
        }
        for _ in range(rng.randint(1, 6)):
            entry = polib.POEntry(
                msgid=random_text(rng, rng.randint(1, 25)),
                msgstr=random_text(rng, rng.randint(0, 25)),
                comment="module: acme " + random_text(rng, rng.randint(0, 20)),
                tcomment=random_text(rng, rng.randint(0, 3))
                if rng.random() < 0.3
                else "",
                occurrences=[
                    (
                        f"model:ir.model.fields,field_description:acme-sale.field_{i}-x",
                        "",
                    )
                    for i in range(rng.randint(0, 4))
                ],
                flags=["python-format"] if rng.random() < 0.3 else [],
            )
            if rng.random() < 0.2:
                entry.msgctxt = "ctx"
            if rng.random() < 0.15:
                entry.msgid_plural = random_text(rng, 3)
                entry.msgstr_plural = {0: random_text(rng, 2), 1: random_text(rng, 30)}
            if rng.random() < 0.1:
                entry.obsolete = True
            po.append(entry)
        source = str(po)
        # Shuffle the written file's entries to also test parsing unsorted input.
        cases.append({"input": source, "expected": str(polib.pofile(source))})
    return cases


def main() -> None:
    rng = random.Random(SEED)  # noqa: S311 - reproducible test data
    data = {
        "generator": "scripts/gen_python_compat_cases.py",
        "polib": polib.__version__,
        "percent": percent_cases(rng),
        "format": format_cases(rng),
        "po": po_cases(rng),
    }
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(
        json.dumps(data, indent=1, ensure_ascii=False) + "\n", encoding="utf-8"
    )
    counts = {key: len(data[key]) for key in ("percent", "format", "po")}
    print(f"wrote {OUT} {counts}")


if __name__ == "__main__":
    main()
