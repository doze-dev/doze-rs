import logo from "@/assets/doze-kafka.svg";
import { cn } from "@/lib/utils";

/** The doze Kafka mark: the sleeping bat on its yellow tile (doze-website/public/brand/doze-kafka.svg). `swing`: the bat swings
 * on its perch when its link is hovered, as on doze-dev.com (no motion when the person asks for less). */
export function Logo({ size, swing = false, className }: { size: number; swing?: boolean; className?: string }) {
  return (
    <img
      src={logo}
      alt=""
      width={size}
      height={size}
      draggable={false}
      className={cn("shrink-0 select-none", swing && "transition-transform duration-[600ms] ease-[cubic-bezier(.2,.8,.2,1)] motion-safe:group-hover/brand:-rotate-12 motion-safe:group-hover/brand:scale-[1.06]", className)}
      style={{ width: size, height: size }}
    />
  );
}

/** The name as the brand writes it: "doze" in ink, "Kafka" in the accent, in the family's display face (Bricolage Grotesque,
 * as on doze-dev.com). Read as one word by assistive tech. */
export function Wordmark({ className }: { className?: string }) {
  return (
    <span className={cn("font-display font-extrabold tracking-[-0.025em] whitespace-nowrap", className)}>
      <span className="sr-only">doze Kafka</span>
      <span aria-hidden className="text-bright">
        doze
      </span>
      <span aria-hidden className="text-link">
        {" "}
        Kafka
      </span>
    </span>
  );
}
