"""Effective style properties of a docx, resolved the way Word does, and a diff between two docx files.

usage: wordcheck.py REFERENCE.docx OURS.docx [--stock-normal] [--theme major,minor,accent1hex]
--stock-normal ignores the reference's own Normal overrides (a teacher's customisation), as a blank doc has none.
--theme replaces the reference's theme fonts/accent, to compare against a Word install with another theme.
"""
import re, sys, zipfile

W = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"
STYLES = ["Normal", "Title", "Heading1", "Heading2", "Heading3"]


def attr(el, name):
    m = re.search(r'w:%s="([^"]*)"' % name, el)
    return m and m.group(1)


def tag(xml, name):
    m = re.search(r"<w:%s\b[^>]*?/?>" % name, xml)
    return m and m.group(0)


def shade(hexcol, shade_hex):
    f = int(shade_hex, 16) / 255
    return "".join("%02X" % round(int(hexcol[i:i + 2], 16) * f) for i in (0, 2, 4))


def props(rpr, ppr, theme):
    out = {}
    if rpr:
        f = tag(rpr, "rFonts")
        if f:
            if attr(f, "ascii"): out["font"] = attr(f, "ascii")
            elif attr(f, "asciiTheme"): out["font"] = theme["major" if "major" in attr(f, "asciiTheme") else "minor"]
        for k in ["sz", "kern", "spacing"]:
            t = tag(rpr, k)
            if t: out[{"spacing": "charspacing"}.get(k, k)] = attr(t, "val")
        c = tag(rpr, "color")
        if c:
            # Word stores the shaded theme colour it computed; an overridden theme brings its own
            out["color"] = theme["heading"] if attr(c, "themeColor") == "accent1" and "heading" in theme else attr(c, "val")
        for k in ["b", "i", "u"]:
            t = tag(rpr, k)
            if t: out[k] = attr(t, "val") or "on"
        if "ligatures" in rpr: out["ligatures"] = re.search(r'w14:ligatures w14:val="([^"]*)"', rpr).group(1)
    if ppr:
        s = tag(ppr, "spacing")
        if s:
            for k in ["before", "after", "line", "lineRule"]:
                if attr(s, k) is not None: out[k] = attr(s, k)
        b = re.search(r"<w:pBdr>.*?<w:bottom\b[^>]*>", ppr, re.S)
        if b:
            for k in ["val", "sz", "space", "color"]: out["border." + k] = attr(b.group(0).split("<w:bottom")[1], k)
        for k in ["keepNext", "keepLines", "contextualSpacing"]:
            if tag(ppr, k): out[k] = "on"
        for k in ["outlineLvl", "jc"]:
            t = tag(ppr, k)
            if t: out[k] = attr(t, "val")
    return out


def section(block, name):
    m = re.search(r"<w:%s>(.*?)</w:%s>" % (name, name), block or "", re.S)
    return m and m.group(1)


def resolve(path, stock_normal=False, theme_override=None, override_defaults=None):
    z = zipfile.ZipFile(path)
    x = z.read("word/styles.xml").decode()
    theme = {}
    if "word/theme/theme1.xml" in z.namelist():
        t = z.read("word/theme/theme1.xml").decode()
        theme["major"] = re.search(r'<a:majorFont><a:latin typeface="([^"]*)"', t).group(1)
        theme["minor"] = re.search(r'<a:minorFont><a:latin typeface="([^"]*)"', t).group(1)
        theme["accent1"] = re.search(r'<a:accent1><a:srgbClr val="([^"]*)"', t).group(1)
    if theme_override:
        theme.update(zip(["major", "minor", "heading"], theme_override.split(",")))
    defaults = re.search(r"<w:docDefaults>(.*?)</w:docDefaults>", x, re.S).group(1)
    base = props(section(defaults, "rPrDefault"), section(defaults, "pPrDefault"), theme)
    if override_defaults: base.update(zip(["sz", "line", "font"], override_defaults.split(",")))
    styles = {}
    for m in re.finditer(r'<w:style [^>]*w:type="paragraph"[^>]*w:styleId="([^"]*)"[^>]*>(.*?)</w:style>', x, re.S):
        sid, body = m.groups()
        parent = re.search(r'<w:basedOn w:val="([^"]*)"', body)
        styles[sid] = (parent and parent.group(1), props(section(body, "rPr"), section(body, "pPr"), theme))

    def eff(sid):
        if sid not in styles: return dict(base)
        parent, own = styles[sid]
        if sid == "Normal" and stock_normal: own = {}
        out = eff(parent) if parent else dict(base)
        out.update(own)
        return out

    doc = z.read("word/document.xml").decode()
    page = {}
    for el in ["pgSz", "pgMar"]:
        t = tag(doc, el)
        for k in ["w", "h", "top", "right", "bottom", "left", "header", "footer"]:
            if t and attr(t, k) is not None: page[f"{el}.{k}"] = attr(t, k)
    st = z.read("word/settings.xml").decode()
    tab = tag(st, "defaultTabStop")
    compat = re.search(r"<w:compat>(.*?)</w:compat>", st, re.S)
    settings = {"defaultTabStop": tab and attr(tab, "val")}
    for m in re.finditer(r'<w:(\w+)\s*/>|compatSetting w:name="(\w+)"[^>]*w:val="(\w+)"', compat.group(1) if compat else ""):
        settings[m.group(1) or m.group(2)] = m.group(3) or "on"
    out = {sid: eff(sid) for sid in STYLES} | {"page": page, "settings": settings}
    for sid in STYLES:
        p = out[sid]
        # what renders: kerned or not at this size, and no spacing before == 0
        kern = p.pop("kern", "0")
        p["kerned"] = "yes" if int(kern) and int(p.get("sz", 20)) >= int(kern) else "no"
        if p.get("before") == "0": p.pop("before")
    return out


if __name__ == "__main__":
    args = sys.argv[1:]
    stock = "--stock-normal" in args
    override = args[args.index("--theme") + 1] if "--theme" in args else None
    ref, ours = [a for a in args if a.endswith(".docx")]
    stock_defaults = args[args.index("--defaults") + 1] if "--defaults" in args else None
    a, b = resolve(ref, stock, override, stock_defaults), resolve(ours)
    diffs = 0
    for sid in STYLES + ["page", "settings"]:
        keys = sorted(set(a[sid]) | set(b[sid]))
        for k in keys:
            if a[sid].get(k) != b[sid].get(k):
                diffs += 1
                print(f"DIFF {sid:9} {k:18} word={a[sid].get(k)!s:14} ours={b[sid].get(k)}")
    total = sum(len(set(a[s]) | set(b[s])) for s in STYLES + ["page", "settings"])
    print(f"{total - diffs}/{total} properties match")
