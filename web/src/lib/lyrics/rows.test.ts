import { test } from "node:test";
import assert from "node:assert/strict";
import type { Lyrics, Word } from "./parse.ts";
import {
  ACTIVE_TOLERANCE_MS,
  INTERLUDE_MIN_MS,
  INTERLUDE_TEXT,
  activeRow,
  buildRows,
  fillSpan,
  progress,
  sungUntil,
  wordPieces,
  type LyricRow,
} from "./rows.ts";

function synced(lines: [number, string][]): Lyrics {
  return {
    synced: true,
    lines: lines.map(([timeMs, text]) => ({ timeMs, text, words: [], background: null })),
  };
}

function word(startMs: number, endMs: number | null, start: number, end: number): Word {
  return { startMs, endMs, start, end };
}

function row(timeMs: number, text: string): LyricRow {
  return { text, words: [], background: null, timeMs, kind: "lyric" };
}

const halo = synced([
  [188_260, "How you're plannin' to go about makin' your amends"],
  [194_830, ""],
  [196_980, "To the dead"],
  [201_660, ""],
  [204_460, "To the dead"],
  [209_170, ""],
  [212_850, "With your halo slippin' down"],
]);

test("between lines only blank lines become interludes", () => {
  const rows = buildRows(halo, 240_000);
  assert.deepEqual(
    rows.map((r) => r.kind),
    ["interlude", "lyric", "lyric", "lyric", "lyric", "lyric", "interlude", "lyric"],
  );
  assert.equal(rows[6].timeMs, 209_170);
  assert.equal(rows[6].text, INTERLUDE_TEXT);
});

test("a late first line gets an intro interlude", () => {
  const rows = buildRows(synced([[5_000, "one"], [9_000, "two"]]), null);
  assert.equal(rows.length, 3);
  assert.equal(rows[0].kind, "interlude");
  assert.deepEqual(fillSpan(rows, 0, null), [0, 5_000]);
  assert.equal(activeRow(rows, 0), 0);
  assert.equal(activeRow(rows, 5_000), 1);
  assert.equal(buildRows(synced([[INTERLUDE_MIN_MS - 1, "one"]]), null).length, 1);
});

test("a blank line inside the intro folds into it", () => {
  const rows = buildRows(synced([[4_000, ""], [10_000, "one"]]), null);
  assert.equal(rows.length, 2);
  assert.deepEqual(fillSpan(rows, 0, null), [0, 10_000]);
});

test("a blank line needs at least the minimum", () => {
  const short = buildRows(synced([[0, "one"], [1_000, ""], [1_000 + INTERLUDE_MIN_MS - 1, "two"]]), null);
  assert.equal(short[1].kind, "lyric");
  assert.equal(short[1].text, "");
  const long = buildRows(synced([[0, "one"], [1_000, ""], [1_000 + INTERLUDE_MIN_MS, "two"]]), null);
  assert.equal(long[1].kind, "interlude");
  assert.equal(buildRows(synced([[0, "one"], [60_000, "two"]]), 120_000).length, 2);
});

test("back to back blank lines collapse into one interlude", () => {
  const rows = buildRows(synced([[0, "a"], [4_000, ""], [8_000, ""], [12_000, "b"]]), null);
  assert.equal(rows.length, 3);
  assert.equal(rows[1].timeMs, 4_000);
  assert.equal(activeRow(rows, 9_000), 1);
  assert.deepEqual(fillSpan(rows, 1, null), [4_000, 8_000]);
});

test("trailing blank lines follow the track length", () => {
  assert.equal(buildRows(synced([[0, "one"], [5_000, ""]]), 20_000)[1].kind, "interlude");
  assert.equal(buildRows(synced([[0, "one"], [30_000, ""]]), 20_000)[1].kind, "lyric");
  assert.equal(buildRows(synced([[0, "one"], [2_000, ""]]), null)[1].kind, "lyric");
});

test("spans keep the lead off lyric rows only", () => {
  const rows = buildRows(halo, 240_000);
  assert.deepEqual(fillSpan(rows, 1, 240_000), [188_260, Math.floor(6_570 * 0.92)]);
  assert.deepEqual(fillSpan(rows, 6, 240_000), [209_170, 3_680]);
  const last = buildRows(synced([[0, "one"]]), 4_000);
  assert.deepEqual(fillSpan(last, 0, 4_000), [0, 3_680]);
  assert.equal(fillSpan(last, 0, null), null);
  assert.equal(fillSpan(last, 9, 4_000), null);
  assert.equal(fillSpan(buildRows(synced([[4_000, "a"], [4_000, "b"]]), 9_000), 1, 9_000), null);
});

test("the fill starts at zero and ends before the hand over", () => {
  const rows = buildRows(synced([[1_000, "one"], [3_000, "two"]]), null);
  const span = fillSpan(rows, 0, null);
  assert.ok(span !== null);
  const [start, length] = span;
  assert.equal(activeRow(rows, 1_000 - ACTIVE_TOLERANCE_MS), 0);
  assert.equal(progress(1_000, start, length), 0);
  assert.ok(progress(start + length - 1, start, length) < 1);
  assert.equal(progress(start + length, start, length), 1);
  assert.equal(activeRow(rows, 3_000 - ACTIVE_TOLERANCE_MS - 1), 0);
  assert.equal(activeRow(rows, 3_000 - ACTIVE_TOLERANCE_MS), 1);
  assert.equal(progress(0, 0, 0), 1);
});

test("rows go active a touch before their timestamp", () => {
  const rows = [row(1_000, "one"), row(5_000, "two")];
  assert.equal(activeRow(rows, 0), null);
  assert.equal(activeRow([], 10_000), null);
  assert.equal(activeRow(rows, 1_000 - ACTIVE_TOLERANCE_MS - 1), null);
  assert.equal(activeRow(rows, 1_000 - ACTIVE_TOLERANCE_MS), 0);
  assert.equal(activeRow(rows, 4_000), 0);
  assert.equal(activeRow(rows, 900_000), 1);
  assert.equal(activeRow([{ ...row(0, "plain"), timeMs: null }], 10_000), null);
});

test("word pieces sweep from the previous word and hold between words", () => {
  const text = "first word, two";
  const words = [word(1_000, 2_000, 0, 5), word(3_000, 3_500, 6, 11)];
  const at = (now: number) => wordPieces(text, words, null, now).map((p) => [p.text, p.fill]);
  assert.deepEqual(at(0), [
    ["first", 0],
    [" word,", 0],
    [" two", 0],
  ]);
  assert.deepEqual(at(1_500), [
    ["first", 0.5],
    [" word,", 0],
    [" two", 0],
  ]);
  assert.deepEqual(at(2_400), [
    ["first", 1],
    [" word,", 0],
    [" two", 0],
  ]);
  assert.deepEqual(at(3_250), [
    ["first", 1],
    [" word,", 0.5],
    [" two", 0],
  ]);
  assert.deepEqual(at(3_500), [
    ["first", 1],
    [" word,", 1],
    [" two", 1],
  ]);
});

test("missing ends borrow the next start, the line end or the open length", () => {
  const words = [word(1_000, null, 0, 3), word(2_000, null, 4, 7)];
  assert.deepEqual(
    wordPieces("one two", words, 4_000, 3_000).map((p) => p.fill),
    [1, 0.5],
  );
  assert.equal(wordPieces("x", [word(5_000, null, 0, 1)], 4_000, 5_300)[0].fill, 0.5);
  assert.equal(wordPieces("x", [word(1_000, 1_000, 0, 1)], null, 999)[0].fill, 0);
  assert.equal(wordPieces("x", [word(1_000, 1_000, 0, 1)], null, 1_000)[0].fill, 1);
});

test("overlapping singers never pull the fill back", () => {
  const words = [word(2_000, 4_000, 0, 5), word(1_000, 3_000, 6, 10)];
  assert.deepEqual(
    wordPieces("hello you!", words, null, 2_500).map((p) => p.fill),
    [1, 0.75],
  );
});

test("a row is sung until its last main or backing word ends", () => {
  const line = row(1_000, "one two");
  assert.equal(sungUntil(line, 2_000), null);
  line.words = [word(1_000, 1_500, 0, 3), word(1_600, null, 4, 7)];
  assert.equal(sungUntil(line, 2_000), 2_000);
  line.background = { text: "(oh oh)", words: [word(1_200, 2_400, 0, 3), word(2_500, null, 4, 6)] };
  assert.equal(sungUntil(line, 2_000), 3_100);
});
