import { useEffect, useRef, useState } from "react";
import { DEFAULTS, clearJob, docUrl, fetchThemes, followJob, previewUrl, startJob, type DocTheme, type JobEvent, type Mode, type Settings, type Upload } from "./api";
import { DocPreview, Groups, Idle, type Doc } from "./Live";
import { SettingsView, Toml } from "./Settings";
import { ThemeView } from "./Theme";

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

const isDoc = (path: string) => /\.(pdf|docx)$/i.test(path);

function reduce(docs: Doc[], e: JobEvent, mode: Mode, job: string): Doc[] {
  if (e.t === "docs") return e.items.map(d => ({ id: d.id, job, title: d.title, status: "waiting to start", error: false, tiles: null, since: Date.now() }));
  return docs.map(d => {
    if (d.id !== e.doc) return d;
    switch (e.t) {
      case "reading": return { ...d, since: Date.now(), status: e.pages ? `reading ${e.pages} pages` : "reading the file" };
      case "found": return { ...d, status: `found ${e.items.length} ${mode === "solve" ? "questions" : "files"}`, tiles: e.items.map(i => ({ ...i, state: "waiting", note: "", code: "", written: false, output: "", shots: [], since: Date.now() })) };
      case "done": return { ...d, status: "ready to download", file: { n: e.file, name: e.name } };
      case "error": return { ...d, status: e.message, error: true };
    }
    if (!d.tiles || !("id" in e)) return d;
    const k = d.tiles.findIndex(t => t.id === e.id);
    if (k < 0) return d;
    const tiles = [...d.tiles], t = { ...tiles[k] };
    let status = d.status;
    if (e.t === "step") { if (t.state !== e.state) t.since = Date.now(); t.state = e.state; t.note = e.note; status = `q${k + 1}: ${e.note || SAY[e.state] || e.state}` }
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
  const [editing, setEditing] = useState(false);
  const [presets, setPresets] = useState<DocTheme[]>([]);
  const [custom, setCustom] = useStored<Record<string, DocTheme>>("savecodex.themes", {});
  useEffect(() => { fetchThemes().then(setPresets).catch(() => {}) }, []);
  const [docs, setDocs] = useState<Doc[]>([]);
  const [toast, setToast] = useState("");
  const [over, setOver] = useState(false);
  const [dragging, setDragging] = useState(false);
  const fileInput = useRef<HTMLInputElement>(null), folderInput = useRef<HTMLInputElement>(null);
  const stopFollowing = useRef<() => void>(() => {});
  const active = useRef(0);
  useEffect(() => {
    if (phase !== "running") return;
    const leave = () => stopFollowing.current();
    addEventListener("pagehide", leave);
    return () => removeEventListener("pagehide", leave);
  }, [phase]);
  const mode = view.mode, files = uploads[mode];
  const set = (patch: Partial<Settings>) => setSettings({ ...settings, ...patch });
  const preview = useDebounced(previewUrl(settings), 200);
  const themes = [...presets, ...Object.values(custom)];
  const theme = themes.find(t => t.name === settings.doc_theme) ?? presets[0];
  const names = themes.map(t => t.name);

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

  /** Follows a job until it finishes; stopping later also drops it on the server. */
  const follow = (id: string, onEvent: (e: JobEvent) => void) => {
    active.current++;
    const unfollow = followJob(id, e => {
      onEvent(e);
      if (e.t === "finished" && --active.current === 0) setPhase("done");
    });
    const prev = stopFollowing.current;
    stopFollowing.current = () => { prev(); unfollow(); clearJob(id) };
  };
  const unfollowAll = () => { stopFollowing.current(); stopFollowing.current = () => {}; active.current = 0 };

  const run = async () => {
    unfollowAll();
    setCfg(false);
    setEditing(false);
    setDocs([]);
    setPhase("running");
    try {
      const id = await startJob(mode, files, settings, theme, apiKey);
      follow(id, e => setDocs(d => reduce(d, e, mode, id)));
    } catch (err) {
      setPhase("idle");
      setToast(`couldn't start: ${(err as Error).message}`);
    }
  };
  const stop = () => { unfollowAll(); setPhase("idle") };
  /** Solves one document of several again, as its own job, keeping the others. */
  const redo = async (d: Doc) => {
    const mine = [files.filter(u => isDoc(u.path))[Number(d.id)], ...files.filter(u => !isDoc(u.path))];
    const patch = (f: (x: Doc) => Doc) => setDocs(ds => ds.map(x => x.id === d.id ? f(x) : x));
    patch(x => ({ ...x, status: "waiting to start", error: false, tiles: null, file: undefined, since: Date.now() }));
    setPhase("running");
    try {
      const id = await startJob(mode, mine, settings, theme, apiKey);
      patch(x => ({ ...x, job: id }));
      follow(id, e => e.t !== "docs" && patch(x => ({ ...reduce([{ ...x, id: e.doc }], e, mode, id)[0], id: x.id })));
    } catch (err) {
      patch(x => ({ ...x, status: `couldn't start: ${(err as Error).message}`, error: true }));
      if (!active.current) setPhase("done");
    }
  };
  const home = () => { setCfg(false); setEditing(false); if (phase === "done") { stop(); setDocs([]) } };
  const clear = () => { stop(); setDocs([]); setUploads({ ...uploads, [mode]: [] }) };
  const download = (list: Doc[]) => list.filter(d => d.file).forEach((d, i) => setTimeout(() => {
    const a = document.createElement("a");
    a.href = docUrl(d.job, d.file!.n);
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
  const name = files[0] ? stem(files[0].path.split("/")[0]) : mode === "solve" ? "document" : "code";
  const noun = (n: number) => `${n} ${n === 1 ? "file" : "files"}`;

  return (
    <div className={`app${view.closed ? " closed" : ""}${dragging ? " dragging" : ""}`} style={{ "--side": `${view.side}px` } as React.CSSProperties}>
      <main className="main">
        <button className="title brand" onClick={home}>savecodex</button>
        {phase === "idle"
          ? cfg && theme ? <DocPreview mode={mode} s={settings} t={theme} name={name} preview={preview} /> : <Idle mode={mode} preview={preview} />
          : <Groups docs={docs} mode={mode} onDownload={d => download([d])} onRedo={docs.length > 1 ? redo : undefined} />}
      </main>

      <aside className="side" aria-label="Panel">
        <div className="grip" role="separator" aria-orientation="vertical" aria-label="Resize panel" tabIndex={0} onPointerDown={grip} onKeyDown={nudge} />
        <div className="side-head">
          <div className="seg" role="group" aria-label="Mode">
            {(["solve", "pack"] as Mode[]).map(m => <button key={m} aria-pressed={mode === m} disabled={phase !== "idle"} onClick={() => setView({ ...view, mode: m })}>{m}</button>)}
          </div>
          <button className="fold" aria-label={view.closed ? "Show panel" : "Hide panel"} onClick={() => setView({ ...view, closed: !view.closed })}>{view.closed ? "«" : "»"}</button>
        </div>

        {cfg && phase === "idle" ? editing && theme ? (
          <ThemeView theme={theme} names={names} custom={custom} setCustom={setCustom} select={doc_theme => set({ doc_theme })} onBack={() => setEditing(false)} />
        ) : (
          <SettingsView mode={mode} s={settings} set={set} themes={names} onEditTheme={() => setEditing(true)} apiKey={apiKey} setApiKey={setApiKey} onBack={() => setCfg(false)} />
        ) : <>
          <h1 className="title">{mode === "solve" ? "solve a document" : "pack your code"}</h1>
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
            <Toml s={settings} set={set} themes={names} onMore={() => setCfg(true)} />
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
        <a className="credit" href="https://github.com/kartinul/savecodex" target="_blank" rel="noopener noreferrer">
          made by kartinul
          <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true" fill="currentColor"><path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0016 8c0-4.42-3.58-8-8-8z" /></svg>
        </a>
      </aside>
      {toast && <div className="toast" role="status">{toast}</div>}
    </div>
  );
}
