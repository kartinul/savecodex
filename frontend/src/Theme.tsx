import type { ReactNode } from "react";
import { CODE_FONTS, FONTS, type DocTheme, type Look } from "./api";
import { Seg, Switch } from "./Settings";

type Change = (patch: Partial<DocTheme>) => void;

function Num({ label, value, step = 1, max = 72, onChange }: { label: string; value: number; step?: number; max?: number; onChange: (v: number) => void }) {
  return (
    <label className="row"><span>{label}</span>
      <input className="ri num" type="number" min={0} max={max} step={step} value={value} onChange={e => onChange(+e.target.value)} />
    </label>
  );
}

function Font({ label, value, fonts, onChange }: { label: string; value: string; fonts: string[]; onChange: (v: string) => void }) {
  return (
    <label className="row"><span>{label}</span>
      <select className="ri" value={value} onChange={e => onChange(e.target.value)}>
        <option value="">same as text</option>
        {fonts.map(f => <option key={f}>{f}</option>)}
      </select>
    </label>
  );
}

const Group = ({ title, children }: { title: string; children: ReactNode }) => (
  <details><summary>{title}</summary><div className="rows">{children}</div></details>
);

const MARKS: [keyof Look, string][] = [["bold", "b"], ["italic", "i"], ["underline", "u"], ["center", "mid"], ["rule", "line"]];

function LookRows({ look, onChange }: { look: Look; onChange: (l: Look) => void }) {
  const set = (patch: Partial<Look>) => onChange({ ...look, ...patch });
  return <>
    <Font label="font" value={look.font} fonts={FONTS} onChange={font => set({ font })} />
    <Num label="size (0 = text)" value={look.size} onChange={size => set({ size })} />
    <div className="row"><span>style</span>
      <div className="seg">{MARKS.map(([k, l]) => <button key={k} aria-pressed={!!look[k]} onClick={() => set({ [k]: !look[k] })}>{l}</button>)}</div>
    </div>
    <label className="row"><span>colour</span>
      <span className="pick">
        <input type="color" className="swatch" value={`#${look.color || "000000"}`} onChange={e => set({ color: e.target.value.slice(1).toUpperCase() })} />
        {look.color && <button className="btn small" onClick={() => set({ color: "" })}>auto</button>}
      </span>
    </label>
    <Num label="space above (pt)" value={look.before} onChange={before => set({ before })} />
    <label className="row"><span>space below (pt)</span>
      <input className="ri num" type="number" min={0} max={72} value={look.after ?? ""} placeholder="text" onChange={e => set({ after: e.target.value === "" ? null : +e.target.value })} />
    </label>
  </>;
}

/** Edits the doc theme in place; editing a preset first copies it under a new name. */
export function ThemeView({ theme, names, custom, setCustom, select, onBack }: {
  theme: DocTheme; names: string[]; custom: Record<string, DocTheme>;
  setCustom: (c: Record<string, DocTheme>) => void; select: (name: string) => void; onBack: () => void;
}) {
  const own = theme.name in custom;
  const fresh = (base: string) => { let n = base, i = 2; while (names.includes(n)) n = `${base} ${i++}`; return n };
  const change: Change = patch => {
    if (own) return setCustom({ ...custom, [theme.name]: { ...theme, ...patch } });
    const name = fresh(`my ${theme.name}`);
    setCustom({ ...custom, [name]: { ...theme, ...patch, name } });
    select(name);
  };
  const without = () => Object.fromEntries(Object.entries(custom).filter(([n]) => n !== theme.name));
  const rename = (name: string) => {
    name = name.trim();
    if (!name || names.includes(name)) return;
    setCustom({ ...without(), [name]: { ...theme, name } });
    select(name);
  };
  const remove = () => { setCustom(without()); select("current") };
  const download = () => {
    const a = document.createElement("a");
    a.href = URL.createObjectURL(new Blob([JSON.stringify(theme, null, 2)], { type: "application/json" }));
    a.download = `${theme.name}.json`;
    a.click();
    URL.revokeObjectURL(a.href);
  };
  const heading = (i: number) => (look: Look) => change({ headings: theme.headings.map((h, k) => k === i ? look : h) as DocTheme["headings"] });

  return (
    <div className="cfg">
      <header className="cfg-head"><h1 className="title">doc theme</h1><button className="btn small" onClick={onBack}>back</button></header>
      <div className="rows">
        <label className="row"><span>theme</span>
          <select className="ri" value={theme.name} onChange={e => select(e.target.value)}>{names.map(n => <option key={n}>{n}</option>)}</select>
        </label>
        {own
          ? <label className="row"><span>name</span>
              <input key={theme.name} className="ri" defaultValue={theme.name} spellCheck={false}
                onBlur={e => rename(e.target.value)} onKeyDown={e => e.key === "Enter" && e.currentTarget.blur()} />
            </label>
          : <div className="row muted">a preset: changing it saves your own copy</div>}
        <div className="row"><span />
          <span className="pick">
            <button className="btn small" onClick={download}>download .json</button>
            {own && <button className="btn small" onClick={remove}>delete</button>}
          </span>
        </div>
      </div>
      <Group title="page">
        <div className="row"><span>paper</span><Seg value={theme.paper} options={[["a4", "a4"], ["letter", "letter"]]} onChange={paper => change({ paper })} /></div>
        <Num label="margins (cm)" step={0.1} max={6} value={theme.margin} onChange={margin => change({ margin })} />
        <Num label="top margin (cm)" step={0.1} max={6} value={theme.margin_top} onChange={margin_top => change({ margin_top })} />
        <Switch label="page numbers" checked={theme.page_numbers} onChange={page_numbers => change({ page_numbers })} />
      </Group>
      <Group title="text">
        <label className="row"><span>font</span>
          <select className="ri" value={theme.font} onChange={e => change({ font: e.target.value })}>{FONTS.map(f => <option key={f}>{f}</option>)}</select>
        </label>
        <Num label="size" value={theme.size} onChange={size => change({ size })} />
        <Num label="line spacing" step={0.05} max={3} value={theme.line} onChange={line => change({ line })} />
        <Num label="space after (pt)" value={theme.gap} onChange={gap => change({ gap })} />
      </Group>
      <Group title="title"><LookRows look={theme.title} onChange={title => change({ title })} /></Group>
      <Group title="heading 1">
        <label className="row"><span>number prefix</span>
          <input className="ri" value={theme.prefix} placeholder="none, Q, Question " onChange={e => change({ prefix: e.target.value })} />
        </label>
        <LookRows look={theme.headings[0]} onChange={heading(0)} />
      </Group>
      <Group title="heading 2"><LookRows look={theme.headings[1]} onChange={heading(1)} /></Group>
      <Group title="heading 3"><LookRows look={theme.headings[2]} onChange={heading(2)} /></Group>
      <Group title="question">
        <Switch label="show the question" checked={theme.show_question} onChange={show_question => change({ show_question })} />
        <LookRows look={theme.question} onChange={question => change({ question })} />
      </Group>
      <Group title="code">
        <Font label="font" value={theme.code_font} fonts={CODE_FONTS} onChange={code_font => change({ code_font })} />
        <Num label="size" value={theme.code_size} onChange={code_size => change({ code_size })} />
        <div className="row"><span>box</span><Seg value={theme.code_box} options={[["none", "none"], ["shade", "shaded"], ["border", "border"]]} onChange={code_box => change({ code_box })} /></div>
        <Switch label="code: / output: labels" checked={theme.labels} onChange={labels => change({ labels })} />
      </Group>
      <Group title="screenshots">
        <div className="row"><span>size</span><Seg value={theme.shots} options={[["small", "small"], ["medium", "medium"], ["full", "full"]]} onChange={shots => change({ shots })} /></div>
        <Switch label="centred" checked={theme.shots_center} onChange={shots_center => change({ shots_center })} />
      </Group>
    </div>
  );
}
