// What a person can do with a call: copy it as an AWS CLI command or as curl (the server writes both, from what the record has),
// and open what it acted on in Resources. In the row's menu and in the drawer.
//
// Not here: replay. It would re-send the request, and the record does not keep requests (bodies weigh what they weigh and carry
// whatever secrets were in them), so there is nothing to re-send.
import { Link } from "@tanstack/react-router";
import { ExternalLinkIcon, MoreHorizontalIcon, SquareTerminalIcon, TerminalIcon } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@doze/ui/components/ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from "@doze/ui/components/ui/dropdown-menu";
import { errorWords } from "@doze/ui/components/app/alert";
import { client, PATHS } from "../../api/client";
import type { CallRecord } from "../../api/types/CallRecord";
import type { CopyCommands } from "../../api/types/CopyCommands";
import { resourceHref, resourceName, resourceSearchOf } from "./resource";

export type CopyKind = "cli" | "curl";

const WORDS: Record<CopyKind, string> = { cli: "AWS CLI command", curl: "curl command" };

/** Asks the server for the call's commands and copies one. Says what was filled in: every other parameter is the person's to add. */
export async function copyCommand(seq: number, kind: CopyKind): Promise<boolean> {
  let made: CopyCommands;
  try {
    made = await client.api<CopyCommands>(PATHS.copy(seq));
  } catch (e) {
    toast.error(`The ${WORDS[kind]} could not be made`, { description: errorWords(e).message });
    return false;
  }
  const text = made[kind];
  if (text === null) {
    toast.error(`This call has no ${WORDS[kind]}`, { description: kind === "cli" ? "It named no operation of a service of this build." : "Its protocol is not one curl can be written for here." });
    return false;
  }
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    toast.error("The browser did not allow copying", { description: "Open the call and select the text by hand." });
    return false;
  }
  const filled = made.filled.length > 0 ? `${made.filled.join(", ")} filled in` : "No parameter filled in";
  toast.success(`${WORDS[kind][0]!.toUpperCase()}${WORDS[kind].slice(1)} copied`, { description: `${filled}: the record keeps no request body, so add any other parameter the call needs.`, duration: 3500 });
  return true;
}

/** The menu at the end of a row. */
export function RowMenu({ call }: { call: CallRecord }) {
  const href = resourceHref(call);
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="ghost" size="icon-sm" className="-my-1 opacity-60 group-hover:opacity-100 focus-visible:opacity-100 data-[state=open]:opacity-100" aria-label={`Actions for call ${call.seq}`} onClick={(e) => e.stopPropagation()} data-testid="row-menu">
          <MoreHorizontalIcon />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" onClick={(e) => e.stopPropagation()}>
        <DropdownMenuItem disabled={call.action === null} onSelect={() => void copyCommand(call.seq, "cli")} data-testid="copy-cli">
          <SquareTerminalIcon /> Copy as AWS CLI
        </DropdownMenuItem>
        <DropdownMenuItem disabled={call.action === null} onSelect={() => void copyCommand(call.seq, "curl")} data-testid="copy-curl">
          <TerminalIcon /> Copy as curl
        </DropdownMenuItem>
        {href && (
          <>
            <DropdownMenuSeparator />
            <DropdownMenuItem asChild data-testid="open-resource">
              <Link to={href} search={resourceSearchOf(call)}>
                <ExternalLinkIcon /> Open <span className="max-w-[14rem] truncate font-mono text-[12.5px]">{resourceName(call)}</span>
              </Link>
            </DropdownMenuItem>
          </>
        )}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
