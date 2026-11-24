import type { ProbeView } from "@/api/types/ProbeView";

/** Under the Address: which flag the address comes from. When the broker advertises an address other than the one it
 *  listens on (under doze the listener is the engine's private backend, a port chosen afresh at each start), the
 *  address shown is the advertised one and the bind is said here, with why a client should not use it. */
export function AddressNote({ probe, loopback }: { probe: ProbeView; loopback: boolean }) {
  if (probe.same) {
    return (
      <p className="text-[12.5px] text-muted-foreground" data-testid="address-note">
        The listener, set by <code className="font-mono">--listen</code>.{" "}
        {loopback ? "On loopback it is reachable from this machine only." : "It is not on loopback, so other machines can reach it."}
      </p>
    );
  }
  return (
    <p className="text-[12.5px] text-muted-foreground" data-testid="address-note">
      The advertised address, set by <code className="font-mono">--advertised-host</code> and{" "}
      <code className="font-mono">--advertised-port</code>. The broker itself listens on{" "}
      <code className="font-mono">{probe.listen}</code> (<code className="font-mono">--listen</code>): a client pointed
      there goes around whatever stands in front of the broker, such as doze.
    </p>
  );
}
