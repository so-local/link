import relocations from "./docs/relocations.json";

/** Hashes stay in the browser when a page moves. Resolve moved sections from
 * the same declared relocation data as the server's page redirects. */
export function relocatedFragment(
  pathname: string,
  hash: string,
  search = "",
  language = "en",
): string | null {
  if (!hash) return null;
  const localized = `/${language}/`;
  const page = pathname.startsWith(localized)
    ? pathname.slice(localized.length - 1)
    : pathname;
  const fragments: Record<string, string> = relocations.fragments;
  const destination = fragments[page + hash];
  if (!destination) return null;
  const at = destination.indexOf("#");
  const targetPage = at < 0 ? destination : destination.slice(0, at);
  const targetHash = at < 0 ? "" : destination.slice(at);
  const prefix = language === "en" ? "" : `/${language}`;
  const target = prefix + targetPage + search + targetHash;
  return target === pathname + search + hash ? null : target;
}
