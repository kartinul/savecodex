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
  check: boolean;
  instructions: string;
  base_url: string;
  model: string;
};

export const DEFAULTS: Settings = {
  style: "windows", theme: "dark", username: "user", hostname: "laptop", cwd: "lab", messy: 0, min_cols: 80, seed: 1,
  doc_title: "", doc_text: "", output: "", page_break: false, check: false, instructions: "", base_url: "", model: "",
};

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

export async function startJob(mode: Mode, uploads: Upload[], s: Settings, apiKey: string): Promise<string> {
  const body = new FormData();
  const fields: Record<string, string> = {
    ...shotFields(s), doc_title: s.doc_title, doc_text: s.doc_text, output: s.output,
    page_break: String(s.page_break), check: String(s.check), instructions: s.instructions, api_key: apiKey, base_url: s.base_url, model: s.model,
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
