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
  instructions: string;
  base_url: string;
  model: string;
};

export const DEFAULTS: Settings = {
  style: "windows", theme: "dark", username: "user", hostname: "laptop", cwd: "lab", messy: 0, min_cols: 80, seed: 1,
  doc_title: "", doc_text: "", output: "", page_break: false, doc_theme: "current", numbering: "1(b)(i)", full_labels: false, check: false, instructions: "", base_url: "", model: "",
};

export type Numbering = "1(b)(i)" | "1(2)(1)" | "1.b.i" | "1.2.1";
export const NUMBERINGS: Numbering[] = ["1(b)(i)", "1(2)(1)", "1.b.i", "1.2.1"];

export type Look = { font: string; size: number; color: string; bold: boolean; italic: boolean; underline: boolean; center: boolean; rule: boolean; before: number };
export type DocTheme = {
  name: string; paper: "a4" | "letter"; margin: number; margin_top: number;
  font: string; size: number; line: number; gap: number;
  title: Look; headings: [Look, Look, Look]; question: Look;
  code_font: string; code_size: number; code_box: "none" | "shade" | "border";
  shots: "small" | "medium" | "full"; shots_center: boolean;
  page_numbers: boolean; labels: boolean; show_question: boolean; prefix: string;
};
export const FONTS = ["Calibri", "Cambria", "Times New Roman", "Arial", "Georgia"];
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
  | { t: "finished" }
);
export type State = "waiting" | "writing" | "running" | "fixing" | "ok" | "failed";

const shotFields = (s: Settings) => ({
  style: s.style, theme: s.theme, username: s.username, hostname: s.hostname, cwd: s.cwd,
  messy: String(s.messy), min_cols: String(s.min_cols),
});

export const previewUrl = (s: Settings) => `/api/preview?${new URLSearchParams({ ...shotFields(s), seed: String(s.seed) })}`;
export const shotUrl = (job: string, n: number) => `/api/jobs/${job}/shot/${n}`;
export const docUrl = (job: string, n: number) => `/api/jobs/${job}/doc/${n}`;
export const clearJob = (job: string) => fetch(`/api/jobs/${job}`, { method: "DELETE", keepalive: true }).catch(() => {});

export async function startJob(mode: Mode, uploads: Upload[], s: Settings, theme: DocTheme | undefined, apiKey: string): Promise<string> {
  const body = new FormData();
  const fields: Record<string, string> = {
    ...shotFields(s), doc_title: s.doc_title, doc_text: s.doc_text, output: s.output,
    page_break: String(s.page_break), doc_theme: theme ? JSON.stringify({ ...theme, numbering: s.numbering, full_labels: s.full_labels }) : s.doc_theme, check: String(s.check), instructions: s.instructions, api_key: apiKey, base_url: s.base_url, model: s.model,
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
