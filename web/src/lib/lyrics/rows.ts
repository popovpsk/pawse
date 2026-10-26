import type { Backing, Lyrics, Word } from "./parse";

export const ACTIVE_TOLERANCE_MS = 50;
export const INTERLUDE_MIN_MS = 3_000;
export const INTERLUDE_TEXT = "♪ ♪ ♪";
const FILL_LEAD = 0.92;
const OPEN_WORD_MS = 600;

export type RowKind = "lyric" | "interlude";

export type LyricRow = {
  text: string;
  words: Word[];
  background: Backing | null;
  timeMs: number | null;
  kind: RowKind;
};

export type Piece = {
  text: string;
  fill: number;
};

export function buildRows(parsed: Lyrics, durationMs: number | null): LyricRow[] {
  const base: LyricRow[] = parsed.lines.map((line) => ({
    text: line.text,
    words: line.words,
    background: line.background,
    timeMs: line.timeMs,
    kind: "lyric",
  }));
  if (!parsed.synced || base.length === 0) return base;

  const out: LyricRow[] = [];
  if ((base[0].timeMs ?? 0) >= INTERLUDE_MIN_MS) {
    out.push({ text: INTERLUDE_TEXT, words: [], background: null, timeMs: 0, kind: "interlude" });
  }
  base.forEach((row, ix) => {
    const next = base[ix + 1]?.timeMs ?? durationMs;
    if (row.text === "" && next !== null && next - (row.timeMs ?? 0) >= INTERLUDE_MIN_MS) {
      if (out[out.length - 1]?.kind === "interlude") return;
      row.text = INTERLUDE_TEXT;
      row.kind = "interlude";
    }
    out.push(row);
  });
  return out;
}

export function activeRow(rows: LyricRow[], posMs: number): number | null {
  const pos = posMs + ACTIVE_TOLERANCE_MS;
  let found: number | null = null;
  let lo = 0;
  let hi = rows.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    const t = rows[mid].timeMs;
    if (t !== null && t <= pos) {
      found = mid;
      lo = mid + 1;
    } else {
      hi = mid;
    }
  }
  return found;
}

export function rowEnd(rows: LyricRow[], ix: number, durationMs: number | null): number | null {
  return rows[ix + 1]?.timeMs ?? durationMs;
}

export function fillSpan(
  rows: LyricRow[],
  ix: number,
  durationMs: number | null,
): [number, number] | null {
  const row = rows[ix];
  if (row?.timeMs == null) return null;
  const end = rowEnd(rows, ix, durationMs);
  if (end === null) return null;
  const span = Math.max(0, end - row.timeMs);
  if (span === 0) return null;
  return [row.timeMs, row.kind === "interlude" ? span : Math.max(1, Math.floor(span * FILL_LEAD))];
}

export function progress(nowMs: number, startMs: number, spanMs: number): number {
  if (spanMs <= 0) return 1;
  return Math.min(1, Math.max(0, (nowMs - startMs) / spanMs));
}

function wordEnd(words: Word[], word: Word, lineEndMs: number | null): number {
  let nextStart: number | null = null;
  for (const other of words) {
    if (other.startMs > word.startMs && (nextStart === null || other.startMs < nextStart)) {
      nextStart = other.startMs;
    }
  }
  const lineEnd = lineEndMs !== null && lineEndMs > word.startMs ? lineEndMs : null;
  const end = word.endMs ?? nextStart ?? lineEnd ?? word.startMs + OPEN_WORD_MS;
  return Math.max(end, word.startMs);
}

export function sungUntil(row: LyricRow, lineEndMs: number | null): number | null {
  let until: number | null = null;
  for (const words of [row.words, row.background?.words ?? []]) {
    for (const word of words) {
      const end = wordEnd(words, word, lineEndMs);
      if (until === null || end > until) until = end;
    }
  }
  return until;
}

export function wordPieces(
  text: string,
  words: Word[],
  lineEndMs: number | null,
  nowMs: number,
): Piece[] {
  const pieces: Piece[] = [];
  let from = 0;
  let started = -1;
  let done = true;
  words.forEach((word, ix) => {
    const end = wordEnd(words, word, lineEndMs);
    const fill =
      nowMs >= end ? 1 : nowMs <= word.startMs ? 0 : (nowMs - word.startMs) / (end - word.startMs);
    if (fill > 0) started = ix;
    if (fill < 1) done = false;
    pieces.push({ text: text.slice(from, word.end), fill });
    from = word.end;
  });
  for (let ix = 0; ix < started; ix++) pieces[ix].fill = 1;
  if (from < text.length) pieces.push({ text: text.slice(from), fill: done ? 1 : 0 });
  return pieces;
}
