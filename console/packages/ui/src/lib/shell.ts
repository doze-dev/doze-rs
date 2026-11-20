// Words for a shell command the page offers to copy. The server's `live_words::shell_word` in TypeScript: a command made on the
// page from data (an address, a topic) quotes every such word, so nothing in it can run when it is pasted into a terminal.
// Commands that carry a client's own words (a client id) are made by the server instead.

const SAFE = /^[A-Za-z0-9._\-:/=@%+,]+$/;

/** A word for a shell: plain when it is made of safe characters, otherwise in single quotes (a `'` inside closes, escapes, reopens). */
export function shellWord(s: string): string {
  return SAFE.test(s) ? s : `'${s.replaceAll("'", "'\\''")}'`;
}
