// Stubs for the red test commit; the implementation follows.

export interface Entry {
  id: string;
  text: string;
}

export interface Stale {
  id: string;
  hash: string;
  now: string;
}

export interface Listing {
  tag: string;
  total: number;
  current: number;
  missing: string[];
  unreviewed: string[];
  stale: Stale[];
  unknown: string[];
}

const NOT_YET = "not implemented yet";

export function entries(_ftl: string): Entry[] {
  throw new Error(NOT_YET);
}

export function sourceHash(_text: string): string {
  throw new Error(NOT_YET);
}

export function parseSources(_text: string, _file: string): Map<string, string> {
  throw new Error(NOT_YET);
}

export function renderSources(_sources: ReadonlyMap<string, string>): string {
  throw new Error(NOT_YET);
}

export function listing(
  _tag: string,
  _english: readonly Entry[],
  _translated: ReadonlySet<string>,
  _sources: ReadonlyMap<string, string>,
): Listing {
  throw new Error(NOT_YET);
}

export function renderListing(_listings: readonly Listing[], _translatedFrom: (id: string, hash: string) => string | null): string {
  throw new Error(NOT_YET);
}

export function accepting(
  _tag: string,
  _english: readonly Entry[],
  _translated: ReadonlySet<string>,
  _sources: ReadonlyMap<string, string>,
  _ids: readonly string[] | "all",
): { sources: Map<string, string>; accepted: string[] } | string {
  throw new Error(NOT_YET);
}

export function history(_dir: string): (id: string, hash: string) => string | null {
  throw new Error(NOT_YET);
}

if (process.argv[1]?.endsWith("translations.ts")) {
  console.error(NOT_YET);
  process.exitCode = 1;
}
