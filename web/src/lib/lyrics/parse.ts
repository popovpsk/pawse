export type Word = {
  startMs: number;
  endMs: number | null;
  start: number;
  end: number;
};

export type Backing = {
  text: string;
  words: Word[];
};

export type LyricLine = {
  timeMs: number | null;
  text: string;
  words: Word[];
  background: Backing | null;
};

export type Lyrics = {
  synced: boolean;
  lines: LyricLine[];
};

const BACKING_TAG = "bg:";
const BYTE_ORDER_MARK = "\ufeff";
const U32_MAX = 0xffffffff;
const DIGITS = /^[0-9]+$/;
const LEADING_SPACE = /^[\t-\r \u0085\u00a0\u1680\u2000-\u200a\u2028\u2029\u202f\u205f\u3000]+/;
const TRAILING_SPACE = /[\t-\r \u0085\u00a0\u1680\u2000-\u200a\u2028\u2029\u202f\u205f\u3000]+$/;

export function parseLrc(raw: string): Lyrics {
  const lines: LyricLine[] = [];
  let synced = false;
  let groupStart = 0;
  let groupEnd = 0;

  const body = raw.startsWith(BYTE_ORDER_MARK) ? raw.slice(BYTE_ORDER_MARK.length) : raw;
  for (const rawLine of body.split(/[\n\r]/)) {
    let rest = rawLine;
    const times: number[] = [];
    let backing: string | null = null;

    for (let bracket = takeBracket(rest); bracket !== null; bracket = takeBracket(rest)) {
      const [inner, after] = bracket;
      const ms = parseTimeTag(inner);
      if (ms !== null) {
        times.push(ms);
      } else {
        const tag = trimStart(inner);
        if (tag.startsWith(BACKING_TAG)) backing = tag.slice(BACKING_TAG.length);
      }
      rest = after;
    }

    if (backing !== null && times.length === 0) {
      const [text, words] = splitEnhanced(backing);
      const kept = groupEnd - groupStart === 1 ? words : [];
      for (let i = groupStart; i < groupEnd; i++) addBacking(lines[i], text, kept);
      continue;
    }

    const [text, words] = splitEnhanced(rest);

    if (times.length === 0) {
      if (text === "") continue;
      groupStart = lines.length;
      groupEnd = lines.length + 1;
      lines.push({ timeMs: null, text, words: [], background: null });
    } else {
      synced = true;
      const kept = times.length === 1 ? words : [];
      groupStart = lines.length;
      groupEnd = lines.length + times.length;
      for (const ms of times) {
        lines.push({ timeMs: ms, text, words: kept.map((w) => ({ ...w })), background: null });
      }
    }
  }

  if (!synced) return { synced, lines };
  const timed = lines.filter((line) => line.timeMs !== null);
  timed.sort((a, b) => (a.timeMs ?? 0) - (b.timeMs ?? 0));
  return { synced, lines: timed };
}

function addBacking(line: LyricLine, text: string, words: Word[]) {
  if (text === "") return;
  if (line.text === "" && line.background === null) {
    line.text = text;
    line.words = words.map((w) => ({ ...w }));
    return;
  }
  const backing = line.background ?? { text: "", words: [] };
  line.background = backing;
  let shift = 0;
  if (backing.text !== "") {
    backing.text += " ";
    shift = backing.text.length;
  }
  backing.text += text;
  for (const w of words) backing.words.push({ ...w, start: w.start + shift, end: w.end + shift });
}

function takeBracket(s: string): [string, string] | null {
  const start = s.indexOf("[");
  if (start < 0) return null;
  const end = s.indexOf("]", start);
  if (end < 0) return null;
  if (trim(s.slice(0, start)) !== "") return null;
  return [s.slice(start + 1, end), s.slice(end + 1)];
}

export function parseTimeTag(inner: string): number | null {
  const colon = inner.indexOf(":");
  if (colon < 0) return null;
  const minStr = inner.slice(0, colon);
  if (!DIGITS.test(minStr)) return null;
  const rest = inner.slice(colon + 1);
  const dot = rest.indexOf(".");
  const secStr = dot < 0 ? rest : rest.slice(0, dot);
  if (!DIGITS.test(secStr)) return null;

  let fracMs = 0;
  if (dot >= 0) {
    const frac = rest.slice(dot + 1);
    if (!DIGITS.test(frac) || frac.length === 1) return null;
    fracMs = frac.length === 3 ? Number(frac) : Number(frac.slice(0, 2)) * 10;
  }

  const total = Number(minStr) * 60_000 + Number(secStr) * 1_000 + fracMs;
  return total <= U32_MAX ? total : null;
}

function splitEnhanced(raw: string): [string, Word[]] {
  let text = "";
  const words: Word[] = [];
  let started: number | null = null;
  let rest = raw;
  for (;;) {
    const marker = nextMarker(rest);
    let segment = marker !== null ? marker.before : rest;
    if (TRAILING_SPACE.test(text)) segment = trimStart(segment);
    const body = trim(segment);
    if (started !== null) {
      const startMs = started;
      started = null;
      if (body === "") {
        const last = words[words.length - 1];
        if (last !== undefined && last.endMs === null) last.endMs = startMs;
      } else {
        const from = text.length + segment.length - trimStart(segment).length;
        words.push({ startMs, endMs: null, start: from, end: from + body.length });
      }
    }
    text += segment;
    if (marker === null) break;
    started = marker.ms;
    rest = marker.after;
  }
  const lead = text.length - trimStart(text).length;
  for (const w of words) {
    w.start -= lead;
    w.end -= lead;
  }
  return [trim(text), words];
}

function trimStart(s: string): string {
  return s.replace(LEADING_SPACE, "");
}

function trim(s: string): string {
  return s.replace(LEADING_SPACE, "").replace(TRAILING_SPACE, "");
}

function nextMarker(s: string): { before: string; ms: number; after: string } | null {
  let from = 0;
  for (;;) {
    const open = s.indexOf("<", from);
    if (open < 0) return null;
    const close = s.indexOf(">", open);
    if (close < 0) return null;
    const ms = parseTimeTag(s.slice(open + 1, close));
    if (ms !== null) return { before: s.slice(0, open), ms, after: s.slice(close + 1) };
    from = open + 1;
  }
}
