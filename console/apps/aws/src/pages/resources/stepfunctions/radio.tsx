// Rows chosen one at a time, by a radio at the left of the row: the Versions and Aliases tabs, where Start execution acts on the chosen one.
import { type DataColumn } from "@doze/ui/components/app/data-table";
import { RadioGroup, RadioGroupItem } from "@doze/ui/components/ui/radio-group";

export function RadioRows({ value, onChange, children }: { value: string | null; onChange: (v: string) => void; children: React.ReactNode }) {
  return <RadioGroup value={value ?? ""} onValueChange={onChange} aria-label="Choose a row" className="block">{children}</RadioGroup>;
}

export function radioColumn<T>(id: (r: T) => string, noun: string): DataColumn<T> {
  return { id: "choose", size: 44, enableSorting: false, header: () => <span className="sr-only">Choose</span>, cell: (c) => <RadioGroupItem value={id(c.row.original)} aria-label={`Choose ${noun} ${id(c.row.original)}`} data-testid={`choose-${id(c.row.original)}`} /> };
}
