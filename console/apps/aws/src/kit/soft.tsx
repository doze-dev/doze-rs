import { Fragment } from "react";

/** The places a text may be broken at: after each comma. */
export const softParts = (text: string): string[] => text.split(/(?<=,)/);

/** The text with a place to break after each comma, so a long JSON message or list wraps between its members and only a member longer than the
 * column is cut inside (the cell needs `break-words`, which breaks a word only when it is the only way). */
export function Soft({ text }: { text: string }) {
  const parts = softParts(text);
  return <>{parts.map((p, i) => <Fragment key={i}>{p}{i < parts.length - 1 ? <wbr /> : null}</Fragment>)}</>;
}
