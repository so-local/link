/** The languages a page may be translated into, read once from the list. */
export function languages(): string[];
/** Whether `name` is a translation's language: `es`, or `pt-BR`. */
export function isLanguage(name: string): boolean;
/** A translation's language, from its file name: guide.es.md is es, foo.js.md none. */
export function translationLanguage(name: string): string | undefined;
