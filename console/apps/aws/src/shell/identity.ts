// The role session this browser holds (Switch Role, STS): the credentials AssumeRole gave and how the person asked to see them, and the
// roles used recently. Kept in this browser's local storage only; the console's own calls are never made with them. A small store, so the top
// bar's chip and the Switch Role page see the same session.
import { useSyncExternalStore } from "react";

export type SessionColor = "red" | "green";
export type Session = { roleArn: string; role: string; account: string; display: string; color: SessionColor | null; accessKeyId: string; secretAccessKey: string; sessionToken: string; expiresAt: number };
type State = { session: Session | null; recent: { role: string; account: string; display: string }[]; open: boolean };

const KEY = "doze-aws.session";
const MOST_RECENT = 5;

function read(): State {
  try {
    const v = JSON.parse(localStorage.getItem(KEY) ?? "null") as Partial<State> | null;
    return { session: v?.session ?? null, recent: v?.recent ?? [], open: false };
  } catch {
    return { session: null, recent: [], open: false };
  }
}

let state: State = read();
const listeners = new Set<() => void>();
const emit = () => {
  for (const l of listeners) l();
};
const save = () => {
  try {
    localStorage.setItem(KEY, JSON.stringify({ session: state.session, recent: state.recent }));
  } catch {
    // storage full or blocked: the session lives for this page
  }
};

/** Takes a role session: it becomes the identity, and the role joins the recent ones. */
export function switchTo(session: Session): void {
  const seen = { role: session.role, account: session.account, display: session.display };
  state = { session, recent: [seen, ...state.recent.filter((r) => !(r.role === seen.role && r.account === seen.account))].slice(0, MOST_RECENT), open: true };
  save();
  emit();
}

/** Back to the person's own identity. */
export function switchBack(): void {
  state = { ...state, session: null, open: false };
  save();
  emit();
}

export function setIdentityOpen(open: boolean): void {
  state = { ...state, open };
  emit();
}

export function useIdentity(): State {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => state,
  );
}

/** Whether the session has run out. */
export const expired = (s: Session, now = Date.now()): boolean => s.expiresAt <= now;
