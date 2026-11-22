// Secrets Manager's addresses, and what a page reads of a secret through the kind's own calls (`resources/secrets_ops.rs`).
import { useKindOpRead } from "../../../kit";
import type { Described, Version } from "./model";

export const SECRETS = { service: "secretsmanager", kind: "secrets" } as const;

export type SecretValue = { SecretString?: string; SecretBinary?: string; VersionId: string; VersionStages?: string[]; CreatedDate?: number };

export const useDescribed = (name: string) => useKindOpRead<Described>(SECRETS, "DescribeSecret", { SecretId: name });
export const useVersions = (name: string, enabled = true) => useKindOpRead<{ Versions: { VersionId?: string; VersionStages?: string[]; CreatedDate?: number }[] }>(SECRETS, "ListSecretVersionIds", { SecretId: name }, enabled);
export const usePolicy = (name: string, enabled = true) => useKindOpRead<{ ResourcePolicy?: string }>(SECRETS, "GetResourcePolicy", { SecretId: name }, enabled);

export type { Described, Version };
