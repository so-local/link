// Section outlines stay structural metadata and retain one authored order.
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { dirname, join, posix } from "node:path";
import type { Link, List, Root } from "mdast";
import { toString } from "mdast-util-to-string";
import { translationLanguage } from "./languages.mjs";

/** A translation's suffix: navigation.es.md is .es.md, navigation.md none. */
const suffixOf = (name: string) => {
  const language = translationLanguage(name);
  return language === undefined ? undefined : `.${language}.md`;
};

/**
 * Read one root outline, expanding its local top-level section includes:
 * a translated outline's from each section's translation beside it, where
 * there is one.
 */
export function readOutline(path: string, fallback: string, parse: (text: string) => Root): Root {
  const root = existsSync(/*turbopackIgnore: true*/ path) ? path : fallback;
  const suffix = suffixOf(root);
  const tree = parse(readFileSync(/*turbopackIgnore: true*/ root, "utf8"));
  const listOf = (tree: Root): List => {
    const at = tree.children.findIndex((node) => node.type === "heading" && node.depth === 2 && toString(node) === "Pages");
    const list = tree.children.slice(at + 1).find((node) => node.type === "list" || node.type === "heading");
    if (at < 0 || list?.type !== "list") throw new Error(`${root}: missing Pages outline`);
    return list;
  };
  const linkOf = (item: List["children"][number]): Link | undefined => {
    const first = item.children[0];
    return first?.type === "paragraph" ? first.children.find((node): node is Link => node.type === "link") : undefined;
  };
  const rebase = (list: List, directory: string) => {
    for (const item of list.children) {
      const link = linkOf(item);
      if (link?.title === "include") throw new Error("outline includes must be top-level sections, never nested");
      if (link && !/^(?:[a-z][a-z+.-]*:|\/|#)/i.test(link.url)) link.url = posix.normalize(posix.join(directory, link.url));
      const nested = item.children.find((node): node is List => node.type === "list");
      if (nested) rebase(nested, directory);
    }
  };
  const outline = listOf(tree);
  const seen = new Set<string>();
  outline.children = outline.children.flatMap((item) => {
    const link = linkOf(item);
    if (link?.title !== "include") {
      const nested = item.children.find((node): node is List => node.type === "list");
      if (nested) rebase(nested, ".");
      return [item];
    }
    if (!/^navigation\/[^/]+\.md$/.test(link.url) || item.children.some((node) => node.type === "list")) {
      throw new Error("outline include must name navigation/<section>.md and have no children");
    }
    if (seen.has(link.url)) throw new Error(`duplicate outline include ${link.url}`);
    seen.add(link.url);
    const own = suffix ? join(dirname(root), link.url.replace(/\.md$/, suffix)) : undefined;
    const file = own && existsSync(/*turbopackIgnore: true*/ own) ? own : join(dirname(root), link.url);
    const included = listOf(parse(readFileSync(/*turbopackIgnore: true*/ file, "utf8")));
    if (included.children.length !== 1 || toString(linkOf(included.children[0])) !== toString(link)) {
      throw new Error(`${file} must contain one top-level ${toString(link)} section`);
    }
    rebase(included, posix.dirname(link.url));
    return included.children;
  });
  const directory = join(dirname(root), "navigation");
  if (existsSync(/*turbopackIgnore: true*/ directory)) {
    for (const name of readdirSync(/*turbopackIgnore: true*/ directory)) {
      if (name.endsWith(".md") && !seen.has(`navigation/${name.replace(suffixOf(name) ?? ".md", ".md")}`)) throw new Error(`unreferenced outline ${name}`);
    }
  }
  return tree;
}
