// The order a template lists its resources in. ListStackResources answers by logical ID, and the boards draw what a stack owns in the order
// the template declares it (the queue it is built around first), so the page puts that order back. Pure, so it is tested without a browser.

/** The logical IDs under `Resources`, in the order the template writes them (JSON, or YAML read line by line; nothing is evaluated). */
export function resourceOrder(template: string): string[] {
  try {
    const t = JSON.parse(template) as { Resources?: Record<string, unknown> };
    return Object.keys(t.Resources ?? {});
  } catch {
    /* YAML */
  }
  const ids: string[] = [];
  let inResources = false;
  let idsAt: number | null = null;
  for (const line of template.split("\n")) {
    const body = line.trimStart();
    if (body === "" || body.startsWith("#")) continue;
    const indent = line.length - body.length;
    if (indent === 0) {
      inResources = body.startsWith("Resources:");
      idsAt = null;
      continue;
    }
    const colon = body.indexOf(":");
    if (!inResources || colon < 0) continue;
    idsAt ??= indent;
    if (idsAt === indent) ids.push(body.slice(0, colon).replace(/^["']|["']$/g, ""));
  }
  return ids;
}

/** `rows` in the template's order; one the template does not name (it was read late, or is not there) keeps its place after the rest. */
export function inTemplateOrder<T extends { LogicalResourceId: string }>(rows: readonly T[], order: readonly string[]): T[] {
  const at = (r: T) => {
    const i = order.indexOf(r.LogicalResourceId);
    return i < 0 ? order.length : i;
  };
  return rows.map((r, i) => ({ r, i })).sort((a, b) => at(a.r) - at(b.r) || a.i - b.i).map(({ r }) => r);
}
