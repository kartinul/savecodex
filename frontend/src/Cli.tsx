import { useState } from "react";

type Os = "macos" | "windows" | "linux";

const RELEASE = "https://github.com/kartinul/savecodex/releases/latest/download/";
const WINDOWS_FILE = "savecodex-windows-amd64.exe";
const AGENTS = ["claude", "codex", "gemini", "copilot", "cursor-agent", "opencode"];
const detected: Os = /Windows/.test(navigator.userAgent) ? "windows" : /Mac/.test(navigator.userAgent) ? "macos" : "linux";

const link = (href: string, text: string, className?: string) => <a className={className} href={href} target="_blank" rel="noopener noreferrer">{text}</a>;
const unix = (asset: string) => [`curl -Lo savecodex ${RELEASE}${asset}`, "chmod +x savecodex && sudo mv savecodex /usr/local/bin/"];

const GUIDE: Record<Os, { prompt: string; open: string; install: string[]; hint: string; key: string; run: string }> = {
  macos: {
    prompt: "$",
    open: "open Terminal (cmd + space, type terminal) and paste this",
    install: unix("savecodex-macos-arm64"),
    hint: "apple silicon macs only.",
    key: "export GEMINI_API_KEY=your_key",
    run: "savecodex",
  },
  linux: {
    prompt: "$",
    open: "open a terminal and paste this",
    install: unix("savecodex-linux-amd64"),
    hint: "",
    key: "export GEMINI_API_KEY=your_key",
    run: "savecodex",
  },
  windows: {
    prompt: ">",
    open: "then open PowerShell (windows key, type powershell) and paste this",
    install: ["cd ~\\Downloads", `ren ${WINDOWS_FILE} savecodex.exe`],
    hint: "if windows warns about an unknown app, pick more info, then run anyway.",
    key: '$env:GEMINI_API_KEY = "your_key"',
    run: ".\\savecodex",
  },
};

/** What to do sits inside the box as a comment, since the box is what gets read. */
function Cmd({ prompt, note, lines }: { prompt: string; note: string; lines: string[] }) {
  const [label, setLabel] = useState("copy");
  const copy = () => (navigator.clipboard?.writeText(lines.join("\n")) ?? Promise.reject())
    .then(() => "copied", () => "failed")
    .then(said => { setLabel(said); setTimeout(() => setLabel("copy"), 1500) });
  return (
    <div className="cmd">
      <span className="cmd-note">{note}</span>
      <button className="link" aria-live="polite" onClick={copy}>{label}</button>
      <pre>{lines.map(line => <code key={line} data-prompt={prompt}>{line}</code>)}</pre>
    </div>
  );
}

export function Cli({ onBack }: { onBack: () => void }) {
  const [os, setOs] = useState(detected);
  const guide = GUIDE[os];
  return (
    <section className="cli">
      <header className="cfg-head">
        <h1 className="title">use it from your terminal</h1>
        <button className="btn small" onClick={onBack}>back</button>
      </header>
      <p className="muted">the same solve and pack, on your own computer with your own ai. no limits, any model.</p>
      <div className="seg" role="group" aria-label="Your system">
        {(["macos", "windows", "linux"] as Os[]).map(o => <button key={o} aria-pressed={os === o} onClick={() => setOs(o)}>{o}</button>)}
      </div>

      <ol className="cli-steps">
        <li className="step">
          <h2 className="title">get it</h2>
          {os === "windows" && <a className="btn primary" href={RELEASE + WINDOWS_FILE}>download for windows</a>}
          <Cmd prompt={guide.prompt} note={guide.open} lines={guide.install} />
          {guide.hint && <p className="note">{guide.hint}</p>}
        </li>

        <li className="step">
          <h2 className="title">give it an ai</h2>
          <div className="either">
            <div className="card have">
              <strong>already use one of these in your terminal?</strong>
              <div className="chips">{AGENTS.map(agent => <span key={agent}>{agent}</span>)}</div>
              <span><strong>✓ nothing to do.</strong> savecodex finds it by itself</span>
            </div>
            <div className="card">
              <strong>none of them?</strong>
              {link("https://aistudio.google.com/app/apikey", "get a free gemini key", "btn small")}
              <Cmd prompt={guide.prompt} note="add your key, then paste" lines={[guide.key]} />
            </div>
          </div>
        </li>

        <li className="step">
          <h2 className="title">run it</h2>
          <Cmd prompt={guide.prompt} note="solve an assignment, you get lab7_solved.docx" lines={[`${guide.run} solve lab7.pdf`]} />
          <Cmd prompt={guide.prompt} note="or pack code you already wrote into lab5_pack.docx" lines={[`${guide.run} pack lab5`]} />
          <p className="note">every option is in the {link("https://github.com/kartinul/savecodex#commands", "readme")}, or run <code>{guide.run} --help</code></p>
        </li>
      </ol>
    </section>
  );
}
