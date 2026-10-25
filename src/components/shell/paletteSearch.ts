/** Case- and accent-insensitive form of a label or a query. */
const fold = (text: string): string =>
  text.normalize("NFKD").replace(/\p{M}/gu, "").toLocaleLowerCase().trim();

/**
 * How well a palette entry's label matches what was typed; 0 is no match.
 *
 * An empty query matches everything equally. A label that starts with the
 * query beats one with a word that starts with it, which beats the query
 * anywhere inside it, which beats the query's letters in order with gaps
 * ("shcut" finds "Shortcuts"). Pure, so it is tested on its own.
 */
export const matchScore = (label: string, query: string): number => {
  const q = fold(query);
  if (!q) return 1;
  const l = fold(label);
  if (l.startsWith(q)) return 100 - Math.min(l.length - q.length, 50) / 100;
  if (l.split(/[\s\-–·/]+/).some((word) => word.startsWith(q))) return 80;
  if (l.includes(q)) return 60;
  let at = 0;
  for (const letter of q) {
    if (letter === " ") continue;
    at = l.indexOf(letter, at);
    if (at < 0) return 0;
    at += 1;
  }
  return 20;
};
