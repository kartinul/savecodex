import { useEffect, useState } from "react";
import { shotUrl, type Mode, type Settings, type State } from "./api";
import { Ticker, type Step } from "./Ticker";

export type Tile = { id: string; title: string; state: State; note: string; code: string; written: boolean; output: string; shots: number[]; since: number };
export type Doc = { id: string; title: string; status: string; error: boolean; tiles: Tile[] | null; since: number; file?: { n: number; name: string } };

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

const Ghost = ({ since }: { since: number }) => <div className="tile ghost"><div className="tile-title"><Ticker step="reading" since={since} /></div><div className="tile-body" /><div className="tile-note" /></div>;

export function Groups({ docs, job, mode, onDownload }: { docs: Doc[]; job: string; mode: Mode; onDownload: (d: Doc) => void }) {
  const [open, setOpen] = useState<{ doc: string; id: string } | null>(null);
  const shown = open && docs.find(d => d.id === open.doc)?.tiles?.find(t => t.id === open.id);
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
            </header>
            <div className="tiles">
              {d.tiles ? d.tiles.map((t, i) => <TileView key={t.id} tile={t} n={i + 1} job={job} mode={mode} onOpen={() => setOpen({ doc: d.id, id: t.id })} />) : !d.error && <Ghost since={d.since} />}
            </div>
          </section>
        );
      })}
      {shown && (
        <div className="modal" onClick={e => e.target === e.currentTarget && setOpen(null)}>
          <div className="modal-card" role="dialog" aria-modal="true">
            <button className="btn small close" onClick={() => setOpen(null)}>close</button>
            <h2 className="title">{mode === "pack" ? shown.title : `q${(docs.find(d => d.id === open!.doc)?.tiles?.indexOf(shown) ?? 0) + 1}`}</h2>
            {mode === "solve" && <p>{shown.title}</p>}
            {shown.note && <p className="muted">{shown.note}</p>}
            {shown.code && <><h3>{shown.written ? "answer" : "code"}</h3>{shown.written ? <p>{shown.code}</p> : <pre>{shown.code}</pre>}</>}
            {shown.shots.length > 0 && <><h3>output</h3>{shown.shots.map(n => <img key={n} className="shot" src={shotUrl(job, n)} alt="Terminal screenshot" />)}</>}
            {!shown.shots.length && shown.output && <><h3>output</h3><pre>{shown.output}</pre></>}
          </div>
        </div>
      )}
    </>
  );
}

export function Idle({ mode, preview }: { mode: Mode; preview: string }) {
  const steps = mode === "solve"
    ? [["read ", "finds every question in your files"], ["write", "writes an answer for each one"], ["run  ", "runs it and takes a real terminal screenshot"], ["pack ", "puts code and screenshots into a word document"]]
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

/** How the document will come out, drawn from the settings, so changes show up while customizing. */
export function DocPreview({ mode, s, name, preview }: { mode: Mode; s: Settings; name: string; preview: string }) {
  const fill = (t: string) => t.split("{}").join(name);
  const heading = (i: number) => mode === "solve" ? `Question ${i}` : `${i}. ${i === 1 ? "fact" : "fib"}`;
  const first = <><h2>{heading(1)}</h2><pre>{SAMPLE}</pre><img src={preview} alt="" /></>;
  const second = <><h2>{heading(2)}</h2>{mode === "solve"
    ? <p>An array keeps its elements next to each other in memory, so any element can be read straight from its index.</p>
    : <pre>{SAMPLE.split("\n").slice(0, 4).join("\n")}</pre>}</>;
  return (
    <section className="pages" aria-label="Document preview">
      <p># how {fill(s.output || (mode === "solve" ? "{}_solved" : "{}_pack"))}.docx will look</p>
      <article className="page"><div className="sheet">
        <h1>{fill(s.doc_title) || name}</h1>
        {s.doc_text && <pre>{fill(s.doc_text)}</pre>}
        <div className="gap" />
        {first}
        {!s.page_break && <><div className="gap" />{second}</>}
      </div></article>
      {s.page_break && <article className="page"><div className="sheet">{second}</div></article>}
    </section>
  );
}
