// KMS's addresses, and what a page reads of a key through the kind's own calls (`resources/kms_ops.rs`).
import { useKindOpRead } from "../../../kit";

export const KEYS = { service: "kms", kind: "keys" } as const;
export const ALIASES = { service: "kms", kind: "aliases" } as const;

export type KeyPolicy = { Policy: string; PolicyName: string };
export type RotationStatus = { KeyRotationEnabled: boolean; RotationPeriodInDays?: number; NextRotationDate?: number; KeyId?: string };
export type Rotation = { KeyMaterialId?: string; KeyMaterialState?: string; RotationDate?: number; RotationType?: string };
export type Rotations = { Rotations: Rotation[]; Truncated?: boolean };
export type Described = { KeyMetadata: { KeyId: string; Arn: string; KeySpec: string; KeyUsage: string; Origin?: string; MultiRegion?: boolean; EncryptionAlgorithms?: string[]; SigningAlgorithms?: string[]; KeyAgreementAlgorithms?: string[]; MacAlgorithms?: string[]; DeletionDate?: number; KeyState: string } };

export const useKeyPolicy = (id: string) => useKindOpRead<KeyPolicy>(KEYS, "GetKeyPolicy", { KeyId: id, PolicyName: "default" });
export const useRotationStatus = (id: string, enabled = true) => useKindOpRead<RotationStatus>(KEYS, "GetKeyRotationStatus", { KeyId: id }, enabled);
export const useRotations = (id: string, enabled = true) => useKindOpRead<Rotations>(KEYS, "ListKeyRotations", { KeyId: id }, enabled);
export const useDescribed = (id: string) => useKindOpRead<Described>(KEYS, "DescribeKey", { KeyId: id });
