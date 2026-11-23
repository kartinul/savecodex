import { useEffect, useRef, useState } from "react";
import { renderAsync } from "docx-preview";
import { EMPTY_DRAFT, docUrl, followStep, reportStart, reportStep, reportUpload, type Block, type DocTheme, type Draft, type ReportFields, type Upload } from "./api";
import { Drop, Seg, Switch, Text, Tip } from "./Settings";

const TIP = {
  format: "the instruction pdf your teacher gave; its sections and look are followed. without one, a usual jiit layout is used",
  idea: "what the project is and does, in a few lines; the more real detail, the less the ai guesses",
  course: "left empty, it's read from the format pdf",
  logo: "the picture on top of the cover; jiit unless you pick another",
  role: "what this member did, for the team roles table; optional",
  theme: "how the word file looks; word is plain word 365. the format pdf's font and spacing go on top",
  toc: "a contents page after the cover",
  code: "code-based reports run and show the code; others are written, with charts from your data",
};
const HONORIFICS = ["Dr.", "Prof.", "Mr.", "Ms.", "Mrs."];
const STATE: Record<string, string> = { waiting: "·", writing: "…", done: "✓", failed: "✗" };
const KEY = "savecodex.report";

type Busy = "" | "planning" | "writing" | "redoing" | "saving";
type Saved = { draft: Draft; job: string; theme: string };
type Ai = { api_key: string; base_url: string; model: string };

function load(): Saved {
  const empty = { draft: EMPTY_DRAFT, job: "", theme: "word" };
  try { return { ...empty, ...JSON.parse(localStorage.getItem(KEY) || "{}") } } catch { return empty }
}

/** A section's text as the user edits it: blank-line separated paragraphs, `## ` sub-headings,
 *  `- ` and `1. ` lists, and `[[n: …]]` for tables and figures, which keep their place. */
const toText = (blocks: Block[]) => blocks.map((b, i) => {
  if (b.type === "para") return b.text;
  if (b.type === "heading") return `## ${b.text}`;
  if (b.type === "bullets") return b.items.map(x => `- ${x}`).join("\n");
  if (b.type === "numbered") return b.items.map((x, k) => `${k + 1}. ${x}`).join("\n");
  return `[[${i + 1}: ${b.type} "${(b as { caption: string }).caption}"]]`;
}).join("\n\n");

function fromText(text: string, old: Block[]): Block[] {
  return text.split(/\n\s*\n/).map(c => c.trim()).filter(Boolean).map((chunk): Block | undefined => {
    const kept = chunk.match(/^\[\[(\d+):/);
    if (kept) return old[+kept[1] - 1];
    if (chunk.startsWith("## ")) return { type: "heading", text: chunk.slice(3) };
    const lines = chunk.split("\n");
    if (lines.every(l => l.startsWith("- "))) return { type: "bullets", items: lines.map(l => l.slice(2)) };
    if (lines.every(l => /^\d+\.\s/.test(l))) return { type: "numbered", items: lines.map(l => l.replace(/^\d+\.\s/, "")) };
    return { type: "para", text: chunk };
  }).filter((b): b is Block => !!b);
}

export function useReport(themes: DocTheme[], ai: Ai) {
  const [saved, setSaved] = useState(load);
  const [files, setFiles] = useState<Upload[]>([]);
  const [logo, setLogo] = useState<File | null>(null);
  const [busy, setBusy] = useState<Busy>("");
  const [status, setStatus] = useState("");
  const [doc, setDoc] = useState<{ job: string; n: number } | null>(null);
  const unfollow = useRef<() => void>(() => {});
  const { draft, job } = saved;
  const store = (patch: Partial<Saved>) => setSaved(s => {
    const next = { ...s, ...patch };
    try { localStorage.setItem(KEY, JSON.stringify(next)) } catch { /* private mode */ }
    return next;
  });
  const setDraft = (patch: Partial<Draft>) => store({ draft: { ...saved.draft, ...patch } });
  const theme = themes.find(t => t.name === saved.theme);
  const fields: ReportFields = { ...ai, doc_theme: theme ? JSON.stringify(theme) : saved.theme };

  const follow = (id: string, from: number, what: Busy) => {
    unfollow.current();
    setBusy(what);
    setStatus("");
    unfollow.current = followStep(id, from, e => {
      if (e.t === "draft") store({ draft: e.draft, job: id });
      if (e.t === "done") setDoc({ job: id, n: e.file });
      if (e.t === "error") setStatus(e.message);
      if (e.t === "finished") setBusy("");
    });
  };

  /** Runs a step in the job, making a new job from the files when the old one expired (an hour). */
  const step = async (what: Busy, name: "write" | "redo" | "build", d: Draft, extra?: Record<string, string>) => {
    store({ draft: d });
    setBusy(what);
    try {
      let id = job;
      if (!id) id = (await reportStart(files, logo, d, fields, "build")).id;
      const started = await reportStep(id, name, d, fields, extra).catch(async e => {
        if ((e as { status?: number }).status !== 404) throw e;
        id = (await reportStart(files, logo, d, fields, "build")).id;
        return reportStep(id, name, d, fields, extra);
      });
      store({ draft: d, job: id });
      follow(id, started.from, what);
    } catch (e) {
      setBusy("");
      setStatus(`couldn't start: ${(e as Error).message}`);
    }
  };

  const plan = async () => {
    const d = { ...draft, sections: [], questions: [], runs: [] };
    store({ draft: d });
    setDoc(null);
    setBusy("planning");
    try {
      const started = await reportStart(files, logo, d, fields);
      store({ draft: d, job: started.id });
      follow(started.id, started.from, "planning");
    } catch (e) {
      setBusy("");
      setStatus(`couldn't start: ${(e as Error).message}`);
    }
  };

  const add = async (list: FileList | File[]) => {
    const added = [...list].map(file => ({ file, path: file.webkitRelativePath || file.name }));
    setFiles(f => [...f, ...added]);
    if (job && draft.sections.length) await reportUpload(job, added).catch(() => {});
  };

  // a draft from an earlier visit gets its preview back (no ai, just the docx)
  useEffect(() => {
    if (job && draft.sections.length) reportStep(job, "build", draft, fields).then(s => follow(job, s.from, "")).catch(() => store({ job: "" }));
    return () => unfollow.current();
  }, []);

  return {
    draft, files, logo, busy, status, doc, theme: saved.theme, themes,
    setDraft, setLogo, add, plan,
    setTheme: (name: string) => store({ theme: name }),
    removeFile: (i: number) => setFiles(f => f.filter((_, k) => k !== i)),
    write: (assume: boolean) => step("writing", "write", assume ? { ...draft, questions: draft.questions.map(q => ({ ...q, answer: "" })) } : draft),
    redo: (i: number, note: string) => step("redoing", "redo", draft, { section: String(i), note }),
    edit: (i: number, text: string) => step("saving", "build", { ...draft, sections: draft.sections.map((s, k) => k === i ? { ...s, blocks: fromText(text, s.blocks) } : s) }),
    stop: () => { unfollow.current(); setBusy("") },
    fresh: () => { unfollow.current(); setBusy(""); setDoc(null); store({ job: "", draft: { ...EMPTY_DRAFT, kind: draft.kind, cover: { ...draft.cover, title: "" } } }) },
  };
}

type R = ReturnType<typeof useReport>;

export function ReportMain({ r }: { r: R }) {
  const view = useRef<HTMLDivElement>(null);
  const [zoom, setZoom] = useState(1);
  useEffect(() => {
    if (!r.doc || !view.current) return;
    let current = true;
    const el = view.current;
    fetch(docUrl(r.doc.job, r.doc.n)).then(res => res.blob()).then(blob => {
      if (!current) return;
      const next = document.createElement("div");
      return renderAsync(blob, next, undefined, { inWrapper: true, breakPages: true, ignoreLastRenderedPageBreak: true, renderFooters: true, className: "docx" })
        .then(() => { if (current) el.replaceChildren(next) });
    }).catch(() => {});
    return () => { current = false };
  }, [r.doc]);
  useEffect(() => {
    const el = view.current;
    if (!el) return;
    const fit = new ResizeObserver(() => setZoom(Math.min(1, el.clientWidth / 820)));
    fit.observe(el);
    return () => fit.disconnect();
  }, []);
  const done = r.draft.sections.filter(s => s.state === "done").length;
  const say = { planning: "reading your files and planning the outline", writing: `writing: ${done} of ${r.draft.sections.length} sections done`, redoing: "writing that section again", saving: "saving your edit", "": "" }[r.busy];
  return <>
    {!r.doc && <div className="idle">
      <div className="steps">
        <b>{r.draft.kind === "synopsis" ? "a synopsis" : "a report"} in four steps</b>
        <span>1. drop the format pdf, your code and data</span>
        <span>2. fill the cover once, it's remembered</span>
        <span>3. plan: answer a few questions, fix the outline</span>
        <span>4. write: watch it fill in, redo any section</span>
      </div>
    </div>}
    {(say || r.status) && <p className={`status${r.status ? " err" : ""}`}>{r.status || say}</p>}
    <div className="docx-view" ref={view} style={{ zoom }} hidden={!r.doc} />
  </>;
}

export function ReportSide({ r }: { r: R }) {
  const { draft } = r;
  const cover = draft.cover;
  const input = useRef<HTMLInputElement>(null), folder = useRef<HTMLInputElement>(null), logoInput = useRef<HTMLInputElement>(null);
  const [over, setOver] = useState(false);
  const [noting, setNoting] = useState<{ i: number; note: string } | null>(null);
  const [editing, setEditing] = useState<{ i: number; text: string } | null>(null);
  const setCover = (patch: Partial<typeof cover>) => r.setDraft({ cover: { ...cover, ...patch } });
  const list = <T,>(items: T[], i: number, patch: Partial<T>) => items.map((x, k) => k === i ? { ...x, ...patch } : x);
  const sections = draft.sections;
  const planned = sections.length > 0;
  const written = sections.some(s => s.state === "done" || s.state === "failed");
  const noun = draft.kind;

  return <>
    <h1 className="title">{noun === "synopsis" ? "write a synopsis" : "write a report"}</h1>
    {!planned && !r.busy && <>
      <Seg value={draft.kind} options={[["report", "report"], ["synopsis", "synopsis"]]} onChange={kind => r.setDraft({ kind })} />
      <div className={`tray${over ? " over" : ""}`}
        onDragOver={e => { e.preventDefault(); setOver(true) }} onDragLeave={() => setOver(false)}
        onDrop={e => { e.preventDefault(); setOver(false); r.add(e.dataTransfer.files) }}>
        <div className="tray-copy"><strong>drop files here<Tip t={TIP.format} /></strong><span>format pdf, code, screenshots, csv</span></div>
        <button className="btn small" onClick={() => input.current?.click()}>files</button>
        <button className="btn small" onClick={() => folder.current?.click()}>folder</button>
      </div>
    </>}
    <input ref={input} type="file" multiple hidden onChange={e => { r.add(e.target.files!); e.target.value = "" }} />
    <input ref={folder} type="file" hidden {...{ webkitdirectory: "" }} onChange={e => { r.add(e.target.files!); e.target.value = "" }} />
    <input ref={logoInput} type="file" accept="image/png,image/jpeg" hidden onChange={e => { r.setLogo(e.target.files?.[0] ?? null); e.target.value = "" }} />
    {r.files.length > 0 && !planned && <ul className="tree">
      {r.files.map((u, i) => <li key={u.path + i}><span className="name">{u.path}</span><button className="x" aria-label={`Remove ${u.path}`} onClick={() => r.removeFile(i)}>×</button></li>)}
    </ul>}

    {!planned && !r.busy && <>
      <div className="rows">
        <Text label="title" tip="the project's name on the cover" value={cover.title} ph="pokemon battle simulator" onChange={title => setCover({ title })} />
        <label className="row stack"><span>idea<Tip t={TIP.idea} /></span>
          <textarea className="ri" rows={3} value={draft.idea} placeholder="a java console game where…" onChange={e => r.setDraft({ idea: e.target.value })} />
        </label>
      </div>
      <h2 className="sec">cover</h2>
      <div className="rows">
        <Text label="course" tip={TIP.course} value={cover.course} ph="from the pdf" onChange={course => setCover({ course })} />
        <Text label="code" tip={TIP.course} value={cover.code} ph="from the pdf" onChange={code => setCover({ code })} />
        <Text label="semester" tip={TIP.course} value={cover.semester} ph="from the pdf" onChange={semester => setCover({ semester })} />
        <div className="row"><span>logo<Tip t={TIP.logo} /></span>
          <span className="pick">
            <span className="muted">{r.logo ? r.logo.name : "jiit"}</span>
            {r.logo && <button className="x" style={{ opacity: 1 }} aria-label="Use the jiit logo" onClick={() => r.setLogo(null)}>×</button>}
            <button className="btn small" onClick={() => logoInput.current?.click()}>pick</button>
          </span>
        </div>
      </div>
      <h2 className="sec">submitted by</h2>
      <div className="rows">
        {cover.members.map((m, i) => <div className="row stack" key={i}>
          <div className="trio">
            <input className="ri" value={m.name} placeholder="name" aria-label="member name" onChange={e => setCover({ members: list(cover.members, i, { name: e.target.value }) })} />
            <input className="ri" value={m.enrollment} placeholder="enrollment" aria-label="enrollment number" onChange={e => setCover({ members: list(cover.members, i, { enrollment: e.target.value }) })} />
            <button className="x" style={{ opacity: 1 }} aria-label="Remove member" onClick={() => setCover({ members: cover.members.filter((_, k) => k !== i) })}>×</button>
          </div>
          <input className="ri" value={m.role} placeholder="did what (optional)" aria-label="what they did" title={TIP.role} onChange={e => setCover({ members: list(cover.members, i, { role: e.target.value }) })} />
        </div>)}
        <div className="row"><button className="more-opts" onClick={() => setCover({ members: [...cover.members, { name: "", enrollment: "", role: "" }] })}>[+] member</button></div>
      </div>
      <h2 className="sec">submitted to</h2>
      <div className="rows">
        {cover.supervisors.map((s, i) => <div className="row stack" key={i}>
          <div className="trio">
            <span className="pick" style={{ justifyContent: "flex-start" }}>
              <Drop label="honorific" value={s.honorific} options={HONORIFICS} onChange={honorific => setCover({ supervisors: list(cover.supervisors, i, { honorific }) })} />
              <input className="ri" value={s.name} placeholder="name" aria-label="supervisor name" onChange={e => setCover({ supervisors: list(cover.supervisors, i, { name: e.target.value }) })} />
            </span>
            <input className="ri" value={s.designation} placeholder="designation (optional)" aria-label="designation" onChange={e => setCover({ supervisors: list(cover.supervisors, i, { designation: e.target.value }) })} />
            <button className="x" style={{ opacity: 1 }} aria-label="Remove supervisor" onClick={() => setCover({ supervisors: cover.supervisors.filter((_, k) => k !== i) })}>×</button>
          </div>
        </div>)}
        <div className="row"><button className="more-opts" onClick={() => setCover({ supervisors: [...cover.supervisors, { honorific: "Dr.", name: "", designation: "" }] })}>[+] supervisor</button></div>
      </div>
      <div className="rows">
        <div className="row"><span>look<Tip t={TIP.theme} /></span><Drop label="look" value={r.theme} options={r.themes.map(t => t.name)} onChange={r.setTheme} /></div>
      </div>
    </>}

    {planned && !written && r.busy !== "writing" && <>
      {draft.questions.length > 0 && <>
        <h2 className="sec">a few questions</h2>
        <div className="rows">
          {draft.questions.map((q, i) => <div className="row stack" key={i}>
            <span>{q.ask}</span>
            <div className="opts">
              {q.options.map(o => <button key={o} aria-pressed={(q.answer || q.default) === o} onClick={() => r.setDraft({ questions: list(draft.questions, i, { answer: o }) })}>{o}{o === q.default && !q.answer ? " (default)" : ""}</button>)}
            </div>
          </div>)}
        </div>
      </>}
      <h2 className="sec">outline, about {draft.pages} pages</h2>
      <ol className="outline">
        {sections.map((s, i) => <li key={i}>
          <input className="ri" value={s.title} aria-label="section title" onChange={e => r.setDraft({ sections: list(sections, i, { title: e.target.value }) })} />
          <span className="muted">{s.needs.filter(n => n !== "text").join(", ")}</span>
          <button className="x" aria-label="Move up" disabled={i === 0} onClick={() => r.setDraft({ sections: sections.map((x, k) => k === i - 1 ? sections[i] : k === i ? sections[i - 1] : x) })}>↑</button>
          <button className="x" aria-label="Remove section" onClick={() => r.setDraft({ sections: sections.filter((_, k) => k !== i) })}>×</button>
        </li>)}
      </ol>
      <button className="more-opts" onClick={() => r.setDraft({ sections: [...sections, { title: "new section", numbered: true, brief: "", words: 200, needs: ["text"], state: "waiting", blocks: [] }] })}>[+] section</button>
      <div className="rows">
        <Switch label="code-based" tip={TIP.code} checked={draft.code_based} onChange={code_based => r.setDraft({ code_based })} />
        {noun === "report" && <Switch label="contents page" tip={TIP.toc} checked={draft.toc} onChange={toc => r.setDraft({ toc })} />}
      </div>
    </>}

    {(written || r.busy === "writing") && <ul className="secs">
      {sections.map((s, i) => <li key={i} data-state={s.state}>
        <span className="state">{STATE[s.state] ?? "·"}</span>
        <span className="name">{s.title}</span>
        {!r.busy && <>
          <button className="link" onClick={() => setNoting({ i, note: "" })}>redo</button>
          <button className="link" onClick={() => setEditing({ i, text: toText(s.blocks) })}>edit</button>
        </>}
        {noting?.i === i && <form className="note-row" onSubmit={e => { e.preventDefault(); r.redo(i, noting.note); setNoting(null) }}>
          <input className="ri" autoFocus value={noting.note} placeholder="what to change (shorter, mention jdbc…)" onChange={e => setNoting({ i, note: e.target.value })} />
          <button className="btn small">redo</button>
        </form>}
      </li>)}
    </ul>}

    {written && !r.busy && <div className="tray">
      <div className="tray-copy"><strong>add screenshots</strong><span>then redo the section that shows them</span></div>
      <button className="btn small" onClick={() => input.current?.click()}>files</button>
    </div>}

    <div className="action" aria-live="polite">
      {r.busy ? <>
        <div className="prog"><span className="prog-text">{r.busy}…</span></div>
        <button className="btn" onClick={r.stop}>stop</button>
      </> : !planned ? <button className="btn primary" disabled={!cover.title.trim()} onClick={r.plan}>{cover.title.trim() ? `plan the ${noun}` : "give it a title to start"}</button>
        : !written ? <>
          <button className="btn primary" onClick={() => r.write(false)}>write</button>
          {draft.questions.length > 0 && <button className="btn" title="skip the questions and take the defaults" onClick={() => r.write(true)}>assume &amp; write</button>}
          <button className="btn" title="back to the files and the cover" onClick={() => r.setDraft({ sections: [], questions: [] })}>back</button>
        </> : <>
          {r.doc && <a className="btn primary" href={docUrl(r.doc.job, r.doc.n)} download>download</a>}
          {sections.some(s => s.state !== "done") && <button className="btn" onClick={() => r.write(false)}>finish</button>}
          <button className="btn" title="start another report, keeping the cover" onClick={r.fresh}>new</button>
        </>}
    </div>

    {editing && <div className="modal" onClick={() => setEditing(null)}>
      <div className="modal-card" onClick={e => e.stopPropagation()}>
        <button className="btn small close" onClick={() => setEditing(null)}>close</button>
        <h3>{sections[editing.i]?.title}</h3>
        <p className="note">blank lines split paragraphs; ## sub-heading, - bullets, 1. steps; [[…]] keeps a table or figure</p>
        <textarea className="ri edit" rows={18} value={editing.text} onChange={e => setEditing({ ...editing, text: e.target.value })} />
        <div className="action"><button className="btn primary" onClick={() => { r.edit(editing.i, editing.text); setEditing(null) }}>save</button></div>
      </div>
    </div>}
  </>;
}
