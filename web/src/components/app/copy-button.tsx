import { CheckIcon, CopyIcon } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { copyText } from "./copy";

/** A Copy button that says Copied for 1.4 s in the width it had (nothing beside it moves). */
export function CopyButton({ text, label = "Copy", what, className, size = "sm", variant = "outline", iconOnly = false }: { text: string; label?: string; what?: string; className?: string; size?: "sm" | "default" | "icon-sm"; variant?: "outline" | "ghost"; iconOnly?: boolean }) {
  const [done, setDone] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  useEffect(() => () => clearTimeout(timer.current), []);
  return (
    <Button
      type="button"
      variant={variant}
      size={iconOnly ? "icon-sm" : size}
      className={cn(!iconOnly && "min-w-[5.5rem]", className)}
      aria-label={iconOnly ? `${label}: ${text}` : undefined}
      onClick={async () => {
        if (await copyText(text, what)) {
          setDone(true);
          clearTimeout(timer.current);
          timer.current = setTimeout(() => setDone(false), 1400);
        }
      }}
    >
      {done ? <CheckIcon className="text-good" /> : <CopyIcon />}
      {!iconOnly && <span>{done ? "Copied" : label}</span>}
    </Button>
  );
}
