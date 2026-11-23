// What the node panel says in big type: a queue's headline is how many messages are waiting (`2` over `waiting`), with the rest of
// what it holds under it (`1 in flight · hidden 30 s on receive`). Pure: read from the sentence the services gave the node.

/** `2 waiting · 1 in flight` as `{ count: "2", rest: "1 in flight" }`; null when the sentence does not begin with a waiting count. */
export function waitingHeadline(sentence: string | null): { count: string; rest: string } | null {
  const m = /^([\d,]+) waiting(?: · (.*))?$/.exec(sentence ?? "");
  return m ? { count: m[1]!, rest: m[2] ?? "" } : null;
}
