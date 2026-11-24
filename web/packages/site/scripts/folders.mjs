// The folders whose docs make the website an app is, as src/docs.ts
// reads them from scripts/documentation-index.json: the app's own, then
// the index's when they're its, but those marked `"released": false`.
// `web` is the app's folder.

import { readFileSync } from "node:fs";
import { join, normalize, sep } from "node:path";

function definition(web) {
  const repo = join(web, "../../..");
  const index = JSON.parse(readFileSync(join(repo, "scripts/documentation-index.json"), "utf8"));
  if (index.schema !== 1 || !Array.isArray(index.sites)) throw new Error("unsupported documentation index");
  const inRepo = (directory) => {
    const folder = normalize(join(repo, directory));
    if ((folder !== repo && !folder.startsWith(repo + sep)) || directory.split(/[\\/]/).includes("..")) throw new Error("documentation owner escapes the repository");
    return folder;
  };
  const apps = index.sites.map((w) => inRepo(w.directory));
  const own = index.sites[apps.indexOf(normalize(web))];
  if (!own) throw new Error(`${web} is no website of the documentation index`);
  return { index, apps, own, inRepo };
}

export function websiteFolders(web) {
  const { index, apps, own, inRepo } = definition(web);
  const listed = index.roots.filter((r) => r.released !== false).map(({ directory }) => inRepo(directory)).filter((f) => !apps.includes(f));
  return [normalize(web), ...(own.roots ? listed : [])];
}

/** An optional shared homepage, authored once at its repository path. */
export function websiteHomepage(web) {
  const { own, inRepo } = definition(web);
  return own.homepage ? inRepo(own.homepage) : undefined;
}
