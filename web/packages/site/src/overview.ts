import type { Image, Paragraph, Root } from "mdast";

declare module "mdast" {
  interface ParagraphData {
    overviewIntroduction?: boolean;
  }
}

/**
 * Keep the README's brand from its centered header without enabling raw
 * HTML: its static image, its title and its tagline, the first paragraph
 * that is bold alone. The header's other lines (its links and license)
 * are for a reader on GitHub, and the website's tabs already carry them.
 */
export function overview(tree: Root): Root {
  let centered = false;
  let tagline = false;
  let introduction = false;
  tree.children = tree.children.flatMap((node): Root["children"] => {
    if (node.type === "html") {
      const value = node.value.trim();
      if (/^<div\s+align=["']center["']\s*>$/i.test(value)) {
        centered = true;
        return [];
      }
      if (centered && /^<\/div>$/i.test(value)) {
        centered = false;
        introduction = true;
        return [{ type: "thematicBreak", data: { hProperties: { dataOverviewHeader: true } } }];
      }
      if (/^<img\s[^>]*\/?\s*>$/i.test(value)) {
        const attributes = new Map([...value.matchAll(/\b(src|alt|width|height)\s*=\s*(?:"([^"]*)"|'([^']*)')/gi)].map(match => [match[1].toLowerCase(), match[2] ?? match[3]]));
        const url = attributes.get("src");
        if (!url) return [];
        const properties: Record<string, number> = {};
        for (const key of ["width", "height"]) {
          const number = Number(attributes.get(key));
          if (Number.isSafeInteger(number) && number > 0) properties[key] = number;
        }
        const image: Image = { type: "image", url, alt: attributes.get("alt") ?? "", data: { hProperties: properties } };
        return [{ type: "paragraph", children: [image] }];
      }
    }
    if (centered && node.type === "paragraph") {
      const bold = node.children.length === 1 && node.children[0].type === "strong";
      if (!bold || tagline) return [];
      tagline = true;
    }
    if (introduction && node.type === "paragraph") {
      node.data = { ...node.data, overviewIntroduction: true };
      introduction = false;
    }
    return [node];
  });
  return tree;
}

/** The narrative introduction follows the branding, rather than using its tagline. */
export function opening(tree: Root): Paragraph | undefined {
  const marked = tree.children.find((node): node is Paragraph => node.type === "paragraph" && node.data?.overviewIntroduction === true);
  if (marked) return marked;
  const at = tree.children.findIndex(node => node.type === "heading" && node.depth === 1);
  return tree.children.slice(at + 1).find((node): node is Paragraph => node.type === "paragraph" && node.children.some(child => child.type !== "image"));
}
