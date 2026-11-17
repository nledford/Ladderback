// The journey lint's rule for keyboard walks (§4.4): a walk to a target in a
// page's content starts after the header, with "I skip to the content".

/** A walk that crosses the header to reach the page's content. */
export interface Finding {
  line: number;
  step: string;
  from: "top" | "header" | "content's start";
}

/** How a walk step names a stop in the header, from the English catalog. */
export function headerTargets(catalog: string): RegExp {
  throw new Error(`todo: the header's stops in ${catalog.length} bytes of catalog`);
}

/** The walks in `feature`'s `@ui` scenarios that cross the header. */
export function walksWithoutSkip(feature: string, header: RegExp): Finding[] {
  throw new Error(`todo: the walks in ${feature.length} bytes against ${header.source}`);
}
