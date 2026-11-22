// The KMS key a secret or a parameter is encrypted with, chosen by alias: the customer managed keys that have one, or the service's own
// default key. Reads the aliases list the console reads for the Aliases of KMS.
import { Combobox, type Option } from "@doze/ui/components/app/combobox";
import { useAbout } from "../../api/queries";
import { useAllRows } from "./cfg-api";

export function KeyAliasPicker({ value, onChange, defaultLabel, id }: { value: string | null; onChange: (v: string | null) => void; /** `aws/secretsmanager`: the key used when none is chosen. */ defaultLabel: string; id?: string }) {
  const served = useAbout().data?.services.some((s) => s.name === "kms" && s.served) ?? false;
  const aliases = useAllRows("kms", "aliases", { enabled: served });
  const options: Option[] = [{ value: "", label: `${defaultLabel} (default)` }, ...aliases.rows.filter((r) => !r.name.startsWith("alias/aws/")).map((r) => ({ value: r.name, label: r.name })).sort((a, b) => a.label.localeCompare(b.label))];
  return <Combobox options={options} value={value ?? ""} onChange={(v) => onChange(v ? v : null)} placeholder="Choose a key" search="Search keys…" empty="No key matches." id={id} aria-label="Encryption key" className="w-full max-w-xl bg-card font-mono" />;
}
