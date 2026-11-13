import { useQueryClient } from "@tanstack/react-query";
import { PlusIcon, Trash2Icon } from "lucide-react";
import { useEffect, useState } from "react";
import { useForm, useWatch } from "react-hook-form";
import { toast } from "sonner";
import { ApiFailure } from "@/api/client";
import { post, qs, useAclDelete, useAclNew } from "@/api/r4";
import type { AclChoice } from "@/api/types/AclChoice";
import type { AclCreateOutcome } from "@/api/types/AclCreateOutcome";
import type { AclDeleteOutcome } from "@/api/types/AclDeleteOutcome";
import type { KafkaRefusal } from "@/api/types/KafkaRefusal";
import { Spans } from "@/components/app/spans";
import { StatusDot } from "@/components/app/status-dot";
import { Button } from "@/components/ui/button";
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Form, FormControl, FormDescription, FormField, FormItem, FormLabel, FormMessage } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { CallBox, RefusalPanel } from "../r4/bits";

type AclValues = { principal: string; host: string; type: string; pattern: string; name: string; op: string; perm: string };
const BLANK: AclValues = { principal: "", host: "*", type: "topic", pattern: "literal", name: "", op: "read", perm: "allow" };

/** The value a query string of the form has after a pause in typing (so the call box does not ask the server on every key). */
function useSettled(value: string, ms = 250): string {
  const [v, setV] = useState(value);
  useEffect(() => {
    const t = setTimeout(() => setV(value), ms);
    return () => clearTimeout(t);
  }, [value, ms]);
  return v;
}

function CreateBody({ kinds, operations, onDone }: { kinds: AclChoice[]; operations: AclChoice[]; onDone: () => void }) {
  const qc = useQueryClient();
  const form = useForm<AclValues>({ defaultValues: BLANK });
  const values = useWatch({ control: form.control }) as AclValues;
  const query = useSettled(qs({ type: values.type, name: values.name, pattern: values.pattern, principal: values.principal, host: values.host, op: values.op, perm: values.perm }));
  const preview = useAclNew(query, true);
  const [refused, setRefused] = useState<KafkaRefusal | null>(null);
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);

  const submit = async (v: AclValues) => {
    setBusy(true);
    setRefused(null);
    setFailure(null);
    try {
      const out = await post<AclCreateOutcome>("/api/v1/access/acls/new", v);
      if (out.outcome === "created") {
        toast.success("ACL created", { description: `${v.perm === "allow" ? "Allow" : "Deny"} ${v.principal} ${v.op} on ${v.type} ${v.name || "kafka-cluster"}` });
        await qc.invalidateQueries({ queryKey: ["access"] });
        onDone();
        return;
      }
      if (out.outcome === "invalid" && out.form.error) {
        const [field, words] = out.form.error;
        form.setError(field as keyof AclValues, { message: words });
      }
      if (out.outcome === "refused" && out.form.refused) setRefused(out.form.refused);
    } catch (e) {
      setFailure(e instanceof ApiFailure ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };
  const kindWord = preview.data?.kind_word ?? values.type;
  return (
    <Form {...form}>
      <form noValidate onSubmit={form.handleSubmit((v) => void submit(v))} className="grid min-w-0 items-start gap-4 sm:grid-cols-2" data-testid="acl-create-form">
        <FormField
          control={form.control}
          name="principal"
          rules={{ required: "Type the principal, as Type:name such as User:orders." }}
          render={({ field }) => (
            <FormItem>
              <FormLabel>Principal</FormLabel>
              <FormControl>
                <Input {...field} placeholder="User:orders" autoComplete="off" className="bg-card font-mono" data-testid="acl-principal" autoFocus />
              </FormControl>
              <FormDescription>Who it is for, as Type:name. User:* is everyone.</FormDescription>
              <FormMessage />
            </FormItem>
          )}
        />
        <FormField
          control={form.control}
          name="host"
          render={({ field }) => (
            <FormItem>
              <FormLabel>Host</FormLabel>
              <FormControl>
                <Input {...field} placeholder="*" autoComplete="off" className="bg-card font-mono" data-testid="acl-host" />
              </FormControl>
              <FormDescription>* is any address the client connects from.</FormDescription>
              <FormMessage />
            </FormItem>
          )}
        />
        <FormField
          control={form.control}
          name="type"
          render={({ field }) => (
            <FormItem>
              <FormLabel>Resource type</FormLabel>
              <Select value={field.value} onValueChange={field.onChange}>
                <FormControl>
                  <SelectTrigger className="w-full bg-card" data-testid="acl-type">
                    <SelectValue />
                  </SelectTrigger>
                </FormControl>
                <SelectContent position="popper" className="max-h-80">
                  {kinds.map((k) => (
                    <SelectItem key={k.value} value={k.value}>
                      {k.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <FormMessage />
            </FormItem>
          )}
        />
        <FormField
          control={form.control}
          name="pattern"
          render={({ field }) => (
            <FormItem>
              <FormLabel>Pattern type</FormLabel>
              <FormControl>
                <ToggleGroup type="single" variant="outline" value={field.value} onValueChange={(v) => v && field.onChange(v)} className="self-start bg-card" data-testid="acl-pattern">
                  <ToggleGroupItem value="literal" className="px-3">
                    Literal
                  </ToggleGroupItem>
                  <ToggleGroupItem value="prefixed" className="px-3">
                    Prefixed
                  </ToggleGroupItem>
                </ToggleGroup>
              </FormControl>
              <FormDescription>{field.value === "prefixed" ? `Every ${kindWord} whose name starts with it.` : `The ${kindWord} of exactly that name (* is every one).`}</FormDescription>
            </FormItem>
          )}
        />
        <FormField
          control={form.control}
          name="name"
          rules={{ validate: (v, all) => v.trim() !== "" || all.type === "cluster" || "Type the resource's name." }}
          render={({ field }) => (
            <FormItem className="sm:col-span-2">
              <FormLabel>Name</FormLabel>
              <FormControl>
                <Input {...field} placeholder={values.type === "cluster" ? "kafka-cluster" : "orders"} autoComplete="off" className="bg-card font-mono" data-testid="acl-name" />
              </FormControl>
              <FormMessage />
            </FormItem>
          )}
        />
        <FormField
          control={form.control}
          name="op"
          render={({ field }) => (
            <FormItem>
              <FormLabel>Operation</FormLabel>
              <Select value={field.value} onValueChange={field.onChange}>
                <FormControl>
                  <SelectTrigger className="w-full bg-card" data-testid="acl-op">
                    <SelectValue />
                  </SelectTrigger>
                </FormControl>
                <SelectContent position="popper" className="max-h-80">
                  {operations.map((o) => (
                    <SelectItem key={o.value} value={o.value}>
                      {o.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </FormItem>
          )}
        />
        <FormField
          control={form.control}
          name="perm"
          render={({ field }) => (
            <FormItem>
              <FormLabel>Permission</FormLabel>
              <FormControl>
                <ToggleGroup type="single" variant="outline" value={field.value} onValueChange={(v) => v && field.onChange(v)} className="self-start bg-card" data-testid="acl-perm-choice">
                  <ToggleGroupItem value="allow" className="px-3">
                    Allow
                  </ToggleGroupItem>
                  <ToggleGroupItem value="deny" className="px-3">
                    Deny
                  </ToggleGroupItem>
                </ToggleGroup>
              </FormControl>
              <FormDescription>A DENY wins over any ALLOW.</FormDescription>
            </FormItem>
          )}
        />
        <div className="flex flex-col gap-3 sm:col-span-2" aria-live="polite">
          {refused && <RefusalPanel refusal={refused} />}
          {failure && (
            <p role="alert" className="text-[13px] text-bad">
              {failure}
            </p>
          )}
          {preview.data ? <CallBox lines={preview.data.call} /> : <Skeleton className="h-40 w-full" />}
        </div>
        <DialogFooter className="border-t pt-4 sm:col-span-2">
          <DialogClose asChild>
            <Button type="button" variant="outline">
              Cancel
            </Button>
          </DialogClose>
          <Button type="submit" disabled={busy} data-testid="acl-create-submit">
            <PlusIcon /> {busy ? "Creating…" : "Create ACL"}
          </Button>
        </DialogFooter>
      </form>
    </Form>
  );
}

/** Create ACL, over the Access page (`?acl=new`). */
export function CreateAclDialog({ open, kinds, operations, onClose }: { open: boolean; kinds: AclChoice[]; operations: AclChoice[]; onClose: () => void }) {
  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="top-6 max-h-[calc(100vh-3rem)] translate-y-0 overflow-y-auto sm:max-w-[42rem]" data-testid="acl-create-dialog">
        <DialogHeader>
          <DialogTitle className="text-[20px] font-medium">Create ACL</DialogTitle>
          <DialogDescription>One rule of the authorizer: a principal may (or may not) do an operation on a resource. Sent as CreateAcls, as the console.</DialogDescription>
        </DialogHeader>
        {open && <CreateBody kinds={kinds} operations={operations} onDone={onClose} />}
      </DialogContent>
    </Dialog>
  );
}

/** Delete ACL, over the Access page (`?delete=<the ACL's form words>`): the ACL, what the broker decides afterwards, the call. */
export function DeleteAclDialog(props: { query: string | undefined; kinds: AclChoice[]; operations: AclChoice[]; onClose: () => void }) {
  return <DeleteAcl key={props.query ?? ""} {...props} />;
}

function DeleteAcl({ query, kinds, operations, onClose }: { query: string | undefined; kinds: AclChoice[]; operations: AclChoice[]; onClose: () => void }) {
  const qc = useQueryClient();
  const q = useAclDelete(query);
  const v = q.data;
  const [refused, setRefused] = useState<KafkaRefusal | null>(null);
  const [gone, setGone] = useState(false);
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  const remove = async () => {
    if (!query) return;
    setBusy(true);
    setFailure(null);
    try {
      const body = Object.fromEntries(new URLSearchParams(query));
      const out = await post<AclDeleteOutcome>("/api/v1/access/acls/delete", body);
      if (out.outcome === "deleted") {
        toast.success("ACL deleted");
        await qc.invalidateQueries({ queryKey: ["access"] });
        onClose();
        return;
      }
      if (out.outcome === "gone") setGone(true);
      else setRefused(out.form.refused);
    } catch (e) {
      setFailure(e instanceof ApiFailure ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };
  const e = v?.entry;
  const label = (list: AclChoice[], n: number) => list.find((k) => k.number === n)?.label ?? String(n);
  return (
    <Dialog open={!!query} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="top-6 max-h-[calc(100vh-3rem)] translate-y-0 overflow-y-auto sm:max-w-[40rem]" data-testid="acl-delete-dialog">
        <DialogHeader>
          <DialogTitle className="text-[20px] font-medium">Delete this ACL?</DialogTitle>
          <DialogDescription>DeleteAcls with a filter that matches this one ACL exactly, as the console.</DialogDescription>
        </DialogHeader>
        {q.error ? (
          <p role="alert" className="text-[13.5px] text-bad">
            {q.error.message}
          </p>
        ) : !v || !e ? (
          <Skeleton className="h-60 w-full" />
        ) : (
          <div className="flex min-w-0 flex-col gap-4">
            <div className="rounded-lg border bg-card p-4 text-[13.5px]" data-testid="acl-delete-entry">
              <StatusDot tone={e.permission === "allow" ? "ok" : "warn"}>{e.permission === "allow" ? "Allow" : "Deny"}</StatusDot>{" "}
              <code className="font-mono text-bright">{e.principal}</code> <span className="text-muted-foreground">to</span> <code className="font-mono">{label(operations, e.operation)}</code>{" "}
              <span className="text-muted-foreground">on {label(kinds, e.kind).toLowerCase()}</span> <code className="font-mono text-bright">{e.name}</code>
              <span className="text-muted-foreground">
                {" "}
                ({e.pattern}, from host <code className="font-mono">{e.host}</code>)
              </span>
            </div>
            {v.gone || gone ? (
              <p role="status" className="rounded-lg border bg-well p-3 text-[13.5px]" data-testid="acl-gone">
                This ACL is no longer there: something else deleted it. There is nothing to delete.
              </p>
            ) : (
              v.after.length > 0 && (
                <div className="rounded-lg border-l-2 border-primary/50 bg-well p-3 text-[13.5px]" data-testid="acl-after">
                  <b className="font-semibold text-foreground">What the broker decides afterwards: </b>
                  <Spans spans={v.after} />
                </div>
              )
            )}
            {(refused ?? v.refused) && <RefusalPanel refusal={(refused ?? v.refused)!} />}
            {failure && (
              <p role="alert" className="text-[13px] text-bad">
                {failure}
              </p>
            )}
            <CallBox lines={v.call} />
          </div>
        )}
        <DialogFooter className="border-t pt-4">
          <DialogClose asChild>
            <Button type="button" variant="outline">
              {v?.gone || gone ? "Close" : "Cancel"}
            </Button>
          </DialogClose>
          {!(v?.gone || gone) && (
            <Button variant="destructive" disabled={busy || !v} onClick={() => void remove()} data-testid="acl-delete-confirm">
              <Trash2Icon /> {busy ? "Deleting…" : "Delete ACL"}
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
