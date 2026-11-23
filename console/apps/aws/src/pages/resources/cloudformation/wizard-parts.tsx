// What the wizard's steps share: choices as cards (one of them off, with its reason), and a card with a title the way the boards' sections are.
import { Label } from "@doze/ui/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@doze/ui/components/ui/radio-group";
import { cn } from "@doze/ui/lib/utils";

export type Choice<T extends string> = { value: T; title: React.ReactNode; body?: React.ReactNode; /** Off: the words say why. */ off?: boolean };

/** Choices side by side, each a card with a radio, a title and what it means; one that is `off` cannot be chosen. */
export function Choices<T extends string>({ value, onChange, options, label, name, columns = 2, rows = false }: { value: T; onChange: (v: T) => void; options: readonly Choice<T>[]; label: string; name: string; columns?: 2 | 3; /** The board draws these as a plain list of radios, not as cards. */ rows?: boolean }) {
  if (rows) {
    return (
      <RadioGroup value={value} onValueChange={(v) => onChange(v as T)} className="gap-4" aria-label={label}>
        {options.map((o) => (
          <Label key={o.value} htmlFor={`${name}-${o.value}`} className={cn("flex items-start gap-3 font-normal leading-5", o.off ? "cursor-not-allowed" : "cursor-pointer")} data-testid={`${name}-card-${o.value}`}>
            <RadioGroupItem id={`${name}-${o.value}`} value={o.value} disabled={o.off} className="mt-0.5" data-testid={`${name}-${o.value}`} />
            <span className="grid gap-0.5"><span className={cn("font-medium", o.off ? "text-muted-foreground" : "text-bright")}>{o.title}</span>{o.body && <span className="text-[13px] text-muted-foreground">{o.body}</span>}</span>
          </Label>
        ))}
      </RadioGroup>
    );
  }
  return (
    <RadioGroup value={value} onValueChange={(v) => onChange(v as T)} className={cn("grid gap-3", columns === 3 ? "sm:grid-cols-3" : "sm:grid-cols-2")} aria-label={label}>
      {options.map((o) => (
        <Label key={o.value} htmlFor={`${name}-${o.value}`} className={cn("flex items-start gap-3 rounded-xl border bg-card p-4 font-normal leading-5", o.off ? "cursor-not-allowed opacity-80" : "cursor-pointer has-[[data-state=checked]]:border-primary has-[[data-state=checked]]:bg-pick/40")} data-testid={`${name}-card-${o.value}`}>
          <RadioGroupItem id={`${name}-${o.value}`} value={o.value} disabled={o.off} className="mt-0.5" data-testid={`${name}-${o.value}`} />
          <span className="grid gap-0.5">
            <span className={cn("font-medium", o.off ? "text-muted-foreground" : "text-bright")}>{o.title}</span>
            {o.body && <span className="text-[13px] text-muted-foreground">{o.body}</span>}
          </span>
        </Label>
      ))}
    </RadioGroup>
  );
}

/** A sentence under a field set apart by a rule at its left, its first words in bold (`Stored, not applied locally.`). */
export function Note({ lead, children, testId }: { lead?: string; children: React.ReactNode; testId?: string }) {
  return (
    <p className="border-l-2 pl-3 text-[13px] leading-5 text-muted-foreground" data-testid={testId}>
      {lead && <b className="font-semibold text-foreground">{lead} </b>}
      {children}
    </p>
  );
}

/** The bucket and key an S3 URL names (virtual-hosted, path-style or `s3://`), as doze-aws reads them; `null` for another URL. */
export function s3Of(url: string): { bucket: string; key: string } | null {
  const t = url.trim();
  const plain = /^s3:\/\/([^/]+)\/(.+)$/.exec(t);
  if (plain) return { bucket: plain[1]!, key: plain[2]! };
  const virtual = /^https?:\/\/([^./]+)\.s3[.-][^/]*amazonaws\.com\/([^?#]+)/.exec(t);
  if (virtual) return { bucket: virtual[1]!, key: virtual[2]! };
  const path = /^https?:\/\/[^/]+\/([^/]+)\/([^?#]+)/.exec(t);
  return path ? { bucket: path[1]!, key: path[2]! } : null;
}
