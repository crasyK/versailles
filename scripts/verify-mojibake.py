from pathlib import Path
import re

todo = Path(r"C:\Users\NediM\Documents\Widgets\todo\index.html").read_text(encoding="utf-8")
ph = re.search(r'placeholder="([^"]+)"', todo)
print("placeholder repr:", repr(ph.group(1) if ph else None))
print("placeholder ords:", [hex(ord(c)) for c in (ph.group(1) if ph else "")])
for line in todo.splitlines():
    if "x.textContent" in line:
        print("x-line repr:", repr(line.strip()))
        s = line.split("=", 1)[1].strip().rstrip(";").strip().strip("'\"")
        print("x-line ords:", [hex(ord(c)) for c in s])

bad = []
for p in Path(r"C:\Users\NediM\Documents\Widgets").rglob("*"):
    if p.suffix.lower() not in {".html", ".css", ".js", ".ts"}:
        continue
    if any(x in p.parts for x in ("node_modules", "target", ".git")):
        continue
    try:
        t = p.read_text(encoding="utf-8")
    except OSError:
        continue
    if "â€" in t or "Ã—" in t or "Â°" in t or "Â·" in t:
        bad.append(str(p))
print("still_bad_count:", len(bad))
for b in bad[:10]:
    print(" still:", b)
