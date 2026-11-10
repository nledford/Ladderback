#!/usr/bin/env node
// Writes formatting.json, the expectations every host that formats numbers and
// instants has to meet (see mod.rs). The strings are the ones Node's own `Intl`
// writes, an oracle independent of ICU4X, apart from the one place CLDR and V8
// differ (below). Run it from anywhere: `node generate.mjs`. Node 20 or later;
// the table was made with Node 26.13 (V8 14.6, ICU 78.3, CLDR 48).
//
// What it mirrors of the daemon:
//   - A number literal's decimals are its minimum fraction digits, and a
//     minimum above the maximum yields to the maximum (Fluent's NUMBER).
//   - With no maximum every digit shows, not Intl's default of three.
//   - Rounding is half away from zero on the number's shortest decimal digits.
//   - An instant with no zone, or a zone nobody knows, is read as UTC.
//   - A size or duration is its catalog message (`size`, `duration` of the
//     en-US catalog) around numbers written by the locale.
//
// The one difference: CLDR 48 writes U+202F before AM and PM in en-US, as does
// ICU4X, where V8 writes U+0020. The table keeps CLDR's, so a host running on
// V8 folds U+202F and U+00A0 to U+0020 before it compares.
//
// Where the oracle and ICU4X 2.4 (CLDR 49) write different words, the table has no case
// (see UNSETTLED): the two disagree about the data, not about what Ladderback asks of them.

import { writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
// The last five write digits or dates unlike the first three: other digits (ar, bn, hi), and a
// calendar other than the Gregorian (fa: Persian, th: Buddhist), which an instant is converted to.
const LOCALES = ["en-US", "de", "ja", "ar", "bn", "hi", "fa", "th"];

// Cases left out, because Node's ICU (CLDR 48) and ICU4X's data (CLDR 49) write different words:
//   - ar: the short names of zones (V8 writes "غرينتش+1" where ICU4X puts U+200E before the sign).
//   - ar, bn: the short name of UTC (V8 writes "UTC", ICU4X the locale's own name for it).
//   - th: the short date, whose year V8 writes with two digits and ICU4X with four.
//   - hi: the long date, which ICU4X writes with a comma after the month.
const READ_AS_UTC = [undefined, "UTC", "Europe/Atlantis"];
const UNSETTLED = [
  (locale, options) => locale === "ar" && options.timeZoneName === "short",
  (locale, options, zone) =>
    locale === "bn" && options.timeZoneName === "short" && READ_AS_UTC.includes(zone),
  (locale, options) => locale === "th" && options.dateStyle === "short",
  (locale, options) => locale === "hi" && options.dateStyle === "long",
];

// Instants, in milliseconds since the epoch.
const WINTER = 1794144645000; // 2026-11-08 13:30:45 UTC: standard time in Berlin, New York and Tokyo.
const SUMMER = 1783517400000; // 2026-07-08 13:30:00 UTC: summer time in Berlin and New York.
const BEFORE_GAP = 1774745999000; // 2026-03-29 00:59:59 UTC: the last second of winter time in Berlin.
const AFTER_GAP = 1774746000000; // 2026-03-29 01:00:00 UTC: the first of summer time.
const EPOCH = 0;
const BEFORE_EPOCH = -1; // 1969-12-31 23:59:59.999 UTC.
const LEAP_DAY = 1709208000000; // 2024-02-29 12:00:00 UTC.
const TOKYO_NEXT_DAY = 1794151800000; // 2026-11-08 15:30:00 UTC: already Nov 9 in Tokyo.
const NEW_YORK_PREVIOUS_DAY = 1794108600000; // 2026-11-08 03:30:00 UTC: still Nov 7 in New York.
const PACIFIC_YEARS = 1263556800000; // 2010-01-15 12:00:00 UTC: Metlakatla was on Pacific time.
const ALASKA_YEARS = 1579089600000; // 2020-01-15 12:00:00 UTC: and is on Alaska time.

const BERLIN = "Europe/Berlin";
const NEW_YORK = "America/New_York";
const TOKYO = "Asia/Tokyo";
const UTC = "UTC";
const NOWHERE = "Europe/Atlantis"; // A zone nobody has.
const METLAKATLA = "America/Metlakatla"; // Its short name changed with its metazone in 2015.

// --- The oracle -------------------------------------------------------------

function number(locale, literal, options = {}) {
  const decimals = (literal.split(".")[1] ?? "").length;
  const maximum = options.maximumFractionDigits ?? 100;
  const minimum = Math.min(options.minimumFractionDigits ?? decimals, maximum);
  const format = {
    minimumFractionDigits: minimum,
    maximumFractionDigits: maximum,
    roundingMode: "halfExpand",
  };
  if (options.minimumIntegerDigits !== undefined) format.minimumIntegerDigits = options.minimumIntegerDigits;
  if (options.useGrouping === "false") format.useGrouping = false;
  return new Intl.NumberFormat(locale, format).format(Number(literal));
}

function instant(locale, utcMs, zone, options = {}) {
  let timeZone = zone ?? UTC;
  const make = (extra) => {
    try {
      return new Intl.DateTimeFormat(locale, { timeZone, ...extra });
    } catch {
      timeZone = UTC;
      return new Intl.DateTimeFormat(locale, { timeZone, ...extra });
    }
  };
  if (options.timeZoneName === "short") {
    const parts = make({ timeZoneName: "short" }).formatToParts(utcMs);
    return parts.find((part) => part.type === "timeZoneName").value;
  }
  const styles =
    options.dateStyle || options.timeStyle
      ? { dateStyle: options.dateStyle, timeStyle: options.timeStyle }
      : { dateStyle: "medium", timeStyle: "short" };
  const shown = make(styles).format(utcMs);
  return locale === "en-US" ? shown.replace(/(\d) (AM|PM)/g, "$1\u202f$2") : shown;
}

// --- The size and duration helpers of the en-US catalog ---------------------

const UNITS = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];

// What `size_parts` makes of a byte count: the largest unit that keeps it at 1
// or more, in tenths, and up a unit when the tenths reach 1024.
function sizeParts(bytes) {
  const tenth = (value) => Math.round(value * 10) / 10;
  let value = Number(bytes);
  let unit = 0;
  while (unit + 1 < UNITS.length && tenth(value) >= 1024) {
    value /= 1024;
    unit += 1;
  }
  return [unit === 0 ? value : tenth(value), UNITS[unit]];
}

function size(locale, bytes) {
  const [value, unit] = sizeParts(bytes);
  return `${number(locale, String(value), { minimumFractionDigits: 0, maximumFractionDigits: 1 })} ${unit}`;
}

// What `duration_parts` makes of a millisecond count: its two largest units.
function duration(locale, milliseconds) {
  const ms = BigInt(milliseconds);
  const n = (value) => number(locale, String(value), { minimumFractionDigits: 0 });
  const [second, minute, hour, day] = [1000n, 60_000n, 3_600_000n, 86_400_000n];
  if (ms < second) return `${n(ms)} ms`;
  if (ms < minute) return `${n(ms / second)} s`;
  if (ms < hour) return `${n(ms / minute)} min ${n((ms % minute) / second)} s`;
  if (ms < day) return `${n(ms / hour)} h ${n((ms % hour) / minute)} min`;
  return `${n(ms / day)} d ${n((ms % day) / hour)} h`;
}

// A count above 2^53 reaches a message as its digits, not as a number.
function count(locale, digits) {
  return BigInt(digits) > 2n ** 53n ? digits : number(locale, digits, { minimumFractionDigits: 0 });
}

// --- The cases --------------------------------------------------------------

const numbers = [
  ["0"],
  ["7"],
  ["999"],
  ["1000"],
  ["1234567"],
  ["-1234567"],
  ["1.5"],
  ["-1234.5"],
  ["0.05"],
  ["0.30000000000000004"],
  ["1234567.891"],
  ["9007199254740992"],
  ["1234567890123456789"],
  // Rounding: half away from zero, on the digits the number is written with.
  ["1.25", { maximumFractionDigits: 1 }],
  ["1.35", { maximumFractionDigits: 1 }],
  ["1.5", { maximumFractionDigits: 1 }],
  ["2.5", { maximumFractionDigits: 0 }],
  ["-1.5", { maximumFractionDigits: 0 }],
  ["1234.5678", { maximumFractionDigits: 2 }],
  ["1.005", { maximumFractionDigits: 2 }],
  // Rounding writes no zeros the number has no digits for.
  ["7", { maximumFractionDigits: 1 }],
  ["1.25", { maximumFractionDigits: 3 }],
  // Padding.
  ["1.5", { minimumFractionDigits: 2 }],
  ["7", { minimumFractionDigits: 1 }],
  ["1234.5", { minimumFractionDigits: 3 }],
  ["1234.5", { minimumFractionDigits: 2, maximumFractionDigits: 2 }],
  ["7", { minimumIntegerDigits: 3 }],
  ["1234", { minimumIntegerDigits: 6 }],
  ["1234", { minimumIntegerDigits: 2 }],
  // Grouping off.
  ["1234567", { useGrouping: "false" }],
  ["-1234.5", { useGrouping: "false" }],
  // Signs and magnitudes at the edges.
  ["-0"],
  ["-0.04", { maximumFractionDigits: 1 }],
  ["0.0000001"],
  ["-0.0000001", { maximumFractionDigits: 3 }],
  ["123456789012345680000"],
];

const counts = ["0", "1234567", "9007199254740992", "18446744073709551615"];

const sizes = ["0", "1023", "1536", "1048575", "5905580032", "18446744073709551615"];

const durations = ["500", "61000", "3900000", "90000000", "100000000000", "18446744073709551615"];

const instants = [
  // Every style of the vocabulary, in Berlin in winter.
  [WINTER, BERLIN, {}],
  [WINTER, BERLIN, { dateStyle: "short" }],
  [WINTER, BERLIN, { dateStyle: "medium" }],
  [WINTER, BERLIN, { dateStyle: "long" }],
  [WINTER, BERLIN, { timeStyle: "short" }],
  [WINTER, BERLIN, { timeStyle: "medium" }],
  [WINTER, BERLIN, { dateStyle: "long", timeStyle: "short" }],
  [WINTER, BERLIN, { dateStyle: "short", timeStyle: "medium" }],
  [WINTER, BERLIN, { dateStyle: "medium", timeStyle: "medium" }],
  [WINTER, BERLIN, { timeZoneName: "short" }],
  // Summer time follows the instant and the zone, and so do the zone's names.
  [SUMMER, BERLIN, {}],
  [SUMMER, BERLIN, { dateStyle: "long", timeStyle: "medium" }],
  [SUMMER, BERLIN, { timeZoneName: "short" }],
  [BEFORE_GAP, BERLIN, { dateStyle: "medium", timeStyle: "medium" }],
  [AFTER_GAP, BERLIN, { dateStyle: "medium", timeStyle: "medium" }],
  [BEFORE_GAP, BERLIN, { timeZoneName: "short" }],
  [AFTER_GAP, BERLIN, { timeZoneName: "short" }],
  [WINTER, NEW_YORK, {}],
  [WINTER, NEW_YORK, { timeZoneName: "short" }],
  [SUMMER, NEW_YORK, {}],
  [SUMMER, NEW_YORK, { timeZoneName: "short" }],
  [WINTER, TOKYO, {}],
  [WINTER, TOKYO, { timeZoneName: "short" }],
  [WINTER, UTC, {}],
  [WINTER, UTC, { timeStyle: "medium" }],
  [WINTER, UTC, { timeZoneName: "short" }],
  // No zone, and a zone nobody has, read as UTC.
  [WINTER, undefined, {}],
  [WINTER, undefined, { timeZoneName: "short" }],
  [WINTER, NOWHERE, {}],
  [WINTER, NOWHERE, { timeZoneName: "short" }],
  // Edges of the calendar.
  [EPOCH, UTC, {}],
  [EPOCH, UTC, { dateStyle: "long" }],
  [BEFORE_EPOCH, UTC, {}],
  [BEFORE_EPOCH, UTC, { timeStyle: "medium" }],
  [LEAP_DAY, UTC, {}],
  [LEAP_DAY, UTC, { dateStyle: "long" }],
  // The date is the zone's, not UTC's.
  [TOKYO_NEXT_DAY, TOKYO, {}],
  [TOKYO_NEXT_DAY, TOKYO, { dateStyle: "long" }],
  [TOKYO_NEXT_DAY, UTC, { dateStyle: "long" }],
  [NEW_YORK_PREVIOUS_DAY, NEW_YORK, {}],
  [NEW_YORK_PREVIOUS_DAY, NEW_YORK, { dateStyle: "short", timeStyle: "medium" }],
  [NEW_YORK_PREVIOUS_DAY, UTC, { dateStyle: "short" }],
  // A zone's name follows its metazone through time, not only daylight saving time.
  [PACIFIC_YEARS, METLAKATLA, { timeZoneName: "short" }],
  [ALASKA_YEARS, METLAKATLA, { timeZoneName: "short" }],
  [PACIFIC_YEARS, METLAKATLA, {}],
  [ALASKA_YEARS, METLAKATLA, {}],
];

// --- The table --------------------------------------------------------------

const cases = [];
for (const locale of LOCALES) {
  for (const [literal, options] of numbers) {
    cases.push({ locale, number: literal, ...(options && { options }), expect: number(locale, literal, options) });
  }
  for (const digits of counts) cases.push({ locale, count: digits, expect: count(locale, digits) });
  for (const bytes of sizes) cases.push({ locale, size: bytes, expect: size(locale, bytes) });
  for (const milliseconds of durations) {
    cases.push({ locale, duration: milliseconds, expect: duration(locale, milliseconds) });
  }
  for (const [utcMs, zone, options] of instants) {
    if (UNSETTLED.some((unsettled) => unsettled(locale, options, zone))) continue;
    const when = { utcMs, ...(zone && { zone }) };
    const shown = instant(locale, utcMs, zone, options);
    cases.push({ locale, instant: when, ...(Object.keys(options).length > 0 && { options }), expect: shown });
  }
}

// Spaces that look alike are written as escapes, so a reviewer can tell them apart.
const escape = (json) =>
  json.replace(/[\u00a0\u2000-\u200f\u202f\u2060\u2066-\u2069]/g, (c) => `\\u${c.codePointAt(0).toString(16).padStart(4, "0")}`);

const about =
  "What every host that formats numbers and instants writes, for tests/expectations/mod.rs to run through the " +
  "public Localizer: the ICU4X host of the daemon (tests/lanes/icu.rs) and, later, the browser's Intl. " +
  "Made by generate.mjs from Node's Intl as an oracle. Differences from V8: en-US writes U+202F before AM and PM, " +
  "as CLDR does, where V8 writes U+0020, so a host running on V8 folds U+202F and U+00A0 to U+0020 before comparing. " +
  "A number literal's decimals are its minimum fraction digits, and the maximum wins over a larger minimum; " +
  "a count above 2^53 is its digits. A size and a duration are the en-US catalog's helpers around numbers " +
  "written by the locale. An instant with no zone, or a zone nobody has, is read as UTC, never as the viewer's: a caller that wants the viewer's zone passes it, and the browser lane runs with TZ set to a zone that is not UTC so a host that read the environment's zone fails. " +
  "Sizes, counts and durations travel as text, which holds a 64-bit number exactly.";

const lines = cases.map((c) => `    ${escape(JSON.stringify(c))}`);
const text = `{\n  "about": ${JSON.stringify(about)},\n  "cases": [\n${lines.join(",\n")}\n  ]\n}\n`;
writeFileSync(path.join(here, "formatting.json"), text);
console.log(`${cases.length} cases written`);
