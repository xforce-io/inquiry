// Resolve saved citation identities without guessing evidence-ID indices.
export type Citation = {
  artifact_id: string;
  sha256: string;
  source_id?: string;
  paragraph: number;
  quote: string;
};
export type Evidence = Citation & { id: string };

/** Build display aliases only from unambiguous complete evidence tuples. */
export function buildCitationAliases(citations: readonly Citation[], evidence: readonly Evidence[]): ReadonlyMap<string, number> {
  const positions = new Map<string, number | undefined>();
  citations.forEach((citation, index) => {
    if (citation.source_id !== undefined) {
      const key = citationKey(citation);
      positions.set(key, positions.has(key) ? undefined : index);
    }
  });
  const seen = new Set<string>();
  const aliases = new Map<string, number>();
  for (const item of evidence) {
    if (seen.has(item.id)) {
      aliases.delete(item.id);
      continue;
    }
    seen.add(item.id);
    const index = item.source_id === undefined ? undefined : positions.get(citationKey(item));
    if (index !== undefined) {
      aliases.set(item.id, index);
    }
  }
  return aliases;
}

/** Keep numeric markers canonical; opaque IDs need an explicit tuple-derived alias. */
export function resolveCitationIndex(marker: string, count: number, aliases?: ReadonlyMap<string, number>): number | undefined {
  let index: number | undefined;
  if (/^[0-9]+$/.test(marker)) {
    if (!/^[1-9][0-9]*$/.test(marker)) {
      return undefined;
    }
    index = Number(marker) - 1;
  } else {
    index = aliases?.get(marker);
  }
  return index !== undefined && Number.isSafeInteger(index) && index >= 0 && index < count ? index : undefined;
}

function citationKey(citation: Citation): string {
  return JSON.stringify([citation.artifact_id, citation.sha256, citation.source_id, citation.paragraph, citation.quote]);
}
