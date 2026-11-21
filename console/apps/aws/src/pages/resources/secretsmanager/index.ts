// Secrets Manager's page of a secret: a secret marked for deletion says so on its details and offers the way back.
import { lazy } from "react";
import { register, registerLead } from "../deep/registry";

// Never listed as a tab (`when` is never true): the descriptor names RestoreSecret on this kind so that the banner on the details may make it.
register("secretsmanager", { restore: { page: lazy(() => import("./marked").then((m) => ({ default: m.SecretMarked }))), when: () => false } });
registerLead("secretsmanager", "secrets", lazy(() => import("./marked").then((m) => ({ default: m.SecretMarked }))));
