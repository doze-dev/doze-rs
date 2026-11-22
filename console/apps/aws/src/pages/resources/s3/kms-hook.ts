import { useMemo } from "react";
import { useAbout } from "../../../api/queries";
import { useResourceList } from "../../../api/resources";
import { kmsKeys, type KmsKeys } from "./kms-keys";

/** The keys S3 can encrypt with (and those it cannot, with why), from the console's own key and alias lists; the first page of each. */
export function useKmsKeys(): { keys: KmsKeys; pending: boolean; failed: boolean } {
  const keys = useResourceList("kms", "keys");
  const aliases = useResourceList("kms", "aliases");
  const about = useAbout().data;
  const rows = keys.data?.pages.flatMap((p) => p.rows);
  const named = aliases.data?.pages.flatMap((p) => p.rows);
  const read = useMemo(() => kmsKeys(rows ?? [], named ?? [], about?.region ?? "us-east-1", about?.account ?? ""), [rows, named, about]);
  return { keys: read, pending: keys.isPending || aliases.isPending || !about, failed: keys.isError };
}
