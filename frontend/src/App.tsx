import { useEffect, useRef, useState } from "react";
import { DEFAULTS, clearJob, docUrl, followJob, previewUrl, startJob, type JobEvent, type Mode, type Settings, type Upload } from "./api";
import { DocPreview, Groups, Idle, type Doc } from "./Live";
import { SettingsView, Toml } from "./Settings";

type Phase = "idle" | "running" | "done";

function useStored<T>(key: string, initial: T): [T, (v: T) => void] {
  const [value, setValue] = useState<T>(() => {
    try {
      const saved = localStorage.getItem(key);
      return saved ? { ...initial, ...JSON.parse(saved) } : initial;
    } catch { return initial }
  });
  const store = (v: T) => {
    setValue(v);
    try { localStorage.setItem(key, JSON.stringify(v)) } catch { /* private mode */ }
  };
  return [value, store];
}

function useDebounced<T>(value: T, ms: number) {
  const [current, setCurrent] = useState(value);
  useEffect(() => { const t = setTimeout(() => setCurrent(value), ms); return () => clearTimeout(t) }, [value, ms]);
  return current;
}

const stem = (name: string) => name.replace(/^.*\//, "").replace(/\.[^.]+$/, "");
const size = (n: number) => n < 1024 * 1024 ? `${Math.max(1, Math.round(n / 1024))} KB` : `${(n / 1024 / 1024).toFixed(1)} MB`;
const SAY: Record<string, string> = { writing: "writing the answer", running: "running it", fixing: "fixing it", ok: "done", failed: "failed" };

function reduce(docs: Doc[], e: JobEvent, mode: Mode): Doc[] {
  if (e.t === "docs") return e.items.map(d => ({ id: d.id, title: d.title, status: "waiting to start", error: false, tiles: null }));
  return docs.map(d => {
    if (d.id !== e.doc) return d;
    switch (e.t) {
      case "reading": return { ...d, status: e.pages ? `reading ${e.pages} pages` : "reading the file" };
      case "found": return { ...d, status: `found ${e.items.length} ${mode === "solve" ? "questions" : "files"}`, tiles: e.items.map(i => ({ ...i, state: "waiting", note: "", code: "", written: false, output: "", shots: [] })) };
      case "done": return { ...d, status: "ready to download", file: { n: e.file, name: e.name } };
      case "error": return { ...d, status: e.message, error: true };
    }
    if (!d.tiles || !("id" in e)) return d;
    const k = d.tiles.findIndex(t => t.id === e.id);
    if (k < 0) return d;
    const tiles = [...d.tiles], t = { ...tiles[k] };
    let status = d.status;
    if (e.t === "step") { t.state = e.state; t.note = e.note; status = `q${k + 1}: ${e.note || SAY[e.state] || e.state}` }
    if (e.t === "code") { t.code = e.code; t.written = e.written }
    if (e.t === "output") t.output = e.text.replace(/\r/g, "");
    if (e.t === "shot") t.shots = [...t.shots, e.n];
    tiles[k] = t;
    return { ...d, tiles, status };
  });
}

export default function App() {
  const [settings, setSettings] = useStored<Settings>("savecodex.settings", DEFAULTS);
  const [view, setView] = useStored("savecodex.view", { mode: "solve" as Mode, side: 380, closed: false });
  const [apiKey, setApiKey] = useState("");
  const [uploads, setUploads] = useState<Record<Mode, Upload[]>>({ solve: [], pack: [] });
  const [phase, setPhase] = useState<Phase>("idle");
  const [cfg, setCfg] = useState(false);
  const [docs, setDocs] = useState<Doc[]>([]);
  const [job, setJob] = useState("");
  const [toast, setToast] = useState("");
  const [over, setOver] = useState(false);
  const [dragging, setDragging] = useState(false);
  const fileInput = useRef<HTMLInputElement>(null), folderInput = useRef<HTMLInputElement>(null);
  const stopFollowing = useRef<() => void>(() => {});
  const mode = view.mode, files = uploads[mode];
  const set = (patch: Partial<Settings>) => setSettings({ ...settings, ...patch });
  const preview = useDebounced(previewUrl(settings), 200);

  useEffect(() => { if (toast) { const t = setTimeout(() => setToast(""), 3000); return () => clearTimeout(t) } }, [toast]);

  const add = (list: FileList | File[], pasted = false) => {
    const added = [...list].map((file, i) => ({
      file,
      path: pasted ? `screenshot ${files.length + i + 1}.png` : (file.webkitRelativePath || file.name),
    }));
    if (added.length) setUploads({ ...uploads, [mode]: [...files, ...added] });
  };

  useEffect(() => {
    const paste = (e: ClipboardEvent) => {
      const imgs = [...(e.clipboardData?.files || [])].filter(f => f.type.startsWith("image/"));
      if (!imgs.length || mode !== "solve" || phase !== "idle" || (e.target as HTMLElement).matches("input, textarea")) return;
      e.preventDefault();
      add(imgs, true);
    };
    addEventListener("paste", paste);
    return () => removeEventListener("paste", paste);
  });

  const run = async () => {
    stopFollowing.current();
    if (job) clearJob(job);
    setCfg(false);
    setDocs([]);
    setPhase("running");
    try {
      const id = await startJob(mode, files, settings, apiKey);
      setJob(id);
      stopFollowing.current = followJob(id, e => {
        setDocs(d => reduce(d, e, mode));
        if (e.t === "finished") setPhase("done");
      });
    } catch (err) {
      setPhase("idle");
      setToast(`couldn't start: ${(err as Error).message}`);
    }
  };
  const stop = () => { stopFollowing.current(); if (job) clearJob(job); setJob(""); setPhase("idle") };
  const clear = () => { stop(); setDocs([]); setUploads({ ...uploads, [mode]: [] }) };
  const download = (list: Doc[]) => list.filter(d => d.file).forEach((d, i) => setTimeout(() => {
    const a = document.createElement("a");
    a.href = docUrl(job, d.file!.n);
    a.download = d.file!.name;
    a.click();
  }, i * 400));

  const grip = (e: React.PointerEvent<HTMLDivElement>) => {
    const el = e.currentTarget;
    el.setPointerCapture(e.pointerId);
    setDragging(true);
    let width = view.side;
    const move = (ev: PointerEvent) => {
      width = Math.min(Math.max(innerWidth - ev.clientX, 300), Math.min(720, innerWidth - 360));
      (el.closest(".app") as HTMLElement).style.setProperty("--side", `${width}px`);
    };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", () => { el.removeEventListener("pointermove", move); setDragging(false); setView({ ...view, side: width }) }, { once: true });
  };
  const nudge = (e: React.KeyboardEvent) => {
    const step = e.key === "ArrowLeft" ? 24 : e.key === "ArrowRight" ? -24 : 0;
    if (step) setView({ ...view, side: Math.min(Math.max(view.side + step, 300), 720) });
  };

  const tiles = docs.flatMap(d => d.tiles ?? []);
  const ready = tiles.filter(t => t.state === "ok" || t.state === "failed").length;
  const found = docs.length > 0 && docs.every(d => d.tiles || d.error);
  const progress = !found ? "reading your files" : ready < tiles.length ? `${ready} of ${tiles.length} ${mode === "solve" ? "answers" : "files"} ready` : "putting your documents together";
  const pct = found && tiles.length ? ready / tiles.length : 0;
  const cells = Math.round(pct * 24);
  const finished = docs.filter(d => d.file);
  const name = files[0] ? stem(files[0].path.split("/")[0]) : mode === "solve" ? "assignment" : "code";
  const noun = (n: number) => `${n} ${n === 1 ? "file" : "files"}`;

  return (
    <div className={`app${view.closed ? " closed" : ""}${dragging ? " dragging" : ""}`} style={{ "--side": `${view.side}px` } as React.CSSProperties}>
      <main className="main">
        <div className="title brand">savecodex</div>
        {phase === "idle"
          ? cfg ? <DocPreview mode={mode} s={settings} name={name} preview={preview} /> : <Idle mode={mode} preview={preview} />
          : <Groups docs={docs} job={job} mode={mode} onDownload={d => download([d])} />}
      </main>

      <aside className="side" aria-label="Panel">
        <div className="grip" role="separator" aria-orientation="vertical" aria-label="Resize panel" tabIndex={0} onPointerDown={grip} onKeyDown={nudge} />
        <div className="side-head">
          <div className="seg" role="group" aria-label="Mode">
            {(["solve", "pack"] as Mode[]).map(m => <button key={m} aria-pressed={mode === m} disabled={phase !== "idle"} onClick={() => setView({ ...view, mode: m })}>{m}</button>)}
          </div>
          <button className="fold" aria-label={view.closed ? "Show panel" : "Hide panel"} onClick={() => setView({ ...view, closed: !view.closed })}>{view.closed ? "«" : "»"}</button>
        </div>

        {cfg && phase === "idle" ? (
          <SettingsView mode={mode} s={settings} set={set} apiKey={apiKey} setApiKey={setApiKey} onBack={() => setCfg(false)} />
        ) : <>
          <h1 className="title">{mode === "solve" ? "solve an assignment" : "pack your code"}</h1>
          {phase === "idle" && <>
            <div className={`tray${over ? " over" : ""}`}
              onDragOver={e => { e.preventDefault(); setOver(true) }} onDragLeave={() => setOver(false)}
              onDrop={e => { e.preventDefault(); setOver(false); add(e.dataTransfer.files) }}>
              <div className="tray-copy">
                <strong>{mode === "solve" ? "drop files here" : "drop code here"}</strong>
                <span>{mode === "solve" ? "pdf, docx, images, or paste" : "code files or a whole folder"}</span>
              </div>
              <button className="btn small" onClick={() => fileInput.current?.click()}>{mode === "solve" ? "browse" : "files"}</button>
              {mode === "pack" && <button className="btn small" onClick={() => folderInput.current?.click()}>folder</button>}
              <input ref={fileInput} type="file" multiple hidden onChange={e => { add(e.target.files!); e.target.value = "" }} />
              <input ref={folderInput} type="file" hidden {...{ webkitdirectory: "" }} onChange={e => { add(e.target.files!); e.target.value = "" }} />
            </div>
            {files.length > 0 && (
              <ul className="tree">
                {files.map((u, i) => (
                  <li key={u.path + i}>
                    <span className="name">{u.path}</span><span className="meta">{size(u.file.size)}</span>
                    <button className="x" aria-label={`Remove ${u.path}`} onClick={() => setUploads({ ...uploads, [mode]: files.filter((_, k) => k !== i) })}>×</button>
                  </li>
                ))}
              </ul>
            )}
            <Toml s={settings} set={set} onMore={() => setCfg(true)} />
          </>}
          <div className="action" aria-live="polite">
            {phase === "idle" && <button className="btn primary" disabled={!files.length} onClick={run}>{files.length ? `${mode} ${noun(files.length)}` : "add a file to start"}</button>}
            {phase === "running" && <>
              <div className="prog"><span className="prog-text">{progress}</span><span className="prog-bar">[{"█".repeat(cells)}{"░".repeat(24 - cells)}] {Math.round(pct * 100)}%</span></div>
              <button className="btn" onClick={stop}>stop</button>
            </>}
            {phase === "done" && <>
              {finished.length > 0 && <button className="btn primary" onClick={() => download(finished)}>{finished.length > 1 ? "download all" : "download"}</button>}
              <button className="btn" title={`${mode} again`} onClick={run}>↻ again</button>
              <button className="btn" title="Remove the files and results" onClick={clear}>clear</button>
            </>}
          </div>
        </>}
      </aside>
      {toast && <div className="toast" role="status">{toast}</div>}
    </div>
  );
}
