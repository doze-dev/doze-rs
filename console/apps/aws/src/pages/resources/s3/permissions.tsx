// A bucket's Permissions tab: Block Public Access, the bucket policy, Object Ownership with the ACL, and CORS. Each section has
// an Edit that opens its own page. What doze-aws applies (all four Block Public Access settings, ownership, the policy, CORS) is
// said as it is; nothing here claims a setting is only stored.
import { PencilIcon } from "lucide-react";
import { CheckIcon, MinusIcon } from "lucide-react";
import { DataTable } from "@doze/ui/components/app/data-table";
import { JsonBlock } from "@doze/ui/components/app/code-block";
import { Button } from "@doze/ui/components/ui/button";
import { Card } from "../deep/parts";
import { NONE, useSetting } from "./api";
import { aclRows, BPA_ALL, bpaOf, bpaRows, OWNERSHIP_WORDS, ownershipOf, permissionsSentence } from "./config";
import { NoneYet, Reading, Rows } from "./parts";

/** The reads of the tab; the page above it uses them for its sentence. */
export function usePermissions(name: string, enabled: boolean) {
  const bpa = useSetting(name, "GetPublicAccessBlock", NONE.bpa, {}, enabled);
  const policy = useSetting<{ Policy?: string }>(name, "GetBucketPolicy", NONE.policy, {}, enabled);
  const ownership = useSetting(name, "GetBucketOwnershipControls", NONE.ownership, {}, enabled);
  const acl = useSetting(name, "GetBucketAcl", [], {}, enabled);
  const cors = useSetting<{ CORSRules?: unknown[] }>(name, "GetBucketCors", NONE.cors, {}, enabled);
  return { bpa, policy, ownership, acl, cors, ready: [bpa, policy, ownership, acl, cors].every((q) => q.isSuccess || q.absent) };
}
type Permissions = ReturnType<typeof usePermissions>;

export function sentenceOf(name: string, p: Permissions): string | null {
  if (!p.ready) return null;
  // A bucket with no block configuration has no setting on (S3 answers NoSuchPublicAccessBlockConfiguration).
  return permissionsSentence(name, bpaOf(p.bpa.data), p.policy.data?.Policy);
}

const edit = (go: (d: string) => void, what: string) => (
  <Button variant="outline" size="sm" className="bg-card" onClick={() => go(what)} data-testid={`${what}-open`}>
    <PencilIcon /> Edit
  </Button>
);

const State = ({ on }: { on: boolean }) => (
  <span className="inline-flex items-center gap-1.5">
    {on ? <CheckIcon aria-hidden className="size-4 text-good" /> : <MinusIcon aria-hidden className="size-4 text-muted-foreground" />}
    {on ? "Enabled" : "Disabled"}
  </span>
);

export function PermissionsTab({ name, p, go }: { name: string; p: Permissions; go: (action: string) => void }) {
  const bpa = p.bpa.absent ? { ...BPA_ALL, BlockPublicAcls: false, IgnorePublicAcls: false, BlockPublicPolicy: false, RestrictPublicBuckets: false } : bpaOf(p.bpa.data);
  const own = ownershipOf(p.ownership.data);
  const acl = aclRows(p.acl.data);
  const rules = p.cors.data?.CORSRules ?? [];
  const policy = p.policy.data?.Policy;
  return (
    <div className="grid gap-6" data-testid="permissions">
      <Card title="Block public access (bucket settings)" lede="Public access is granted to buckets and objects through ACLs, bucket policies, access point policies or all of them." actions={edit(go, "bpa-edit")}>
        <Reading q={p.bpa} what="Block public access" height="h-32">
          <DataTable aria-label="Block public access" data={bpaRows(bpa)} rowId={(r) => r.setting} maxHeight={320} columns={[{ id: "setting", header: "Setting", accessorKey: "setting", size: 360, meta: { mono: true } }, { id: "state", header: "State", accessorFn: (r) => (r.on ? 1 : 0), cell: (c) => <State on={c.row.original.on} /> }]} />
        </Reading>
      </Card>
      <Card title="Bucket policy" lede="A bucket policy grants other accounts or IAM identities access to this bucket and its objects." actions={edit(go, "policy-edit")}>
        <Reading q={p.policy} what="The bucket policy" height="h-24">
          {policy ? (
            <JsonBlock value={policy} testId="policy-json" />
          ) : (
            <NoneYet title="No bucket policy" testId="policy-none">{name} has no bucket policy, so only identities of this account whose own policies allow it can reach it.</NoneYet>
          )}
        </Reading>
      </Card>
      <Card title="Object Ownership" lede={own === "BucketOwnerEnforced" || own === null ? "ACLs are disabled, and the bucket owner owns every object." : "ACLs are enabled."} actions={edit(go, "ownership-edit")}>
        <Reading q={p.ownership} what="Object Ownership" height="h-16">
          <Rows rows={[["Object Ownership", own ? OWNERSHIP_WORDS[own] : "Not set (Object writer)"]]} />
        </Reading>
        <div className="mt-5">
          <h3 className="mb-2 text-[15px] font-semibold text-bright">Access control list (ACL)</h3>
          <Reading q={p.acl} what="The ACL" height="h-16">
            <DataTable aria-label="Access control list" data={acl} rowId={(r) => `${r.grantee}|${r.permission}`} maxHeight={240} columns={[{ id: "grantee", header: "Grantee", accessorKey: "grantee", size: 360 }, { id: "permission", header: "Permission", accessorKey: "permission", meta: { mono: true } }]} />
          </Reading>
          <p className="mt-2 text-[13px] text-muted-foreground">{own === "BucketOwnerEnforced" || own === null ? "With Bucket owner enforced, ACLs are off: a request that sets one beyond the owner's full control fails with AccessControlListNotSupported, and reads still work." : "ACLs are kept and returned, and a grant to everyone makes the bucket or an object readable by anyone, unless Block Public Access ignores public ACLs."}</p>
        </div>
      </Card>
      <Card title="Cross-origin resource sharing (CORS)" lede="A CORS configuration is a set of rules that identify the origins you allow to access this bucket and the HTTP methods supported for each origin." actions={edit(go, "cors-edit")}>
        <Reading q={p.cors} what="CORS" height="h-24">
          {rules.length > 0 ? <JsonBlock value={rules} testId="cors-json" /> : <NoneYet title="No CORS configuration" testId="cors-none">A browser page on another origin cannot read from {name}: a preflight request is refused.</NoneYet>}
        </Reading>
      </Card>
    </div>
  );
}
