// What a refused PartiQL statement means, in plain words, to go beside the service's own message (which stays: it is what a script
// would match). Pure, so it is tested without a browser.

/** The table a statement names (`UPDATE "t" SET`, `DELETE FROM "t"`, `INSERT INTO "t"`, `SELECT * FROM "t"`), or null. */
export function tableOf(statement: string): string | null {
  const m = /^\s*(?:UPDATE|DELETE\s+FROM|INSERT\s+INTO|SELECT\b[\s\S]*?\bFROM)\s+(?:"([^"]+)"|([\w.-]+))/i.exec(statement);
  return m ? (m[1] ?? m[2] ?? null) : null;
}

/** The attributes a statement's WHERE clause compares (`order_id`, `created_at`), in order. */
export function whereNames(statement: string): string[] {
  const where = /\bWHERE\b([\s\S]*)$/i.exec(statement)?.[1] ?? "";
  const names = [...where.matchAll(/(?:"([^"]+)"|\b([A-Za-z_][\w.]*))\s*=/g)].map((m) => m[1] ?? m[2] ?? "");
  return [...new Set(names.filter(Boolean))];
}

const list = (xs: string[]) => (xs.length <= 1 ? xs.join("") : `${xs.slice(0, -1).join(", ")} and ${xs[xs.length - 1]}`);

/** A sentence for the refusal `code` of `statement`, or null when there is nothing to add to the service's words. */
export function partiqlWords(code: string, statement: string): string | null {
  const table = tableOf(statement);
  const verb = /^\s*(\w+)/.exec(statement)?.[1]?.toUpperCase() ?? "";
  const keys = whereNames(statement);
  switch (code) {
    case "ConditionalCheckFailedException": {
      const what = verb === "DELETE" ? "delete" : "update";
      return table && keys.length > 0 ? `No item in ${table} has that ${list(keys)}, so there was nothing to ${what}.` : `The item the statement names was not there to ${what}, so nothing was changed.`;
    }
    case "DuplicateItemException":
      return table ? `${table} already has an item with that key. INSERT never replaces an item; UPDATE changes one.` : "An item with that key already exists. INSERT never replaces an item; UPDATE changes one.";
    case "ResourceNotFoundException":
      return table ? `${table} is not a table in this account and Region. Table names go in double quotes.` : "The statement names a table that is not in this account and Region.";
    case "ValidationException":
      return "The statement is not valid for this table. Table and attribute names take double quotes, string values single quotes, and an UPDATE or DELETE must name the whole primary key in its WHERE clause.";
    default:
      return null;
  }
}
