// What was just done to a function, said as the banner the page lands on (`?done=`): its title and what the call returned.

type Said = { title: string; returned: string };

/** What was just done, said as the board's banner: its title and what the call returned. */
const DONE: Record<string, (name: string) => { title: string; returned: string }> = {
  created: (n) => ({ title: `${n} was created.`, returned: "Its code is the Hello world example; change it here and choose Deploy." }),
  deployed: (n) => ({ title: `The code of ${n} was deployed.`, returned: "UpdateFunctionCode replaced the package of $LATEST; published versions keep the code they had." }),
  updated: (n) => ({ title: `The configuration of ${n} was saved.`, returned: "UpdateFunctionConfiguration changed what you changed; it applies to $LATEST." }),
  env: (n) => ({ title: `The environment variables of ${n} were saved.`, returned: "UpdateFunctionConfiguration replaced the whole variables map." }),
  "trigger-added": () => ({ title: "Trigger added.", returned: "CreateEventSourceMapping made a mapping; Lambda starts polling the queue." }),
  "trigger-saved": () => ({ title: "Trigger saved.", returned: "UpdateEventSourceMapping changed the mapping." }),
  "permission-added": () => ({ title: "Permission added.", returned: "AddPermission put one Allow statement in the resource-based policy." }),
  "destination-saved": () => ({ title: "Destination saved.", returned: "The asynchronous invocation settings of the function were updated." }),
  "url-created": () => ({ title: "A function URL was created for the $LATEST unpublished version.", returned: "The function URL appears in the Function URL section and under the function's name." }),
  "alias-created": () => ({ title: "Alias created.", returned: "CreateAlias pointed the name at the version." }),
  "trigger-deleted": () => ({ title: "Trigger deleted.", returned: "DeleteEventSourceMapping removed the mapping; the function is no longer invoked from it." }),
  "permission-deleted": () => ({ title: "Permission deleted.", returned: "RemovePermission took the statement out of the resource-based policy." }),
  "url-deleted": () => ({ title: "Function URL deleted.", returned: "DeleteFunctionUrlConfig removed the URL; it no longer answers." }),
  "alias-deleted": () => ({ title: "Alias deleted.", returned: "DeleteAlias removed the alias; the version it pointed at stays." }),
  published: () => ({ title: "Version published.", returned: "PublishVersion made an immutable copy of $LATEST." }),
  "layer-added": () => ({ title: "Layer added.", returned: "UpdateFunctionConfiguration set the function's layers." }),
};

/** The version PublishVersion answered with when nothing had changed (`unchanged-3`): it made no version, and the banner says so. */
export const unchangedKey = (version: string): string => `unchanged-${version}`;

/** The banner for `?done=<key>` on the function `name`; `undefined` for a key that says nothing. */
export function doneOf(key: string, name: string): Said | undefined {
  const same = /^unchanged-(\d+)$/.exec(key)?.[1];
  if (same) return { title: "No new version was made.", returned: `PublishVersion answered with version ${same}: nothing changed since it, so Lambda made no new one and the list is as it was.` };
  return DONE[key]?.(name);
}
