// Markdown formatting has no filesystem, bucket, application or routing dependency.
import type { Element, Root as Hast } from "hast";
import type { Root } from "mdast";
import { toString } from "mdast-util-to-string";
import rehypeSlug from "rehype-slug";
import remarkDirective from "remark-directive";
import remarkFrontmatter from "remark-frontmatter";
import remarkGfm from "remark-gfm";
import remarkParse from "remark-parse";
import remarkRehype from "remark-rehype";
import remarkStringify from "remark-stringify";
import { unified } from "unified";
import { parseDocument } from "yaml";
import { opening } from "./overview";

export type PageMetadata = { title: string; description: string; image?: string };
export type PageReference = PageMetadata & { href: string };

export const markdown = unified().use(remarkParse).use(remarkGfm).use(remarkFrontmatter).use(remarkDirective).use(remarkStringify);
const html = unified().use(remarkRehype).use(rehypeSlug);

/** Explicit page metadata overrides the heading and introduction defaults. */
export function metadata(tree: Root): PageMetadata {
  const heading = tree.children.find(node => node.type === "heading" && node.depth === 1);
  const paragraph = opening(tree);
  const plain = (node: Parameters<typeof toString>[0]): string => {
    if (node && typeof node === "object" && "type" in node && node.type === "break") return " ";
    return node && typeof node === "object" && "children" in node && Array.isArray(node.children)
      ? node.children.map(plain).join("") : toString(node);
  };
  const defaults = {
    title: heading ? toString(heading) : "",
    description: paragraph ? plain(paragraph).replace(/\s+/g, " ").trim() : "",
  };
  return { ...defaults, ...metadataFields(tree) };
}

/** Retain authored overrides separately so exports do not duplicate inferred copy. */
export function metadataFields(tree: Root): Partial<PageMetadata> {
  const frontmatter = tree.children.find(node => node.type === "yaml");
  if (!frontmatter) return {};
  const document = parseDocument(frontmatter.value, { uniqueKeys: true });
  if (document.errors.length) throw new Error(`invalid page metadata: ${document.errors[0].message}`);
  const value: unknown = document.toJS({ maxAliasCount: 0 });
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("page metadata must be a mapping");
  const fields: Record<string, string> = {};
  for (const [key, text] of Object.entries(value)) {
    if (!["title", "description", "image"].includes(key)) throw new Error(`unknown page metadata: ${key}`);
    if (typeof text !== "string" || !text.trim()) throw new Error(`page metadata ${key} must be nonempty text`);
    fields[key] = text.trim();
  }
  if (fields.image) imageUrl(fields.image);
  return fields;
}

/** Images may be public web URLs or paths, never executable or embedded data. */
export function imageUrl(value: string): string {
  if (/^[a-z][a-z+.-]*:/i.test(value) && !/^https?:\/\//i.test(value) || value.startsWith("//") || /[\\\x00-\x1f]/.test(value)) {
    throw new Error(`unsafe page image: ${value}`);
  }
  return value;
}

const element = (tagName: string, properties: Element["properties"], children: Element["children"] = []): Element => ({ type: "element", tagName, properties, children });
const text = (value: string): Element["children"] => [{ type: "text", value }];

/** Render a tree, resolving card metadata through the caller's content source. */
export function format(tree: Root, page?: (href: string) => PageReference | undefined): Hast {
  tree = structuredClone(tree);
  const visit = (parent: { children: Root["children"] }) => {
    for (const node of parent.children) {
      if (node.type === "containerDirective" && node.name === "cards") {
        if (Object.keys(node.attributes ?? {}).length || node.children.length !== 1 || node.children[0].type !== "list" || node.children[0].ordered) {
          throw new Error(":::cards must contain one unordered list of page links");
        }
        const entries = node.children[0].children.map(item => {
          const paragraph = item.children.length === 1 ? item.children[0] : undefined;
          const link = paragraph?.type === "paragraph" && paragraph.children.length === 1 ? paragraph.children[0] : undefined;
          if (link?.type !== "link" || item.checked != null) throw new Error("each :::cards entry must be one page link");
          const found = page?.(link.url);
          if (!found) throw new Error(`:::cards links an unavailable page: ${link.url}`);
          const cover = element("img", { src: found.image ? imageUrl(found.image) : undefined, alt: "", dataPageCover: found.title });
          return element("li", {}, [element("a", { href: found.href, className: ["page-card"] }, [
            cover,
            element("span", { className: ["page-card-copy"] }, [
              element("span", { className: ["page-card-title"] }, text(found.title)),
              element("span", { className: ["page-card-intro"] }, text(found.description)),
            ]),
          ])]);
        });
        if (!entries.length) throw new Error(":::cards needs at least one page");
        node.data = { hName: "ul", hProperties: { className: ["page-grid", "not-prose"] }, hChildren: entries };
      } else if ("children" in node) visit(node as { children: Root["children"] });
    }
  };
  visit(tree);
  // HTML and frontmatter are omitted by remark-rehype, not evaluated as code.
  return html.runSync(tree) as Hast;
}
