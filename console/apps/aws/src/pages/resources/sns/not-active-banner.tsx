import { Alert } from "@doze/ui/components/app/alert";
import { storedWhy } from "./protocols";

/** `1 subscription was created but is not active locally`: what a script made that doze-aws keeps and delivers nothing to. */
export function NotActiveBanner({ protocols }: { protocols: readonly string[] }) {
  const n = protocols.filter((p) => storedWhy(p) !== null).length;
  if (n === 0) return null;
  return (
    <Alert tone="info" title={`${n} ${n === 1 ? "subscription was" : "subscriptions were"} created but ${n === 1 ? "is" : "are"} not active locally.`} className="mb-6" testId="not-active-banner">
      A script created {n === 1 ? "it" : "them"}. {n === 1 ? "It is" : "They are"} stored and listed, and nothing will be delivered. The script saw the same answer as AWS gives, pending confirmation; the part AWS never needs to say is that here it stays that way.
    </Alert>
  );
}
