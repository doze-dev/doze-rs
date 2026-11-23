// The list's Key actions: Enable, Disable, Schedule key deletion and Cancel key deletion, each made on every key chosen, one call
// a key, stopping at the first the service refuses.
import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { errorWords } from "@doze/ui/components/app/alert";
import { callKindOp } from "../../../kit";
import { KEYS } from "./api";
import type { KeyAction } from "./model";

const OPERATION: Record<KeyAction, string> = { enable: "EnableKey", disable: "DisableKey", schedule: "ScheduleKeyDeletion", cancel: "CancelKeyDeletion" };

export type Done = { action: KeyAction; ids: string[]; refused: { id: string; code: string; message: string } | null };

/** Makes `action` on `ids`; the list is read again after. */
export function useKeyActions() {
  const qc = useQueryClient();
  const [pending, setPending] = useState(false);
  const run = async (action: KeyAction, ids: string[], input: object = {}): Promise<Done> => {
    setPending(true);
    const done: string[] = [];
    let refused: Done["refused"] = null;
    try {
      for (const id of ids) {
        try {
          await callKindOp(KEYS, OPERATION[action], { KeyId: id, ...input });
          done.push(id);
        } catch (e) {
          const w = errorWords(e);
          refused = { id, code: w.code ?? "Error", message: w.message };
          break;
        }
      }
    } finally {
      setPending(false);
      void qc.invalidateQueries({ queryKey: ["resources", "list", "kms", "keys"] });
      void qc.invalidateQueries({ queryKey: ["resources", "one", "kms", "keys"] });
    }
    return { action, ids: done, refused };
  };
  return { run, pending };
}
