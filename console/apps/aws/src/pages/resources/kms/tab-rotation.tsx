// The Key material and rotations tab (kms__key-rotation, key-rotations-none): what GetKeyRotationStatus reports, Rotate now, and the
// rotations ListKeyRotations returns, newest first.
import { useQueryClient } from "@tanstack/react-query";
import { PencilIcon, RefreshCwIcon, RotateCwIcon } from "lucide-react";
import { useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { DataTable, type DataColumn } from "@doze/ui/components/app/data-table";
import { EmptyTab } from "@doze/ui/components/app/empty-tab";
import { ResultBanner } from "@doze/ui/components/app/result-banner";
import { Button } from "@doze/ui/components/ui/button";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { dateTimeUtc } from "@doze/ui/lib/format";
import { callKindOp } from "../../../kit";
import { Card, Note } from "../cfg-parts";
import { KEYS, type Rotation, type RotationStatus } from "./api";
import { refusalOf } from "./schedule";
import type { KeyRow } from "./model";

const TYPES: Record<string, string> = { ON_DEMAND: "On-demand", AUTOMATIC: "Automatic" };
const COLUMNS: DataColumn<Rotation>[] = [
  { id: "date", header: "Rotation date", accessorFn: (r) => r.RotationDate ?? 0, cell: (c) => (c.row.original.RotationDate ? dateTimeUtc(c.row.original.RotationDate * 1000) : "–") },
  { id: "type", header: "Rotation type", accessorFn: (r) => r.RotationType ?? "", cell: (c) => TYPES[c.row.original.RotationType ?? ""] ?? c.row.original.RotationType ?? "–" },
];

export function TabRotation({ id, kind, state, status, rotations, loading, go }: { id: string; kind: KeyRow["kind"]; state: string; status: RotationStatus | null; rotations: Rotation[] | null; loading: boolean; go: () => void }) {
  const qc = useQueryClient();
  const [pending, setPending] = useState(false);
  const [done, setDone] = useState(false);
  const [refused, setRefused] = useState<{ code: string; message: string } | null>(null);
  const why = kind !== "Symmetric" ? "Only a symmetric key rotates." : state === "PendingDeletion" ? "A key pending deletion cannot be rotated." : null;
  const rotate = async () => {
    setPending(true);
    setRefused(null);
    setDone(false);
    try {
      await callKindOp(KEYS, "RotateKeyOnDemand", { KeyId: id });
      setDone(true);
      await qc.invalidateQueries({ queryKey: ["resources", "kindop", "kms", "keys"] });
      await qc.invalidateQueries({ queryKey: ["resources", "one", "kms", "keys"] });
    } catch (e) {
      setRefused(refusalOf(e));
    } finally {
      setPending(false);
    }
  };
  if (kind !== "Symmetric") return <EmptyTab icon={RotateCwIcon} title="This key cannot rotate" testId="rotation-none">Only symmetric encryption keys with key material from KMS can be rotated. {kind === "Asymmetric" ? "An asymmetric key pair keeps the material it was made with." : "An HMAC key keeps the material it was made with."}</EmptyTab>;
  if (loading || !status || !rotations) return <Skeleton className="h-56 rounded-[14px]" data-testid="rotation-loading" />;
  const rotateNow = <Button type="button" variant="outline" className="bg-card" onClick={() => void rotate()} disabled={pending || why !== null} title={why ?? undefined} data-testid="rotate-now"><RefreshCwIcon /> {pending ? "Rotating…" : "Rotate now"}</Button>;
  const result = refused ? <Alert tone="bad" title={`RotateKeyOnDemand was refused: ${refused.code}`} testId="rotate-refused">{refused.message}</Alert> : done ? <ResultBanner title="Key material rotated." returned="RotateKeyOnDemand made new key material current; the older material stays to decrypt what it encrypted." onDismiss={() => setDone(false)} testId="rotated-banner" /> : null;
  const edit = <Button type="button" variant="outline" className="bg-card" onClick={go} disabled={state === "PendingDeletion"} data-testid="rotation-edit"><PencilIcon /> {rotations.length === 0 ? "Edit rotation" : "Edit"}</Button>;
  return (
    <div className="grid gap-5" data-testid="rotation-tab">
      {result}
      {rotations.length === 0 ? (
        <EmptyTab icon={RotateCwIcon} title="No rotations yet" action={<>{edit}{rotateNow}</>} testId="rotations-none">This key has not been rotated. Its key material is the material it was created with. Automatic rotation is {status.KeyRotationEnabled ? `on, every ${status.RotationPeriodInDays ?? 365} days` : "off"}.</EmptyTab>
      ) : (
        <>
          <div className="grid items-start gap-5 lg:grid-cols-2">
            <Card title="Automatic key rotation" lede="What GetKeyRotationStatus reports for this key." actions={edit} testId="auto-rotation">
              <p className="font-mono text-[13.5px] text-bright" data-testid="rotation-enabled">KeyRotationEnabled <span className="ml-1 font-sans text-muted-foreground">{String(status.KeyRotationEnabled)}</span></p>
              {status.KeyRotationEnabled ? <p className="font-mono text-[13.5px] text-bright">RotationPeriodInDays <span className="ml-1 font-sans text-muted-foreground">{status.RotationPeriodInDays}</span></p> : <p className="text-[13.5px] text-muted-foreground"><span className="font-mono text-bright">RotationPeriodInDays</span> is not reported while rotation is off. It is 365 days when you enable rotation without setting one.</p>}
            </Card>
            <Card title="On-demand rotation" lede={`Rotate now, whether or not automatic rotation is on. ListKeyRotations shows ${rotations.length} completed rotation${rotations.length === 1 ? "" : "s"}.`} testId="on-demand">
              <div>{rotateNow}</div>
            </Card>
          </div>
          <Card title="Key materials" lede="The completed rotations ListKeyRotations returns, newest first." testId="key-materials">
            <DataTable aria-label="Rotations" data={rotations} columns={COLUMNS} rowId={(r) => r.KeyMaterialId ?? String(r.RotationDate)} maxHeight={320} />
          </Card>
        </>
      )}
      <Note testId="rotation-note">Rotation changes only the <b>current key material</b>. The key ID, ARN, aliases and policies stay the same, and KMS keeps the older material to decrypt what it encrypted. AWS limits on-demand rotation to 25 times per key; doze-aws does not count or enforce that limit. The automatic setting and period are stored, not applied locally: nothing on this machine rotates on a schedule, so Rotate now is the switch.</Note>
    </div>
  );
}

