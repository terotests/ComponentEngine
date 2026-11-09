#!/usr/bin/env python3
# compare.py cerxes.txt hermes.txt [quickjs.txt [node.txt]] : the feature
# cases side by side, slowest CErXes/Hermes ratio first; X marks a checksum
# that differs from the first engine's.
import sys
def load(p):
    r = {}
    for l in open(p):
        x = l.split()
        if len(x) >= 3:
            try: r[x[0]] = (float(x[1]), x[2])
            except ValueError: pass
    return r
names = ["cerxes", "hermes", "quickjs", "node"][:len(sys.argv) - 1]
runs = [load(p) for p in sys.argv[1:]]
rows = []
for k, (t, h) in runs[0].items():
    o = [r.get(k) for r in runs[1:]]
    ratio = t / o[0][0] if o[0] and o[0][0] > 0 else 0
    rows.append((ratio, k, t, h, o))
rows.sort(reverse=True)
print(f"{'case':26}" + "".join(f"{n:>9}" for n in names) + "  cx/hermes" + ("  cx/qjs" if len(runs) > 2 else ""))
for ratio, k, t, h, o in rows:
    cells = f"{t:9.1f}" + "".join((f"{x[0]:8.1f}" + ("X" if x[1] != h else " ")) if x else "        -" for x in o)
    q = f"  {t / o[1][0]:6.2f}" if len(o) > 1 and o[1] and o[1][0] > 0 else ""
    print(f"{k:26}{cells}  {ratio:8.2f}{q}")
