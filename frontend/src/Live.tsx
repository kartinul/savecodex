import { useEffect, useState } from "react";
import { partLabel, shotUrl, type DocTheme, type Look, type Mode, type Settings, type State } from "./api";
import { Ticker, type Step } from "./Ticker";

export type Tile = { id: string; title: string; state: State; note: string; code: string; written: boolean; output: string; shots: number[]; since: number };
export type Doc = { id: string; job: string; title: string; status: string; error: boolean; tiles: Tile[] | null; since: number; file?: { n: number; name: string } };

const LABEL: Record<State, string> = { waiting: "waiting", writing: "writing", running: "running", fixing: "fixing", ok: "done", failed: "failed" };
const reduced = matchMedia("(prefers-reduced-motion: reduce)").matches;
const SAMPLE = `#include <stdio.h>

int main() {
    int n, f = 1;
    printf("Enter a number: ");
    scanf("%d", &n);
    for (int i = 2; i <= n; i++) f *= i;
    printf("Factorial of %d is %d\\n", n, f);
}`;

const FIB = `def fib(n):
    a, b = 0, 1
    for _ in range(n):
        a, b = b, a + b
    return a

print(fib(10))`;

/** A small ER diagram like the ones `dot` draws for diagram answers. */
const ER = (
  <svg viewBox="0 0 420 150" style={{ width: "70%", display: "inline-block", verticalAlign: "bottom" }} fontFamily="'Times New Roman', Tinos, serif" fontSize="13" textAnchor="middle" stroke="#000" fill="none" aria-label="Sample ER diagram">
    <rect x="20" y="62" width="90" height="34" /><text x="65" y="84" stroke="none" fill="#000">Student</text>
    <polygon points="210,52 262,79 210,106 158,79" /><text x="210" y="83" stroke="none" fill="#000">enrolls</text>
    <rect x="310" y="62" width="90" height="34" /><text x="355" y="84" stroke="none" fill="#000">Course</text>
    <path d="M110 79H158M262 79H310M45 62L35 34M85 62L95 34M335 62L325 34M375 62L385 34" />
    <text x="133" y="73" stroke="none" fill="#000" fontSize="11">M</text><text x="287" y="73" stroke="none" fill="#000" fontSize="11">N</text>
    <ellipse cx="32" cy="22" rx="28" ry="12" /><text x="32" y="26" stroke="none" fill="#000" fontSize="11" textDecoration="underline">roll</text>
    <ellipse cx="98" cy="22" rx="28" ry="12" /><text x="98" y="26" stroke="none" fill="#000" fontSize="11">name</text>
    <ellipse cx="322" cy="22" rx="28" ry="12" /><text x="322" y="26" stroke="none" fill="#000" fontSize="11" textDecoration="underline">code</text>
    <ellipse cx="388" cy="22" rx="28" ry="12" /><text x="388" y="26" stroke="none" fill="#000" fontSize="11">title</text>
  </svg>
);

/** Reveals new code a few characters at a time, since answers arrive whole. */
function useTyped(text: string) {
  const [n, setN] = useState(0);
  useEffect(() => {
    if (reduced) return setN(text.length);
    let i = 0;
    const step = Math.max(4, Math.ceil(text.length / 70));
    const timer = setInterval(() => {
      i = Math.min(i + step, text.length);
      setN(i);
      if (i >= text.length) clearInterval(timer);
    }, 16);
    return () => clearInterval(timer);
  }, [text]);
  return text.slice(0, n);
}

/** What an active tile is doing, for its status line. */
function stepOf(t: Tile, mode: Mode): Step {
  if (t.state === "fixing") return "fixing";
  if (t.state === "writing") return mode === "pack" ? "input" : "writing";
  if (/^\s*(di)?graph\b/.test(t.code)) return "drawing";
  if (/\b(create table|select .+ from|insert into)\b/i.test(t.code)) return "session";
  return "running";
}

function TileView({ tile, n, job, mode, onOpen }: { tile: Tile; n: number; job: string; mode: Mode; onOpen: () => void }) {
  const typed = useTyped(tile.code);
  const done = tile.state === "ok" || tile.state === "failed";
  let body = null;
  if (done && tile.shots.length) body = <img className="shot pop" src={shotUrl(job, tile.shots[0])} alt={`Screenshot for q${n}`} />;
  else if (tile.written && tile.code) body = <p className="tile-text">{done ? tile.code : typed}</p>;
  else if (done && tile.output) body = <pre className="tile-out">{tile.output.split("\n").slice(-7).join("\n")}</pre>;
  else if (tile.code) body = <pre className="tile-code">{typed.split("\n").slice(-7).join("\n")}</pre>;
  return (
    <button className="tile" data-state={tile.state} style={{ animationDelay: `${(n - 1) * 40}ms` }} onClick={onOpen}>
      <span className="tile-top"><span>q{n}</span><span className="state">[{LABEL[tile.state]}]</span></span>
      <span className="tile-title">{tile.title}</span>
      <span className="tile-body">{body}</span>
      <span className="tile-note">{tile.state === "failed" ? tile.note : !done && tile.state !== "waiting" && <Ticker step={stepOf(tile, mode)} since={tile.since} />}</span>
    </button>
  );
}

const Ghost = ({ since }: { since: number }) => <div className="tile ghost"><div className="tile-title">finding the tasks</div><div className="tile-body"><Ticker step="reading" since={since} /></div><div className="tile-note" /></div>;

export function Groups({ docs, mode, onDownload, onRedo }: { docs: Doc[]; mode: Mode; onDownload: (d: Doc) => void; onRedo?: (d: Doc) => void }) {
  const [open, setOpen] = useState<{ doc: string; id: string } | null>(null);
  const openDoc = open && docs.find(d => d.id === open.doc);
  const shown = open && openDoc?.tiles?.find(t => t.id === open.id);
  useEffect(() => {
    const esc = (e: KeyboardEvent) => e.key === "Escape" && setOpen(null);
    addEventListener("keydown", esc);
    return () => removeEventListener("keydown", esc);
  }, []);
  return (
    <>
      {docs.map(d => {
        const done = d.tiles?.filter(t => t.state === "ok").length ?? 0;
        return (
          <section className="grp" key={d.id}>
            <header className="grp-head">
              <div className="grp-name"><h2 className="title">{d.title}</h2><span className={`status${d.error ? " err" : ""}`}>{d.status}</span></div>
              {d.tiles && <span className="count">{done} of {d.tiles.length} done</span>}
              {d.file && <button className="link" onClick={() => onDownload(d)}>download</button>}
              {onRedo && (d.file || d.error) && <button className="link" title="solve this one again" aria-label={`Solve ${d.title} again`} onClick={() => onRedo(d)}>↻</button>}
            </header>
            <div className="tiles">
              {d.tiles ? d.tiles.map((t, i) => <TileView key={t.id} tile={t} n={i + 1} job={d.job} mode={mode} onOpen={() => setOpen({ doc: d.id, id: t.id })} />) : !d.error && <Ghost since={d.since} />}
            </div>
          </section>
        );
      })}
      {shown && (
        <div className="modal" onClick={e => e.target === e.currentTarget && setOpen(null)}>
          <div className="modal-card" role="dialog" aria-modal="true">
            <button className="btn small close" onClick={() => setOpen(null)}>close</button>
            <h2 className="title">{mode === "pack" ? shown.title : `q${(openDoc?.tiles?.indexOf(shown) ?? 0) + 1}`}</h2>
            {mode === "solve" && <p>{shown.title}</p>}
            {shown.note && <p className="muted">{shown.note}</p>}
            {shown.code && <><h3>{shown.written ? "answer" : "code"}</h3>{shown.written ? <p>{shown.code}</p> : <pre>{shown.code}</pre>}</>}
            {shown.shots.length > 0 && <><h3>output</h3>{shown.shots.map(n => <img key={n} className="shot" src={shotUrl(openDoc!.job, n)} alt="Terminal screenshot" />)}</>}
            {!shown.shots.length && shown.output && <><h3>output</h3><pre>{shown.output}</pre></>}
          </div>
        </div>
      )}
    </>
  );
}

export function Idle({ mode, preview }: { mode: Mode; preview: string }) {
  const steps = mode === "solve"
    ? [["read ", "finds every task in your files"], ["write", "writes an answer for each one"], ["run  ", "runs it and takes a real terminal screenshot"], ["pack ", "puts code and screenshots into a word document"]]
    : [["input", "makes up input for programs that ask for it"], ["run  ", "runs every file and takes a real terminal screenshot"], ["pack ", "puts code and screenshots into a word document"]];
  return (
    <section className="idle">
      <div className="steps">
        <div># what happens when you press {mode}</div>
        {steps.map(([k, v], i) => <div key={k}><b>{i + 1} {k}</b> {v}</div>)}
      </div>
      <figure className="sample"><img className="shot" src={preview} alt="Sample terminal screenshot" /><figcaption># your screenshots will look like this</figcaption></figure>
    </section>
  );
}

const PT = 100 / 595; // cqw per point: an A4 page is 595pt wide
const FAMILY: Record<string, string> = {
  Aptos: "Aptos, 'Segoe UI', 'Helvetica Neue', Arial, sans-serif",
  "Aptos Display": "'Aptos Display', Aptos, 'Segoe UI', 'Helvetica Neue', Arial, sans-serif",
  "Calibri Light": "'Calibri Light', Carlito, Calibri, 'Helvetica Neue', sans-serif",
  Calibri: "Calibri, Carlito, 'Helvetica Neue', Arial, sans-serif",
  Cambria: "Cambria, Caladea, Georgia, serif",
  "Times New Roman": "'Times New Roman', Tinos, Times, serif",
  Arial: "Arial, Arimo, Helvetica, sans-serif",
  Georgia: "Georgia, Gelasio, serif",
  Consolas: "Consolas, Inconsolata, Menlo, monospace",
  "Courier New": "'Courier New', Cousine, Courier, monospace",
};
const family = (font: string) => FAMILY[font] ?? font;

const looks = (l: Look): React.CSSProperties => ({
  fontFamily: l.font ? family(l.font) : undefined,
  fontSize: l.size ? `${l.size * PT}cqw` : undefined,
  color: l.color ? `#${l.color}` : undefined,
  fontWeight: l.bold ? 700 : 400,
  fontStyle: l.italic ? "italic" : undefined,
  textDecoration: l.underline ? "underline" : undefined,
  textAlign: l.center ? "center" : undefined,
  borderBottom: l.rule ? `${PT}cqw solid ${l.color ? `#${l.color}` : "currentColor"}` : undefined,
  marginTop: `${l.before * PT}cqw`,
  marginBottom: l.after == null ? undefined : `${l.after * PT}cqw`,
  lineHeight: l.line == null ? undefined : 1.15 * l.line,
  letterSpacing: l.spacing ? `${l.spacing * PT}cqw` : undefined,
});

/** How the document will come out, drawn from the settings and doc theme, so changes show up while customizing. */
export function DocPreview({ mode, s, t, name, preview }: { mode: Mode; s: Settings; t: DocTheme; name: string; preview: string }) {
  const fill = (text: string) => text.split("{}").join(name);
  const cm = (v: number) => `${v / 21 * 100}cqw`;
  const sheet: React.CSSProperties = {
    padding: `${cm(t.margin_top)} ${cm(t.margin)}`, fontFamily: family(t.font), fontSize: `${t.size * PT}cqw`,
    lineHeight: 1.15 * t.line, gap: `${t.gap * PT}cqw`,
  };
  const page: React.CSSProperties = { aspectRatio: t.paper === "letter" ? "1 / 1.294" : "1 / 1.414" };
  const h = (level: 1 | 2 | 3, text: string) => {
    const H = (["h2", "h3", "h4"] as const)[level - 1];
    return <H style={looks(t.headings[level - 1])}>{text}</H>;
  };
  const part = (parts: number[]) => partLabel(s.numbering, s.full_labels, "1", parts);
  const box = t.code_box !== "none";
  const code = (text: string) => <>
    {t.labels && <p style={{ fontWeight: 700 }}>Code:</p>}
    <pre style={{ fontFamily: family(t.code_font || t.font), fontSize: `${t.code_size * PT}cqw`, background: t.code_box === "shade" ? "#F2F2F2" : undefined, border: t.code_box === "border" ? "1px solid #808080" : undefined, padding: box ? "1cqw" : undefined }}>{text}</pre>
  </>;
  // like the docx: a line break above each screenshot, and empty paragraphs between items
  const shot = (sample = "") => <>
    {t.labels && <p style={{ fontWeight: 700 }}>Output:</p>}
    <p style={{ textAlign: t.shots_center ? "center" : undefined }}><br /><img src={sample ? `${preview}&sample=${sample}` : preview} alt="" style={{ display: "inline-block", verticalAlign: "bottom", width: { small: "50%", medium: "75%", full: "100%" }[t.shots] }} /></p>
  </>;
  const blank = <p>{"\u00a0"}</p>;
  const first = mode === "solve" ? <>
    {h(1, `${t.prefix}1`)}
    {h(2, part([1]))}
    {t.show_question && <p style={looks(t.question)}>Write a program that reads a number and prints its factorial.</p>}
    {code(SAMPLE)}
    {shot()}
    {blank}
    {h(2, part([2]))}
    {h(3, part([2, 1]))}
    <p>An array keeps its elements next to each other in memory, so any element can be read straight from its index. Inserting in the middle means moving every element after it.</p>
    {blank}
    {h(3, part([2, 2]))}
    <p>A linked list stores each element with a pointer to the next one, so inserting only changes two pointers, but reaching the n-th element means walking n steps.</p>
  </> : <>{h(1, "1. fact")}{code(SAMPLE)}{shot()}</>;
  const second = mode === "solve" ? <>
    {h(1, `${t.prefix}2`)}
    {h(2, part([1]))}
    {code(FIB)}
    {shot("fib")}
    {blank}
    {h(2, part([2]))}
    <p style={{ textAlign: t.shots_center ? "center" : undefined }}><br />{ER}</p>
  </> : <>{h(1, "2. fib")}{code(FIB)}{shot("fib")}{blank}{h(1, "3. hello")}{code(`print("hello, world")`)}{shot("hello")}</>;
  const number = (n: number) => t.page_numbers && <span className="page-num" style={{ bottom: cm(1.2) }}>{n}</span>;
  return (
    <section className="pages" aria-label="Document preview">
      <p># how {fill(s.output || (mode === "solve" ? "{}_solved" : "{}_pack"))}.docx will look</p>
      <article className="page" style={page}><div className="sheet" style={sheet}>
        <h1 style={looks(t.title)}>{fill(s.doc_title) || name}</h1>
        {s.doc_text && <><pre>{fill(s.doc_text)}</pre>{blank}</>}
        {first}
        {!s.page_break && <>{blank}{second}</>}
      </div>{number(1)}</article>
      {s.page_break && <article className="page" style={page}><div className="sheet" style={sheet}>{second}</div>{number(2)}</article>}
    </section>
  );
}
