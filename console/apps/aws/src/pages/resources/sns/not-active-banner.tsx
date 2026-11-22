import { useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { storedWhy } from "./protocols";

/** `1 subscription was created but is not active locally`: what a script made that doze-aws keeps and delivers nothing to, and Why?
 * (the reason for each protocol, which is the local support table's). */
export function NotActiveBanner({ protocols }: { protocols: readonly string[] }) {
  const [why, setWhy] = useState(false);
  const stored = protocols.filter((p) => storedWhy(p) !== null);
  const n = stored.length;
  if (n === 0) return null;
  const kinds = [...new Set(stored)];
  return (
    <Alert tone="info" title={`${n} ${n === 1 ? "subscription was" : "subscriptions were"} created but ${n === 1 ? "is" : "are"} not active locally.`} className="mb-6" testId="not-active-banner">
      A script created {n === 1 ? "it" : "them"}. {n === 1 ? "It is" : "They are"} stored and listed, and nothing will be delivered to {n === 1 ? "it" : "them"}.{" "}
      <button type="button" className="rounded-sm font-medium text-link hover:underline focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none" aria-expanded={why} onClick={() => setWhy(!why)} data-testid="not-active-why">
        Why?
      </button>
      {why && (
        <span className="mt-2 grid gap-1" data-testid="not-active-reasons">
          {kinds.map((k) => (
            <span key={k}>
              <span className="font-mono">{k}</span>: {storedWhy(k)}
            </span>
          ))}
          <span>The script saw the same answer as AWS gives, pending confirmation. The part AWS never needs to say is that here it stays that way.</span>
        </span>
      )}
    </Alert>
  );
}
