// The list of functions: its rows read from the server's cells, and the sentence about what ran in the last hour. Pure.
import type { ResourceRow } from "../../../api/types/ResourceRow";
import { runtimeName } from "./model";

export type FunctionRow = { name: string; description: string; pkg: string; runtime: string; runtimeText: string; modified: string | null; layers: string[]; role: string };

/** The rows of the list: the columns the descriptor reads are Description, Package type, Runtime, Last modified, Handler, Memory, Code size, Layers. */
export function rowsOf(rows: readonly ResourceRow[]): FunctionRow[] {
  return rows.map((r) => {
    const layers = layerArns(r.cells[7]);
    return { name: r.name, description: r.cells[0] ?? "", pkg: r.cells[1] ?? "Zip", runtime: r.cells[2] ?? "", runtimeText: runtimeName(r.cells[2]), modified: r.cells[3], layers, role: r.cells[8] ?? "" };
  });
}

const times = (n: number): string => (n === 1 ? "once" : `${n.toLocaleString("en-US")} times`);

/** `send-email was invoked 6 times in the last hour, resize-product-image 41 times, and export-report once.` From the function names and their counts; none ran, none said. */
export function invokedSentence(counts: readonly { name: string; n: number }[]): string | null {
  const ran = counts.filter((c) => c.n > 0).sort((a, b) => b.n - a.n);
  if (ran.length === 0) return null;
  const parts = ran.map((c, i) => (i === 0 ? `${c.name} was invoked ${times(c.n)} in the last hour` : `${c.name} ${times(c.n)}`));
  if (parts.length === 1) return `${parts[0]}.`;
  return `${parts.slice(0, -1).join(", ")}, and ${parts[parts.length - 1]}.`;
}

/** What the layers page says: how many layers there are with how many versions, and whether a function uses any. */
export function layersSentence(layers: readonly { name: string; versions: number | null }[], uses: number): string | null {
  if (layers.length === 0) return null;
  const n = layers.length;
  const versions = layers.map((l) => l.versions);
  const detail = layers.length <= 3 ? layers.map((l) => l.name).join(", ") : null;
  const counted = versions.every((v) => v !== null) ? versions.reduce<number>((a, b) => a + (b ?? 0), 0) : null;
  const head = `${n} ${n === 1 ? "layer" : "layers"} here${detail ? `, ${detail}` : ""}${counted !== null ? `, with ${counted} ${counted === 1 ? "version" : "versions"}` : ""}.`;
  const who = n === 1 ? "it" : "them";
  return `${head} ${uses === 0 ? `No function uses ${who} yet.` : `${uses} ${uses === 1 ? "function uses" : "functions use"} ${who}.`}`;
}

/** The ARNs of the layers a function's cell holds (a JSON list of `{Arn}`); none for a cell that is not one. */
function layerArns(cell: string | null | undefined): string[] {
  try {
    return (JSON.parse(cell ?? "[]") as { Arn: string }[]).map((l) => l.Arn);
  } catch {
    return [];
  }
}
