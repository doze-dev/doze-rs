/** Whether what was typed is the words asked for: exact (a name is case-sensitive), a trailing space forgiven (a paste often
 * carries one). */
export const confirmed = (typed: string, words: string): boolean => words !== "" && typed.trimEnd() === words;
