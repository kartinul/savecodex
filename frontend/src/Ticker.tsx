import { useEffect, useState } from "react";

export type Step = "reading" | "writing" | "running" | "session" | "drawing" | "fixing" | "input" | "packing";

const MESSAGES: Record<Step, string[]> = {
  reading: [
    "squinting at page 2", "finding the tasks hiding in plain sight", "skipping the cover page, nobody reads those",
    "decoding the table, send help", "reading the fine print so you don't have to", "counting sub-parts… why are there so many",
    "telling tasks apart from notes", "flipping pages like a speedrunner", "the scan is blurry, putting on glasses",
    "found an (a), (b), (c)… oh no there's a (d)", "reading between the lines (literally)", "matching figures to text",
    "ok which language do they even want", "copying the sample input, carefully", "page 3 is just a logo, cool cool",
    "untangling this numbering", "speedreading but make it accurate", "this table has too many columns",
    "highlighting the important bits mentally", "where does this task even end", "ok that's a sneaky extra part",
    "reading the whole thing twice, just in case", "following the arrows in the figure", "noting every 'also' and 'additionally'",
    "turning walls of text into a list", "spotting the expected output", "reading the footnotes too",
    "connecting part 5 back to part 2", "ignoring the decorative border", "checking for tasks in the margins",
    "zooming in on the tiny font", "translating the formal wording", "reading the headers, then the body",
    "figuring out what 'the above' means", "lining up the numbered steps", "noting the input format",
    "this page is mostly whitespace, nice", "carefully not skipping anything", "finding the one task hidden in a paragraph",
    "reading the sample run", "checking if that's one task or two", "making a mental checklist",
    "mapping out the whole document", "reading the schema column by column", "spotting the 'write a program' parts",
    "unrolling the nested parts", "reading it like a contract", "paying attention to the 'must's",
    "one more page, almost done", "reading the last page first, rebel"],
  writing: [
    "arguing with semicolons", "naming variables better than x and y", "writing the happy path first",
    "off-by-one errors, my old friend", "closing every bracket it opened", "for or while… for. definitely for",
    "making it compile in my head first", "no clever one-liners, keeping it normal", "writing code that reads like english (ish)",
    "tidying the indentation", "remembering to free the memory this time", "making the output match exactly",
    "main goes at the bottom, as is tradition", "double-checking the loop bounds", "adding a prompt so it doesn't look rude",
    "counting brackets on my fingers", "keeping it short, nobody likes essays", "picking a data structure that fits",
    "writing the base case first", "handling the empty input case", "thinking about negative numbers",
    "naming the function something sensible", "deleting a comment that said 'todo'", "reading the task one more time",
    "keeping it simple on purpose", "choosing between array and list", "making sure every path returns",
    "spacing things out so it breathes", "avoiding the global variables", "checking the types line up",
    "writing it the boring way, the reliable way", "laying out the input parsing", "planning the output format",
    "making the loop actually end", "adding the missing return", "putting the logic in its own function",
    "matching the style of the task", "using the language it asked for", "giving the program a proper exit",
    "picking test values that show it works", "making sure it handles zero", "keeping the edge cases in mind",
    "one function, one job", "writing the query step by step", "drafting, then tidying",
    "checking the spelling in the prompts", "lining up the print statements", "taking the scenic route through the logic",
    "almost done writing, final bracket"],
  running: [
    "compiling… please no errors please no errors", "pressing enter with confidence", "hoping for no segfault, fingers crossed",
    "the terminal is thinking, let it cook", "feeding it numbers, it's hungry", "waiting for it to talk back",
    "taking the screenshot, say cheese", "checking it didn't loop forever", "typing the input like a real human",
    "framing the terminal nicely", "warming up the compiler", "reading what it printed",
    "the compiler is reading my code, nervous", "zero warnings so far, nice", "letting the program do its thing",
    "watching the output scroll by", "waiting for the last line", "counting the output lines",
    "making sure it exits cleanly", "checking the exit code", "giving it a fresh terminal",
    "clearing the screen for a clean shot", "lining up the prompt", "running it for real this time",
    "building, linking, launching", "it's alive", "waiting for the program to finish its sentence",
    "catching every printed line", "running it once, carefully", "checking the output makes sense",
    "pressing enter after each input", "waiting for it to ask for input", "answering its questions",
    "watching the cursor blink", "the program is working on it", "grabbing the output before it scrolls away",
    "cropping the screenshot nicely", "making the window just the right size", "running the build",
    "it printed something, good sign"],
  session: [
    "inserting rows one by one, patience", "waiting for mysql to wake up", "joining the tables",
    "reading the result table, looks clean", "dropping tables (the right ones)", "aligning columns, satisfying",
    "running the query", "counting rows, all present", "creating the tables", "typing into the prompt",
    "waiting for the prompt to come back", "pressing enter after every line", "checking the row count",
    "selecting everything, carefully", "filtering with a where clause", "sorting the results",
    "grouping things up", "committing the changes", "typing one command at a time",
    "making sure the table exists first", "watching the rows come back", "reading the column headers",
    "cleaning up after itself", "starting a fresh session", "checking the query didn't error",
    "the database is cooperating today", "describing the table", "updating a few rows",
    "deleting exactly what it should", "checking the foreign keys", "printing the final table",
    "running the next command", "piping things together", "listing the files", "checking the permissions",
    "echoing it back to be sure"],
  drawing: [
    "untangling crossed lines", "underlining the keys", "placing entities with care",
    "making room for the attributes", "centring the diagram", "redrawing that one messy corner",
    "drawing the boxes", "connecting the relationships", "adding the diamonds", "labelling the lines",
    "spreading things out so nothing overlaps", "picking a tidy layout", "putting the main entity in the middle",
    "drawing the attribute bubbles", "adding the cardinality", "lining up the labels",
    "making the arrows point the right way", "giving everything some breathing room", "rendering the picture",
    "checking every entity made it", "moving one box a little to the left", "keeping the lines straight",
    "drawing the weak entity carefully", "making it readable at a glance", "connecting the foreign keys",
    "almost a work of art", "zooming out to check the whole thing", "sketching it, then cleaning it up"],
  fixing: [
    "reading the error, it's being dramatic", "finding the line that broke, who did this", "trying again but smarter this time",
    "it's not a bug it's a feature (it's a bug)", "adding the missing semicolon, classic", "the include was missing",
    "rethinking that loop", "asking the bigger model for help", "comparing with what it actually wanted",
    "fixed it. probably. checking", "cleaning up the last attempt's mess", "stack trace said what now",
    "reading the error message slowly", "it was a typo, it's always a typo", "trying a different approach",
    "checking the input it expected", "fixing the off-by-one", "making it print something this time",
    "patching the logic", "rerunning with the fix", "second try's the charm",
    "looking at it from another angle", "simplifying the tricky part", "fixing the format of the output",
    "adding the bit it forgot", "removing the bit that broke it", "testing the edge case that failed",
    "this time for sure", "reading the docs, just this once", "undoing the clever part",
    "making it actually compile", "giving it another go", "almost fixed, one more run"],
  input: [
    "making up test input", "picking numbers that look legit", "typing like a real user would",
    "keeping the input small", "guessing what it'll ask for", "choosing numbers that prove a point",
    "lining up answers for each prompt", "deciding how many items, five feels right", "picking a sensible name for the test user",
    "reading the prompts it prints", "avoiding numbers that break things", "picking a mix of small and big values",
    "making sure the input matches the format", "choosing a realistic example", "not too many, not too few",
    "typing the numbers in order", "making the sample look natural", "picking values that show off the logic",
    "checking it wants spaces, not commas", "adding one tricky case", "thinking like a user",
    "putting each answer on its own line"],
  packing: [
    "numbering everything neatly", "adding the title", "one answer per page", "saving the word file",
    "checking every screenshot made it", "zipping up the docx, almost there", "laying out the pages",
    "putting code above output", "adding the headings", "sizing the images to fit", "keeping everything in order",
    "adding the text under the title", "lining up the screenshots", "making the code readable",
    "adding page breaks", "double-checking the headings", "putting the pieces together",
    "giving it a final look", "making sure nothing got cut off", "almost ready to download",
    "wrapping it up", "tidying the spacing", "last page, last screenshot"],
};

/** Mixed in now and then once a step runs long, so it's clear nothing is stuck. */
const SLOW: [number, string[]][] = [
  [300, ["ok this is taking a while, it gives up at 10 min", "still alive, just slow today", "not stuck, just dramatic",
    "the model is clearly overthinking this", "patience level: expert", "this is a marathon, not a sprint",
    "still working, promise", "good things take time, apparently", "it's thinking really, really hard"]],
  [120, ["still going, big file", "the ai is thinking hard about this one", "longer than usual, hang tight",
    "lots to read here", "worth the wait (hopefully)", "still on it", "quite a long one this",
    "getting there, slowly but surely", "maybe grab a coffee", "the model is taking its time"]],
  [40, ["this one's a long read", "taking its sweet time", "almost there, probably", "a bit more patience",
    "the ai is still on it", "still working", "longer than a quick one", "hang on, it's thinking",
    "no rush, it's doing its best"]],
];

const SPIN = "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏";
const HOLD = 4000;
const reduced = matchMedia("(prefers-reduced-motion: reduce)").matches;
const elapsed = (secs: number) => secs < 60 ? `${secs}s` : `${Math.floor(secs / 60)}m ${secs % 60}s`;

/** A random line for `step` after `secs` seconds, none of `recent`. */
export function pick(step: Step, secs: number, recent: string[] = []): string {
  const slow = SLOW.find(([after]) => secs >= after)?.[1];
  const pool = slow && Math.random() < 0.25 ? slow : MESSAGES[step];
  const fresh = pool.filter(m => !recent.includes(m));
  return fresh[Math.floor(Math.random() * fresh.length)] ?? pool[0];
}

/** A spinner and one status line that types out, stays for 4s, then is replaced by a new one. */
export function Ticker({ step, since }: { step: Step; since: number }) {
  const [recent, setRecent] = useState<string[]>(() => [pick(step, 0)]);
  const [typed, setTyped] = useState(reduced ? Infinity : 0);
  const [frame, setFrame] = useState(0);
  const line = recent[recent.length - 1];
  const secs = Math.max(0, Math.round((Date.now() - since) / 1000));

  useEffect(() => { const t = setInterval(() => setFrame(f => f + 1), 80); return () => clearInterval(t) }, []);
  useEffect(() => {
    const next = () => {
      setRecent(r => [...r.slice(-24), pick(step, Math.round((Date.now() - since) / 1000), r)]);
      setTyped(reduced ? Infinity : 0);
    };
    const t = typed < line.length ? setTimeout(() => setTyped(n => n + 1), 28 + Math.random() * 40) : setTimeout(next, HOLD);
    return () => clearTimeout(t);
  }, [typed, line, step, since]);

  return (
    <span className="ticker">
      <span className="spin" aria-hidden="true">{SPIN[frame % SPIN.length]}</span>
      <span className="ticker-line">{line.slice(0, typed)}<span className="caret" aria-hidden="true" /></span>
      {secs >= 40 && <span className="ticker-time">({elapsed(secs)})</span>}
    </span>
  );
}
