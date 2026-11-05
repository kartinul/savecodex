import type { Mode, Settings } from "./api";

export const TIP = {
  theme: "how the terminal screenshots look: which computer, dark or light",
  window: "which computer the terminal screenshots look like they came from",
  colors: "dark or light terminal in the screenshots",
  username: "your name in the terminal line, like user@laptop",
  hostname: "the computer name after the @ in the terminal line",
  dir: "the folder the terminal shows your code running in; {} is the file name",
  messy: "makes screenshots look taken by hand: uneven crop, a scrollbar, an unfocused window; the dice shows another sample",
  cols: "how wide the terminal window is at least, in characters",
  title: "the big heading at the top of the document; empty uses the file name, {} is the file name",
  text: "written under the title, like your name and roll number; {} is the file name",
  file: "the name of the word file you download; {} is the uploaded file name",
  pages: "start each answer on a new page",
  rules: "your own rules the ai follows before anything else, like \"use java\" or \"comment every line\"",
  check: "the ai grades every answer and redoes the wrong ones; takes longer",
  key: "use your own openai-compatible account instead of the shared one; never saved",
  url: "where your key works; leave empty for openai",
  model: "which model your key should use, like gpt-4o-mini",
};

export const Tip = ({ t }: { t: string }) => (
  <span className="q" tabIndex={0} role="note" aria-label={t} data-tip={t} onClick={e => e.preventDefault()}>?</span>
);

const LOOKS = ["windows-dark", "windows-light", "macos-dark", "macos-light", "ubuntu-dark", "ubuntu-light"];
type Set = (patch: Partial<Settings>) => void;

function Inline({ k, v, ph, tip, onChange }: { k: string; v: string; ph?: string; tip: string; onChange: (v: string) => void }) {
  return (
    <div><span className="k">{k}<Tip t={tip} /></span> = <span className="tq">
      <input className="tv" value={v} placeholder={ph} style={{ width: `${Math.max((v || ph || "").length, 1)}ch` }}
        autoComplete="off" spellCheck={false} aria-label={k} onChange={e => onChange(e.target.value)} />
    </span></div>
  );
}

export function Toml({ s, set, onMore }: { s: Settings; set: Set; onMore: () => void }) {
  const look = `${s.style}-${s.theme}`;
  const next = () => {
    const [style, theme] = LOOKS[(LOOKS.indexOf(look) + 1) % LOOKS.length].split("-") as [Settings["style"], Settings["theme"]];
    set({ style, theme });
  };
  return (
    <div className="toml">
      <div className="muted"># settings.toml</div>
      <div><span className="k">theme<Tip t={TIP.theme} /></span> = <button className="v" onClick={next}>"{look}"</button></div>
      <Inline k="username" v={s.username} tip={TIP.username} onChange={username => set({ username })} />
      <Inline k="hostname" v={s.hostname} tip={TIP.hostname} onChange={hostname => set({ hostname })} />
      <Inline k="dir" v={s.cwd} tip={TIP.dir} onChange={cwd => set({ cwd })} />
      <Inline k="title" v={s.doc_title} ph="same as the upload" tip={TIP.title} onChange={doc_title => set({ doc_title })} />
      <button className="more-opts" onClick={onMore}>[+] more options</button>
    </div>
  );
}

function Seg<T extends string>({ value, options, onChange }: { value: T; options: [T, string][]; onChange: (v: T) => void }) {
  return <div className="seg">{options.map(([v, label]) => <button key={v} aria-pressed={v === value} onClick={() => onChange(v)}>{label}</button>)}</div>;
}

function Text({ label, tip, value, ph, onChange, stack, masked }: { label: string; tip: string; value: string; ph?: string; onChange: (v: string) => void; stack?: boolean; masked?: boolean }) {
  return (
    <label className={`row${stack ? " stack" : ""}`}>
      <span>{label}<Tip t={tip} /></span>
      <input className={`ri${masked ? " masked" : ""}`} value={value} placeholder={ph} autoComplete="off" spellCheck={false} data-1p-ignore data-lpignore="true" onChange={e => onChange(e.target.value)} />
    </label>
  );
}

function Switch({ label, tip, checked, onChange }: { label: string; tip: string; checked: boolean; onChange: (v: boolean) => void }) {
  return <label className="row"><span>{label}<Tip t={tip} /></span><input type="checkbox" className="sw" checked={checked} onChange={e => onChange(e.target.checked)} /></label>;
}

export function SettingsView({ mode, s, set, apiKey, setApiKey, onBack }: { mode: Mode; s: Settings; set: Set; apiKey: string; setApiKey: (k: string) => void; onBack: () => void }) {
  return (
    <div className="cfg">
      <header className="cfg-head"><h1 className="title">settings</h1><button className="btn small" onClick={onBack}>back</button></header>
      <div className="rows">
        <div className="row"><span>window<Tip t={TIP.window} /></span><Seg value={s.style} options={[["windows", "windows"], ["macos", "mac"], ["ubuntu", "ubuntu"]]} onChange={style => set({ style })} /></div>
        <div className="row"><span>colors<Tip t={TIP.colors} /></span><Seg value={s.theme} options={[["dark", "dark"], ["light", "light"]]} onChange={theme => set({ theme })} /></div>
        <Text label="username" tip={TIP.username} value={s.username} onChange={username => set({ username })} />
        <Text label="hostname" tip={TIP.hostname} value={s.hostname} onChange={hostname => set({ hostname })} />
        <Text label="dir" tip={TIP.dir} value={s.cwd} onChange={cwd => set({ cwd })} />
        <div className="row"><span>messy<Tip t={TIP.messy} /></span>
          <span className="messy">
            <input type="range" min={0} max={3} step={1} value={s.messy} aria-label="messy" onChange={e => set({ messy: +e.target.value })} />
            <span>{s.messy}</span>
            <button className="dice" disabled={!s.messy} title="another sample" aria-label="another sample" onClick={() => set({ seed: Math.floor(Math.random() * 1e9) })}>⚄</button>
          </span>
        </div>
        <label className="row"><span>min width<Tip t={TIP.cols} /></span>
          <input className="ri num" type="number" min={40} max={200} value={s.min_cols} onChange={e => set({ min_cols: +e.target.value || 80 })} />
        </label>
      </div>
      <h2 className="sec">document</h2>
      <div className="rows">
        <Text label="title" tip={TIP.title} value={s.doc_title} ph="same as the upload" onChange={doc_title => set({ doc_title })} />
        <label className="row stack"><span>text under the title<Tip t={TIP.text} /></span>
          <textarea className="ri" rows={2} value={s.doc_text} placeholder="name, roll number, anything else" onChange={e => set({ doc_text: e.target.value })} />
        </label>
        <Text label="file name" tip={TIP.file} value={s.output} ph={mode === "solve" ? "{}_solved" : "{}_pack"} onChange={output => set({ output })} />
        <Switch label={mode === "solve" ? "new page per question" : "new page per file"} tip={TIP.pages} checked={s.page_break} onChange={page_break => set({ page_break })} />
      </div>
      {mode === "solve" && <>
        <h2 className="sec">answers</h2>
        <div className="rows">
          <label className="row stack"><span>ai instructions<Tip t={TIP.rules} /></span>
            <textarea className="ri" rows={3} value={s.instructions} placeholder="use java, keep answers short, comment the code…" onChange={e => set({ instructions: e.target.value })} />
          </label>
          <Switch label="double-check answers" tip={TIP.check} checked={s.check} onChange={check => set({ check })} />
        </div>
      </>}
      <details>
        <summary>own api key</summary>
        <div className="rows">
          <Text stack masked label="key" tip={TIP.key} value={apiKey} ph="sk-..." onChange={setApiKey} />
          <Text stack label="base url" tip={TIP.url} value={s.base_url} ph="https://api.openai.com/v1" onChange={base_url => set({ base_url })} />
          <Text stack label="model" tip={TIP.model} value={s.model} ph="gpt-4o-mini" onChange={model => set({ model })} />
        </div>
      </details>
    </div>
  );
}
