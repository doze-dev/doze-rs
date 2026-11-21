export type Chip = { value: string; label: string; /** How many rows it holds; shown beside the label. */ count?: number | null };

/** The chosen chip for a value read from the address: the value when it names a chip, the first chip otherwise (an old link,
 * a typo, a filter that no longer exists must not leave a list with nothing chosen). */
export function chipValue(raw: string | undefined, chips: readonly Chip[]): string {
  return chips.find((c) => c.value === raw)?.value ?? chips[0]?.value ?? "";
}

/** `In alarm 2` for a screen reader. */
export const chipWords = (c: Chip) => (c.count === undefined || c.count === null ? c.label : `${c.label} ${c.count.toLocaleString("en-US")}`);
