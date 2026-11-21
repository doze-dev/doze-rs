// What a filter pattern is, said once for the stream viewer, the search and the metric filter's pattern field.

const code = (t: string) => <span className="font-mono text-[12.5px] text-foreground">{t}</span>;

/** The teaching under a field that takes a filter pattern; `what` is the field (`The search filter on a stream`). doze-aws reads term, JSON and
 * space-delimited patterns, with regular expressions in `%…%` (crates/aws-logs/src/pattern). */
export const PATTERN_TEACHING = (what: string) => (
  <>
    {what} is a <b>filter pattern</b>, not a free-text search: {code("ERROR ARGUMENTS")} needs both terms, {code("?ERROR ?ARGUMENTS")} either, {code("ERROR -ARGUMENTS")} excludes the second, and {code('{ $.level = "error" }')} reads a JSON field. Patterns are case-sensitive.{" "}
    <span className="text-muted-foreground">Regular-expression ({code("%…%")}) and space-delimited ({code("[…]")}) patterns work in doze-aws within AWS's limits; a pattern it cannot read is refused with InvalidParameterException.</span>
  </>
);

/** The examples the search page lists: the pattern and what it matches. */
export const EXAMPLES: [string, string][] = [
  ["ERROR ARGUMENTS", "both terms"],
  ["?ERROR ?ARGUMENTS", "either term"],
  ["ERROR -ARGUMENTS", "without the second term"],
  ['{ $.level = "error" }', "a JSON field"],
];

/** The note under the search page's examples. */
export const EXAMPLES_NOTE = (
  <>Patterns are case-sensitive. <span className="text-muted-foreground">Regular expressions ({code("%…%")}) and space-delimited patterns ({code("[…]")}) work in doze-aws within AWS's limits.</span></>
);
