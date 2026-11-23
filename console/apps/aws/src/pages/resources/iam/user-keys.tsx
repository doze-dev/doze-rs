// A user's Security credentials tab (iam__user-credentials, access-keys-none): the access keys with their status, age and last use, what
// to do to rotate one, and the actions that switch a key off or delete it. The secret is never here: IAM gave it once, to whoever made the key.
import { useNavigate } from "@tanstack/react-router";
import { ChevronDownIcon, KeyRoundIcon, PlusIcon } from "lucide-react";
import { useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { EmptyTab } from "@doze/ui/components/app/empty-tab";
import { StatusDot } from "@doze/ui/components/app/status-dot";
import { Button } from "@doze/ui/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@doze/ui/components/ui/dialog";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "@doze/ui/components/ui/dropdown-menu";
import { dateTimeUtc } from "@doze/ui/lib/format";
import { clientName } from "../../../people";
import { USERS, useIamCalls, useWrite } from "./api";
import type { AccessKey } from "./model";
import { Card, Note } from "./parts";
import type { UserAccess } from "./user-data";

const MAX_KEYS = 2;

export function SecurityCredentials({ user, access }: { user: string; access: UserAccess }) {
  const navigate = useNavigate();
  const calls = useIamCalls().data?.calls ?? [];
  const [chosen, setChosen] = useState<string | null>(null);
  const [deleting, setDeleting] = useState<AccessKey | null>(null);
  const update = useWrite(USERS, "UpdateAccessKey");
  const remove = useWrite(USERS, "DeleteAccessKey");
  const keys = access.keys;
  const key = keys.find((k) => k.AccessKeyId === (chosen ?? (keys.length === 1 ? keys[0]!.AccessKeyId : ""))) ?? null;
  const create = () => void navigate({ to: "/resources/$service/$kind/$name", params: { service: "iam", kind: "users", name: user }, search: { view: "access-key" } });
  const full = keys.length >= MAX_KEYS;
  const createButton = (
    <Button type="button" onClick={create} disabled={full} title={full ? `A user holds at most ${MAX_KEYS} access keys: delete one first.` : undefined} data-testid="create-key"><PlusIcon /> Create access key</Button>
  );
  if (keys.length === 0) {
    return (
      <EmptyTab icon={KeyRoundIcon} title="No access keys" action={createButton} testId="keys-none">
        {user} has no access keys, so it cannot call the API from code. A user can hold up to two.
      </EmptyTab>
    );
  }
  const columns: DataColumn<AccessKey>[] = [
    { id: "id", header: "Access key ID", size: 330, min: 230, accessorFn: (r) => r.AccessKeyId, meta: { mono: true }, cell: (c) => <span className="font-mono text-[13px] font-medium text-bright">{c.row.original.AccessKeyId}</span> },
    { id: "status", header: "Status", size: 160, accessorFn: (r) => r.Status, cell: (c) => <StatusDot tone={c.row.original.Status === "Active" ? "ok" : "idle"}>{c.row.original.Status}</StatusDot> },
    { id: "created", header: "Created", size: 230, accessorFn: (r) => r.CreateDate, cell: (c) => dateTimeUtc(c.row.original.CreateDate * 1000) },
    { id: "used", header: "Last used", accessorFn: (r) => access.used.get(r.AccessKeyId)?.LastUsedDate ?? 0, cell: (c) => { const u = access.used.get(c.row.original.AccessKeyId); return u?.LastUsedDate ? <span className="grid leading-5"><span>{dateTimeUtc(u.LastUsedDate * 1000)}</span><span className="text-[12.5px] text-muted-foreground">{u.ServiceName}, {u.Region}</span></span> : <span className="text-muted-foreground" title="IAM has no record of a call signed with this key">Never</span>; } },
  ];
  const clientOf = (id: string): string | null => { const sdk = calls.find((c) => c.caller === id && c.sdk)?.sdk; return sdk ? clientName(sdk) : null; };
  const client = clientOf(keys[0]!.AccessKeyId);
  const why = key === null ? "Choose a key in the table first." : undefined;
  return (
    <Card
      title={`Access keys (${keys.length})`}
      lede="The list shows each access key ID and its status. Only the access key ID can be viewed again; the secret access key can be retrieved only when the key is created."
      actions={
        <>
          <DropdownMenu>
            <DropdownMenuTrigger asChild><Button type="button" variant="outline" className="bg-card" data-testid="keys-actions">Actions <ChevronDownIcon /></Button></DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="min-w-48">
              <DropdownMenuItem disabled={!key || key.Status !== "Active"} title={why} onSelect={() => key && update.mutate({ UserName: user, AccessKeyId: key.AccessKeyId, Status: "Inactive" })} data-testid="key-deactivate">Deactivate</DropdownMenuItem>
              <DropdownMenuItem disabled={!key || key.Status === "Active"} title={why} onSelect={() => key && update.mutate({ UserName: user, AccessKeyId: key.AccessKeyId, Status: "Active" })} data-testid="key-activate">Activate</DropdownMenuItem>
              <DropdownMenuItem disabled={!key} title={why} onSelect={() => key && setDeleting(key)} data-testid="key-delete">Delete</DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
          {createButton}
        </>
      }
      testId="keys-card"
    >
      <DataTable aria-label="Access keys" data={keys} columns={columns} rowId={(r) => r.AccessKeyId} onRowClick={(r) => setChosen(r.AccessKeyId)} rowClassName={(r) => (r.AccessKeyId === key?.AccessKeyId && keys.length > 1 ? "[&>td]:bg-pick/50" : undefined)} maxHeight={320} />
      {keys.length > 1 && <p className="text-[13px] text-muted-foreground" data-testid="keys-choose">Choose a key to use Actions on it{key ? <>: <span className="font-mono">{key.AccessKeyId}</span> is chosen</> : null}.</p>}
      {update.error && <Alert tone="bad" title={`UpdateAccessKey was refused`} testId="key-refused">{update.error.message}</Alert>}
      <Note testId="rotate-note">
        To rotate a key, create a second one, move the workload to it{client ? <> (<span className="font-mono">{client}</span> uses this one)</> : null}, then deactivate the first and, once nothing breaks, delete it. <span className="dim">AWS allows two access keys per user, and so does doze-aws: a third is refused with LimitExceeded.</span>
      </Note>
      <Dialog open={deleting !== null} onOpenChange={(o) => { if (!o) { setDeleting(null); remove.reset(); } }}>
        <DialogContent className="sm:max-w-[560px]" data-testid="key-delete-dialog">
          <DialogHeader>
            <DialogTitle className="text-[21px] font-semibold">Delete access key</DialogTitle>
            <DialogDescription className="text-[14px] leading-[22px] text-foreground">This deletes <span className="font-mono font-semibold text-bright">{deleting?.AccessKeyId}</span>. Anything that signs requests with it stops working at once, and a deleted key cannot be recovered.{deleting?.Status === "Active" ? " It is active: deactivate it first to see whether anything breaks." : ""}</DialogDescription>
          </DialogHeader>
          {remove.error && <Alert tone="bad" title="DeleteAccessKey was refused" testId="key-delete-refused">{remove.error.message}</Alert>}
          <DialogFooter>
            <Button type="button" variant="outline" className="bg-card" onClick={() => setDeleting(null)}>Cancel</Button>
            <Button type="button" variant="destructive" disabled={remove.isPending} onClick={() => deleting && remove.mutate({ UserName: user, AccessKeyId: deleting.AccessKeyId }, { onSuccess: () => { setDeleting(null); setChosen(null); } })} data-testid="key-delete-submit">{remove.isPending ? "Working…" : "Delete"}</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </Card>
  );
}
