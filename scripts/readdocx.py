#!/usr/bin/env python3
"""Dump savecodex docx files as text, OCR-ing screenshots locally with tesseract.
usage: readdocx.py [-w MAXLINES] [-p] file.docx...   (-p: only problems)"""
import re, subprocess, sys, tempfile, zipfile, argparse
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
import xml.etree.ElementTree as ET

W = "{http://schemas.openxmlformats.org/wordprocessingml/2006/main}"
A = "{http://schemas.openxmlformats.org/drawingml/2006/main}"
R = "{http://schemas.openxmlformats.org/officeDocument/2006/relationships}"
BAD = re.compile(r"command not found|Traceback|error:|ERROR \d+|No such file|syntax error|panic|Exception in|Segmentation|cannot find symbol", re.I)

def ocr(data, tmp):
    path = Path(tmp) / f"{abs(hash(data))}.png"
    path.write_bytes(data)
    out = subprocess.run(["tesseract", str(path), "-", "--psm", "6"], capture_output=True, text=True).stdout
    return "\n".join(l.rstrip() for l in out.splitlines() if l.strip())

def blocks(z):
    rels = {r.get("Id"): r.get("Target") for r in ET.fromstring(z.read("word/_rels/document.xml.rels"))}
    for p in ET.fromstring(z.read("word/document.xml")).iter(W + "p"):
        text = "".join(t.text or "" for t in p.iter(W + "t"))
        imgs = [b.get(R + "embed") for b in p.iter(A + "blip")]
        if text.strip(): yield "t", text
        for i in imgs: yield "i", "word/" + rels[i]

def render(file, maxlines, only_bad):
    z = zipfile.ZipFile(file)
    items = list(blocks(z))
    with tempfile.TemporaryDirectory() as tmp, ThreadPoolExecutor(8) as ex:
        futs = {n: ex.submit(ocr, z.read(n), tmp) for k, n in items if k == "i"}
        out, bad, nimg, empty = [], 0, 0, 0
        for k, v in items:
            if k == "t": out.append(v[:200]); continue
            text = futs[v].result(); nimg += 1
            flag = bool(BAD.search(text)); empty += not text; bad += flag
            lines = text.splitlines()
            if len(lines) > maxlines: lines = lines[:maxlines] + [f"... (+{len(lines)-maxlines} lines)"]
            out.append(f"  [img{' !' if flag else ''}{' EMPTY' if not text else ''}]\n" + "\n".join("  | " + l for l in lines))
    head = f"=== {Path(file).name}: {nimg} img, {bad} flagged, {empty} empty"
    if only_bad: return head + ("\n" + "\n".join(o for o in out if "[img !" in o or "EMPTY" in o) if bad or empty else "")
    return head + "\n" + "\n".join(out)

if __name__ == "__main__":
    ap = argparse.ArgumentParser(); ap.add_argument("files", nargs="+"); ap.add_argument("-w", type=int, default=12); ap.add_argument("-p", action="store_true")
    a = ap.parse_args()
    for f in a.files: print(render(f, a.w, a.p), "\n")
