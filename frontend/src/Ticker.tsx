import { useEffect, useState } from "react";

export type Step = "reading" | "writing" | "running" | "session" | "drawing" | "fixing" | "input" | "packing";

const MESSAGES: Record<Step, string[]> = {
  reading: ["squinting at page 2", "untangling the question numbers", "reading the fine print", "decoding the table in the middle",
    "telling questions apart from notes", "counting the sub-parts", "deciphering the scanned bits", "finding where q3 really ends",
    "skipping the instructions page", "matching figures to questions", "noticing the bonus question", "reading between the margins"],
  writing: ["sketching the main loop", "naming variables properly", "arguing with semicolons", "picking the right data structure",
    "writing the happy path first", "handling the edge cases", "making the output match the question", "tidying the indentation",
    "deciding between for and while", "writing it like a student would", "double-checking the brackets", "keeping it short"],
  running: ["compiling", "feeding it some input", "waiting for the terminal to talk back", "pressing enter",
    "watching the output scroll", "hoping for no segfault", "letting it think", "reading what it printed",
    "taking the screenshot", "lining up the prompt"],
  session: ["typing into the prompt", "waiting for the prompt to come back", "creating the tables", "inserting the rows",
    "running the query", "reading the result table", "cleaning up after itself", "typing one line at a time"],
  drawing: ["arranging boxes and arrows", "placing the entities", "drawing the relationships", "untangling crossed lines",
    "labelling the cardinality", "underlining the keys", "making room for attributes", "centring the diagram"],
  fixing: ["reading the error", "finding the line that broke", "trying that again, better", "patching the logic",
    "rerunning with the fix", "checking it prints something now", "asking the bigger model for help", "making it actually compile"],
  input: ["making up test input", "picking believable numbers", "typing what a user would type", "choosing a name for the user"],
  packing: ["laying out the pages", "tucking screenshots in", "numbering the questions", "adding the title page",
    "putting each answer on its own page", "saving the word file", "checking every screenshot made it"],
};

const SLOW: [number, string[]][] = [
  [300, ["slow today; it gives up on its own at 10 min", "the ai is really taking its time", "still waiting, nothing is broken"]],
  [120, ["still going, big file", "the ai is thinking hard about this one", "longer than usual, hang on"]],
  [40, ["this one's a long read", "taking a little longer", "almost there, probably"]],
];

const reduced = matchMedia("(prefers-reduced-motion: reduce)").matches;

const elapsed = (secs: number) => secs < 60 ? `${secs}s` : `${Math.floor(secs / 60)}m ${secs % 60}s`;

/** A line for `step` after `secs` seconds that isn't `last`. */
export function pick(step: Step, secs: number, last = ""): string {
  const pool = SLOW.find(([after]) => secs >= after)?.[1] ?? MESSAGES[step];
  const fresh = pool.filter(m => m !== last);
  return fresh[Math.floor(Math.random() * fresh.length)] ?? pool[0];
}

export function Ticker({ step, since }: { step: Step; since: number }) {
  const [now, setNow] = useState(Date.now());
  const [line, setLine] = useState(() => pick(step, 0));
  const secs = Math.max(0, Math.round((now - since) / 1000));
  useEffect(() => { setLine(l => pick(step, 0, l)) }, [step]);
  useEffect(() => {
    const tick = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(tick);
  }, []);
  useEffect(() => {
    if (reduced && secs < 40) return;
    if (secs % 3 === 0 && secs > 0) setLine(l => pick(step, secs, l));
  }, [secs, step]);
  return (
    <span className="ticker">
      <span key={line} className="ticker-line">{line}</span>
      {secs >= 40 && <span className="ticker-time"> ({elapsed(secs)})</span>}
      <span className="cursor" aria-hidden="true">▍</span>
    </span>
  );
}
