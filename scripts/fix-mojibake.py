"""Repair UTF-8 text that was mis-saved as cp1252 mojibake (e.g. … → â€¦)."""
from __future__ import annotations

import pathlib
import re

ROOT = pathlib.Path(r"C:\Users\NediM\Documents\Widgets")
MARKERS = ("â€", "Ã—", "Â°", "Â·", "Â±", "âˆ’")
REPLACEMENTS = {
    "â€¦": "…",
    "â€”": "—",
    "â€“": "–",
    "â€˜": "‘",
    "â€™": "’",
    "â€œ": "“",
    "â€": "”",
    "â€¹": "‹",
    "â€º": "›",
    "Ã—": "×",
    "Â°": "°",
    "Â·": "·",
    "Â±": "±",
    "âˆ’": "−",
}


def repair(text: str) -> str:
    try:
        return text.encode("cp1252", errors="strict").decode("utf-8")
    except UnicodeError:
        out = text
        for bad, good in REPLACEMENTS.items():
            out = out.replace(bad, good)
        return out


def main() -> None:
    fixed = 0
    for path in ROOT.rglob("*"):
        if path.suffix.lower() not in {".html", ".css", ".js", ".ts", ".json", ".md"}:
            continue
        if any(p in path.parts for p in ("node_modules", "target", ".git", "scripts")):
            continue
        try:
            text = path.read_text(encoding="utf-8-sig")
        except OSError:
            continue
        if not any(m in text for m in MARKERS):
            continue
        repaired = repair(text)
        if repaired == text:
            print(f"NOCHANGE {path}")
            continue
        path.write_text(repaired, encoding="utf-8", newline="\n")
        fixed += 1
        print(f"OK {path}")

    todo = (ROOT / "todo" / "index.html").read_text(encoding="utf-8")
    ph = re.search(r'placeholder="([^"]+)"', todo)
    print("todo placeholder:", ph.group(1) if ph else None)
    for line in todo.splitlines():
        if "textContent" in line and ("×" in line or "Ã—" in line or "â€" in line):
            print("todo x-line:", line.strip())
    print(f"fixed={fixed}")


if __name__ == "__main__":
    main()
