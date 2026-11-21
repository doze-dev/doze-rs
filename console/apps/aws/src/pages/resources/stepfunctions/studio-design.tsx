import type { Definition } from "./graph";
export function DesignMode(_p: { text: string; setText: (t: string) => void; definition: Definition | null; valid: boolean; pending: boolean }) {
  return <div data-testid="studio-design" />;
}
