import type { LabOnceTeach } from "../../api/types/LabOnceTeach";
import type { LabSetting } from "../../api/types/LabSetting";
import type { LabTeach } from "../../api/types/LabTeach";

function Settings({ settings, side }: { settings: { name: string; default: string; why: string; whose: string }[]; side: string }) {
  if (settings.length === 0) return null;
  return (
    <div>
      <h4 className="text-[12px] font-medium tracking-[0.06em] text-muted-foreground uppercase">{side}</h4>
      <dl className="mt-1.5 grid gap-2">
        {settings.map((s) => (
          <div key={`${s.whose}-${s.name}`} className="grid gap-0.5">
            <dt className="text-[13px] text-foreground"><span className="font-mono text-[12.5px]">{s.name}</span> <span className="text-muted-foreground">({s.whose}), default {s.default}</span></dt>
            <dd className="text-[12.5px] leading-5 text-muted-foreground">{s.why}</dd>
          </div>
        ))}
      </dl>
    </div>
  );
}

const ofRule = (s: LabSetting) => ({ name: s.name, default: s.default, why: s.why, whose: s.sdk });

/** The lesson of a rule's rehearsal: what it stands for, what to expect, what a client should do, the SDK settings with their defaults,
 * and where to see it. All of it is the server's words. */
export function RuleLesson({ teach }: { teach: LabTeach }) {
  return (
    <div className="grid gap-3 text-[13px] leading-5 text-foreground" data-testid="lesson">
      <p><b className="font-semibold">In production: </b>{teach.simulates}</p>
      <p><b className="font-semibold">What to expect: </b>{teach.expect}</p>
      <div>
        <b className="font-semibold">A well-behaved client:</b>
        <ul className="mt-1 list-disc pl-5 text-muted-foreground">{teach.client.map((c) => <li key={c}>{c}</li>)}</ul>
      </div>
      <Settings settings={teach.settings.map(ofRule)} side="SDK settings and their defaults" />
      <p className="text-muted-foreground"><b className="font-semibold text-foreground">Where to see it: </b>{teach.watch}</p>
    </div>
  );
}

/** The lesson of a forced event. */
export function OnceLesson({ teach }: { teach: LabOnceTeach }) {
  return (
    <div className="grid gap-3 text-[13px] leading-5 text-foreground" data-testid="lesson">
      <p><b className="font-semibold">In production: </b>{teach.production}</p>
      <p><b className="font-semibold">What to expect: </b>{teach.expect}</p>
      <p><b className="font-semibold">A well-behaved client: </b><span className="text-muted-foreground">{teach.client}</span></p>
      <Settings settings={teach.settings.map((s) => ({ name: s.name, default: s.default, why: s.why, whose: s.side }))} side="Settings and their defaults" />
      <p className="text-muted-foreground"><b className="font-semibold text-foreground">Where to see it: </b>{teach.see}</p>
    </div>
  );
}
