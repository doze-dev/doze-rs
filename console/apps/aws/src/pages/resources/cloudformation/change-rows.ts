// The rows of a change set, in the order the boards read them: what would be added, then changed, then removed.
export type ChangeRow = { Action: string; LogicalResourceId: string; PhysicalResourceId?: string; ResourceType: string; Replacement?: string };

const ORDER = ["Add", "Modify", "Remove"];
const rank = (action: string): number => {
  const i = ORDER.indexOf(action);
  return i < 0 ? ORDER.length : i;
};

/** `Changes` as rows, the ones with a ResourceChange only, Add before Modify before Remove (the service's own order within each: the sort is stable). */
export function changeRows(set: { Changes?: { ResourceChange?: ChangeRow }[] }): ChangeRow[] {
  return (set.Changes ?? []).flatMap((c) => (c.ResourceChange ? [c.ResourceChange] : [])).sort((a, b) => rank(a.Action) - rank(b.Action));
}
