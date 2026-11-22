// A refused call: the service's own words for the rule that was broken (the record keeps them), and what would pass. The words are
// the service's; what would pass is said from the error code, the nearest thing that is true of every call that gets it.
import type { CallRecord } from "../../api/types/CallRecord";

const RULES: [RegExp, string][] = [
  [/AlreadyExists|NameExists|Conflict|InUse|BucketAlreadyOwned/i, "The name is taken. Use another, or open the one that is there (Resources lists it)."],
  [/DoesNotExist|NotFound|NoSuch|NonExistent|Unknown/i, "Nothing has that name. Create it first, or call the name of one that exists (Resources lists them)."],
  [/Throttl|TooMany|LimitExceeded|Quota/i, "Too many requests, or over a limit. Send fewer, or raise the limit the message names."],
  [/AccessDenied|Unauthorized|Forbidden|SignatureDoesNotMatch|InvalidClientTokenId|ExpiredToken/i, "The credentials were not accepted. Check the access key and what its policies allow (Access explains a denial)."],
  [/InvalidParameter|Validation|InvalidInput|Malformed|InvalidArgument|BadRequest|InvalidRequest|MissingParameter|InvalidQueryParameter/i, "A value is outside what the service accepts. Change the one the message names to one that is inside the limit it states."],
];

/** What would pass, from the error code (then the status); a sentence for any refusal. */
export function whatWouldPass(c: Pick<CallRecord, "error_code" | "status">): string {
  const code = c.error_code ?? "";
  const hit = RULES.find(([re]) => re.test(code));
  if (hit) return hit[1];
  return c.status === 404 ? RULES[1]![1] : "The service's words say what it wants: change the request to match them and send it again.";
}
