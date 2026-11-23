export type Mode = "solve" | "pack";

export type Settings = {
  style: "windows" | "macos" | "ubuntu";
  theme: "dark" | "light";
  username: string;
  hostname: string;
  cwd: string;
  messy: number;
  min_cols: number;
  seed: number;
  doc_title: string;
  doc_text: string;
  output: string;
  page_break: boolean;
  doc_theme: string;
  numbering: Numbering;
  full_labels: boolean;
  check: boolean;
  level: number;
  instructions: string;
  base_url: string;
  model: string;
};

export const DEFAULTS: Settings = {
  style: "windows", theme: "dark", username: "user", hostname: "laptop", cwd: "lab", messy: 0, min_cols: 80, seed: 1,
  doc_title: "", doc_text: "", output: "", page_break: false, doc_theme: "current", numbering: "1(b)(i)", full_labels: false, check: false, level: 3, instructions: "", base_url: "", model: "",
};

export type Numbering = "1(b)(i)" | "1(2)(1)" | "1.b.i" | "1.2.1";
export const NUMBERINGS: Numbering[] = ["1(b)(i)", "1(2)(1)", "1.b.i", "1.2.1"];

export type Look = {
  font: string; size: number; color: string; bold: boolean; italic: boolean; underline: boolean; center: boolean; rule: boolean;
  before: number; after: number | null; line: number | null; spacing: number;
};
export type DocTheme = {
  name: string; paper: "a4" | "letter"; margin: number; margin_top: number;
  font: string; size: number; line: number; gap: number; kern: boolean;
  title: Look; headings: [Look, Look, Look]; question: Look;
  code_font: string; code_size: number; code_box: "none" | "shade" | "border";
  shots: "small" | "medium" | "full"; shots_center: boolean;
  page_numbers: boolean; labels: boolean; show_question: boolean; prefix: string;
};
export const FONTS = ["Aptos", "Aptos Display", "Calibri", "Calibri Light", "Cambria", "Times New Roman", "Arial", "Georgia"];
export const CODE_FONTS = ["Consolas", "Courier New"];

export const fetchThemes = (): Promise<DocTheme[]> => fetch("/api/themes").then(r => r.json());

const letter = (n: number) => n <= 26 ? String.fromCharCode(96 + n) : String(n);
const roman = (n: number) => ["i", "ii", "iii", "iv", "v", "vi", "vii", "viii", "ix", "x", "xi", "xii"][n - 1] ?? String(n);

/** A sub-part heading, like the docx: `1(b)(i)` in full, or only `(i)`. */
export function partLabel(numbering: Numbering, full: boolean, base: string, parts: number[]) {
  const seg = (depth: number, n: number) => {
    const l = depth === 0 ? letter(n) : roman(n);
    return numbering === "1(2)(1)" ? `(${n})` : numbering === "1.b.i" ? `.${l}` : numbering === "1.2.1" ? `.${n}` : `(${l})`;
  };
  if (full) return base + parts.map((n, d) => seg(d, n)).join("");
  const last = seg(parts.length - 1, parts[parts.length - 1]);
  return last.startsWith(".") ? `${last.slice(1)}.` : last;
}

export type Upload = { file: File; path: string };

export type JobEvent = { doc: string } & (
  | { t: "docs"; items: { id: string; title: string }[] }
  | { t: "reading"; pages: number }
  | { t: "found"; items: { id: string; title: string }[] }
  | { t: "step"; id: string; state: State; note: string }
  | { t: "code"; id: string; written: boolean; code: string }
  | { t: "output"; id: string; text: string }
  | { t: "shot"; id: string; n: number }
  | { t: "done"; file: number; name: string }
  | { t: "error"; message: string }
  | { t: "draft"; draft: Draft }
  | { t: "finished" }
);
export type State = "waiting" | "writing" | "running" | "fixing" | "ok" | "failed";

const shotFields = (s: Settings) => ({
  style: s.style, theme: s.theme, username: s.username, hostname: s.hostname, cwd: s.cwd,
  messy: String(s.messy), min_cols: String(s.min_cols), seed: String(s.seed),
});

export const previewUrl = (s: Settings) => `/api/preview?${new URLSearchParams(shotFields(s))}`;
export const shotUrl = (job: string, n: number) => `/api/jobs/${job}/shot/${n}`;
export const docUrl = (job: string, n: number) => `/api/jobs/${job}/doc/${n}`;
export const clearJob = (job: string) => fetch(`/api/jobs/${job}`, { method: "DELETE", keepalive: true }).catch(() => {});

export async function startJob(mode: Mode, uploads: Upload[], s: Settings, theme: DocTheme | undefined, apiKey: string): Promise<string> {
  const body = new FormData();
  const fields: Record<string, string> = {
    ...shotFields(s), doc_title: s.doc_title, doc_text: s.doc_text, output: s.output,
    page_break: String(s.page_break), doc_theme: theme ? JSON.stringify({ ...theme, numbering: s.numbering, full_labels: s.full_labels }) : s.doc_theme, check: String(s.check), level: String(s.level), instructions: s.instructions, api_key: apiKey, base_url: s.base_url, model: s.model,
  };
  for (const [k, v] of Object.entries(fields)) body.append(k, v);
  for (const u of uploads) body.append(u.path, u.file, u.file.name);
  const res = await fetch(`/api/${mode}`, { method: "POST", body });
  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error(data.error || `the server answered ${res.status}`);
  return data.id;
}

export function followJob(job: string, onEvent: (e: JobEvent) => void): () => void {
  const source = new EventSource(`/api/jobs/${job}/events`);
  source.onmessage = m => {
    const e = JSON.parse(m.data) as JobEvent;
    onEvent(e);
    if (e.t === "finished") source.close();
  };
  return () => source.close();
}

export type Member = { name: string; enrollment: string; role: string };
export type Supervisor = { honorific: string; name: string; designation: string };
export type Block =
  | { type: "para" | "heading"; text: string }
  | { type: "bullets" | "numbered"; items: string[] }
  | { type: "table"; caption: string; header: string[]; rows: string[][] }
  | { type: "figure"; caption: string; [source: string]: unknown };
export type Section = { title: string; numbered: boolean; brief: string; words: number; needs: string[]; state: string; blocks: Block[] };
export type Question = { ask: string; options: string[]; default: string; answer: string };
export type Draft = {
  kind: "report" | "synopsis"; idea: string; code_based: boolean; pages: number; toc: boolean;
  cover: { title: string; course: string; code: string; semester: string; members: Member[]; supervisors: Supervisor[] };
  look: unknown; questions: Question[]; runs: unknown[]; sections: Section[];
};
export const EMPTY_DRAFT: Draft = {
  kind: "report", idea: "", code_based: false, pages: 0, toc: false, look: null, questions: [], runs: [], sections: [],
  cover: { title: "", course: "", code: "", semester: "", members: [{ name: "", enrollment: "", role: "" }], supervisors: [{ honorific: "Dr.", name: "", designation: "" }] },
};

/** Who's asking and how: the user's own key and model, and the doc theme (JSON or a preset name). */
export type ReportFields = { doc_theme: string; api_key: string; base_url: string; model: string };
type Started = { id: string; from: number };

async function answer(res: Response): Promise<Started> {
  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw Object.assign(new Error(data.error || `the server answered ${res.status}`), { status: res.status });
  return data;
}

/** Uploads the files (the logo as logo/…) and starts planning, or with `step: "build"` only rebuilds the docx. */
export function reportStart(uploads: Upload[], logo: File | null, draft: Draft, f: ReportFields, step: "plan" | "build" = "plan"): Promise<Started> {
  const body = new FormData();
  for (const [k, v] of Object.entries({ ...f, draft: JSON.stringify(draft), step })) body.append(k, v);
  for (const u of uploads) body.append(u.path, u.file, u.file.name);
  if (logo) body.append(`logo/${logo.name}`, logo, logo.name);
  return fetch("/api/report", { method: "POST", body }).then(answer);
}

export const reportStep = (job: string, step: "write" | "redo" | "build", draft: Draft, f: ReportFields, extra: Record<string, string> = {}) =>
  fetch(`/api/report/${job}/${step}`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ ...f, ...extra, draft: JSON.stringify(draft) }) }).then(answer);

export async function reportUpload(job: string, uploads: Upload[]) {
  const body = new FormData();
  for (const u of uploads) body.append(u.path, u.file, u.file.name);
  const res = await fetch(`/api/report/${job}/upload`, { method: "POST", body });
  if (!res.ok) throw Object.assign(new Error(`the server answered ${res.status}`), { status: res.status });
}

/** Follows one step of a report job from where it started; stop with the returned function. */
export function followStep(job: string, from: number, onEvent: (e: JobEvent) => void): () => void {
  const source = new EventSource(`/api/jobs/${job}/events?from=${from}`);
  source.onmessage = m => {
    const e = JSON.parse(m.data) as JobEvent;
    onEvent(e);
    if (e.t === "finished") source.close();
  };
  return () => source.close();
}
