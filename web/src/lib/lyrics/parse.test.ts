import { test } from "node:test";
import assert from "node:assert/strict";
import { parseLrc, type LyricLine, type Word } from "./parse.ts";

function line(timeMs: number | null, text: string): LyricLine {
  return { timeMs, text, words: [], background: null };
}

function word(startMs: number, endMs: number | null, start: number, end: number): Word {
  return { startMs, endMs, start, end };
}

function spelled(text: string, words: Word[]): string[] {
  return words.map((w) => text.slice(w.start, w.end));
}

test("a multi-timestamp line duplicates its text", () => {
  const parsed = parseLrc("[00:12.00][00:45.30]hello");
  assert.equal(parsed.synced, true);
  assert.deepEqual(parsed.lines, [line(12_000, "hello"), line(45_300, "hello")]);
});

test("a byte order mark does not hide the first line", () => {
  assert.deepEqual(
    parseLrc("\ufeff[00:01.00]first\n[00:02.00]second").lines.map((l) => l.text),
    ["first", "second"],
  );
});

test("whitespace is Rust's, not that of JS trim", () => {
  assert.deepEqual(
    parseLrc("[00:01.00]a\n\ufeff[00:02.00]b").lines.map((l) => l.text),
    ["a"],
  );
  assert.equal(parseLrc("[00:01.00]\u0085x\u0085").lines[0].text, "x");
});

test("metadata tags are ignored", () => {
  const parsed = parseLrc(
    "[ti:Song]\n[ar:Artist]\n[al:Album]\n[by:Someone]\n[offset:500]\n[length:03:21]\n[00:01.00]first",
  );
  assert.equal(parsed.synced, true);
  assert.deepEqual(parsed.lines, [line(1_000, "first")]);
});

test("plain lyrics have no timestamps", () => {
  const parsed = parseLrc("first line\nsecond line\nthird line");
  assert.equal(parsed.synced, false);
  assert.deepEqual(
    parsed.lines.map((l) => [l.timeMs, l.text]),
    [
      [null, "first line"],
      [null, "second line"],
      [null, "third line"],
    ],
  );
});

test("carriage returns split lines", () => {
  assert.deepEqual(
    parseLrc("first\rsecond\r\nthird").lines.map((l) => l.text),
    ["first", "second", "third"],
  );
});

test("blank plain lines are dropped, blank timed lines kept", () => {
  assert.deepEqual(
    parseLrc("first\n\n   \nsecond").lines.map((l) => l.text),
    ["first", "second"],
  );
  const synced = parseLrc("[00:01.00]a\n[00:02.00]\n[00:03.00]b");
  assert.deepEqual(synced.lines[1], line(2_000, ""));
  assert.equal(synced.lines.length, 3);
});

test("fractions convert to milliseconds", () => {
  for (const [raw, ms] of [
    ["[01:02.50]x", 62_500],
    ["[01:02.500]x", 62_500],
    ["[01:02]x", 62_000],
    ["[00:00.123]x", 123],
    ["[00:01.2345]x", 1_230],
  ] as const) {
    assert.equal(parseLrc(raw).lines[0].timeMs, ms, raw);
  }
  assert.equal(parseLrc("[00:01.5]x").synced, false);
});

test("lines are sorted by time and untimed lines dropped", () => {
  const parsed = parseLrc("[00:30.00]c\nuntimed\n[00:10.00]a\n[00:20.00]b");
  assert.deepEqual(
    parsed.lines.map((l) => [l.timeMs, l.text]),
    [
      [10_000, "a"],
      [20_000, "b"],
      [30_000, "c"],
    ],
  );
});

test("internal whitespace is preserved", () => {
  assert.equal(parseLrc("[00:01.00]  hello   world  ").lines[0].text, "hello   world");
});

test("an overflowing timestamp is not a time", () => {
  const parsed = parseLrc("[99999:00.00]x\n[00:01.00]ok");
  assert.deepEqual(parsed.lines, [line(1_000, "ok")]);
});

test("tags count only at the start of a line", () => {
  assert.equal(parseLrc("intro [00:01.00]x").lines[0].text, "intro [00:01.00]x");
  assert.equal(parseLrc("  [00:01.00]x").lines[0].timeMs, 1_000);
});

test("enhanced lines keep their words", () => {
  const parsed = parseLrc("[00:01.00]<00:01.00>Hello <00:01.60>world<00:02.20>\n[00:03.00]plain");
  assert.equal(parsed.lines[0].text, "Hello world");
  assert.deepEqual(parsed.lines[0].words, [word(1_000, null, 0, 5), word(1_600, 2_200, 6, 11)]);
  assert.deepEqual(parsed.lines[1].words, []);
});

test("words are dropped where they cannot belong to one line", () => {
  const repeated = parseLrc("[00:01.00][00:09.00]<00:01.00>chorus");
  assert.equal(repeated.lines.length, 2);
  assert.ok(repeated.lines.every((l) => l.words.length === 0 && l.text === "chorus"));
  const plain = parseLrc("<00:01.00>no line stamp");
  assert.equal(plain.synced, false);
  assert.deepEqual(plain.lines[0], line(null, "no line stamp"));
});

test("markers become words and leave the text clean", () => {
  const [l] = parseLrc("[00:00.00] <00:01.00>Hello <00:01.50>world<00:02.00> ").lines;
  assert.equal(l.text, "Hello world");
  assert.deepEqual(l.words, [word(1_000, null, 0, 5), word(1_500, 2_000, 6, 11)]);
});

test("spaces on both sides of a marker stay one space", () => {
  const [l] = parseLrc("[00:00.00]<00:12.04> When <00:12.16> the <00:12.82> truth").lines;
  assert.equal(l.text, "When the truth");
  assert.deepEqual(spelled(l.text, l.words), ["When", "the", "truth"]);
  assert.equal(parseLrc("[00:00.00]one  <00:01.00>  two").lines[0].text, "one  two");
});

test("a marker before blank text ends the previous word", () => {
  const [l] = parseLrc("[00:00.00]<00:01.00>one<00:01.40> <00:01.50>two").lines;
  assert.equal(l.text, "one two");
  assert.equal(l.words[0].endMs, 1_400);
  assert.deepEqual(l.words[1], word(1_500, null, 4, 7));
});

test("text that only looks like a marker is kept", () => {
  const [l] = parseLrc("[00:00.00]<3 you <00:02.00>always").lines;
  assert.equal(l.text, "<3 you always");
  assert.deepEqual(spelled(l.text, l.words), ["always"]);
  assert.equal(parseLrc("[00:00.00]a < b > c").lines[0].text, "a < b > c");
  assert.equal(parseLrc("[00:00.00]open <00:01.00").lines[0].text, "open <00:01.00");
});

test("multibyte words keep their positions", () => {
  const [l] = parseLrc("[00:00.00]<00:01.00>Привет, <00:01.50>мир <00:02.00>눈을 😀<00:03.00>x").lines;
  assert.equal(l.text, "Привет, мир 눈을 😀x");
  assert.deepEqual(spelled(l.text, l.words), ["Привет,", "мир", "눈을 😀", "x"]);
});

test("a bg line is backing vocals of the line before", () => {
  const parsed = parseLrc(
    "[00:01.00]<00:01.00>Hello\n[bg: <00:01.50>(hello <00:02.00>there)]\n[bg:(again)]\n[00:03.00]Next",
  );
  assert.equal(parsed.lines.length, 2);
  const backing = parsed.lines[0].background;
  assert.ok(backing !== null);
  assert.equal(backing.text, "(hello there) (again)");
  assert.deepEqual(spelled(backing.text, backing.words), ["(hello", "there)"]);
  assert.equal(parsed.lines[1].background, null);
  assert.equal(parseLrc("[bg: orphan]\n[00:01.00]a").lines.length, 1);
});

test("a repeated line gets its backing on every copy without words", () => {
  const parsed = parseLrc("[00:10.00][01:10.00]Chorus\n[bg: <00:10.50>(ooh)]");
  assert.equal(parsed.lines.length, 2);
  for (const l of parsed.lines) {
    assert.deepEqual(l.background, { text: "(ooh)", words: [] });
  }
});

test("backing vocals after a blank line become its text", () => {
  const [l] = parseLrc("[00:20.00]\n[bg: <00:20.10>(ahh)]\n[bg: (oh)]").lines;
  assert.equal(l.text, "(ahh)");
  assert.equal(l.words.length, 1);
  assert.equal(l.background?.text, "(oh)");
});

test("what the Rust writer emits parses back", () => {
  const raw =
    "[00:01.234]<00:01.234>Hello,<00:01.500> <00:01.600>world <00:02.001>(yeah)<00:02.999>\n" +
    "[bg: <00:01.300>(ooh<00:01.700> <00:01.800>ahh)]\n" +
    "[01:10.000]<01:10.000>be<01:10.200><01:10.200>ing<01:10.500>";
  const parsed = parseLrc(raw);
  assert.equal(parsed.lines[0].text, "Hello, world (yeah)");
  assert.deepEqual(parsed.lines[0].words, [
    word(1_234, 1_500, 0, 6),
    word(1_600, null, 7, 12),
    word(2_001, 2_999, 13, 19),
  ]);
  assert.deepEqual(parsed.lines[0].background?.words, [
    word(1_300, 1_700, 0, 4),
    word(1_800, null, 5, 9),
  ]);
  assert.deepEqual(parsed.lines[1].words, [word(70_000, 70_200, 0, 2), word(70_200, 70_500, 2, 5)]);
});
