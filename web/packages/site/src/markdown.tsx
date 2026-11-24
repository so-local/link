// Lays out one card of a doc (a tree from src/docs.ts) as React: a ```sh
// block gives each command a copy button, a link titled "button" is the same
// pill alone or among a card's words, a list of links
// alone is a row of pills, a link to a page moves within the site. The
// words are the doc's.

import type { Root } from "hast";
import { toJsxRuntime, type Components } from "hast-util-to-jsx-runtime";
import Link from "next/link";
import { isValidElement, type ComponentProps, type ElementType, type ReactElement, type ReactNode } from "react";
import { Fragment, jsx, jsxs } from "react/jsx-runtime";
import { action } from "./card";
import { actionTitle } from "./action";
import Command from "./command";
import Cover from "./cover";

type Words = { copy: string; copied: string };

function text(node: ReactNode): string {
  if (typeof node === "string") return node;
  if (Array.isArray(node)) return node.map(text).join("");
  if (isValidElement(node)) return text((node as ReactElement<{ children?: ReactNode }>).props.children);
  return "";
}

type LinkProps = ComponentProps<"a"> & { "data-button"?: boolean; "data-short-action"?: string };

function ActionLabel({ children, short }: { children: ReactNode; short?: string }) {
  if (!short) return <>{children}</>;
  return <><span className="action-label-full">{children}</span><span className="action-label-short" aria-hidden>{short}</span></>;
}

export function ActionLink({ href = "", children, title, className, "data-button": button, "data-short-action": compact }: LinkProps) {
  const style = actionTitle(title);
  const short = compact ?? style?.short;
  const label = short ? text(children) : undefined;
  if (style?.kind === "placeholder") {
    return <button type="button" className={`${action} disabled:cursor-not-allowed disabled:opacity-60`} data-button data-away data-short-action={short} aria-label={label} disabled><ActionLabel short={short}>{children}</ActionLabel></button>;
  }
  const isButton = button || style?.kind === "button";
  const mail = href.startsWith("mailto:");
  const away = /^https?:/.test(href) || mail || undefined;
  const newTab = mail || (away && isButton);
  if (isButton) {
    const Tag = href.startsWith("/") ? Link : "a";
    return <Tag href={href} target={newTab ? "_blank" : undefined} rel={newTab ? "noopener noreferrer" : undefined} data-button data-away={away} data-short-action={short} aria-label={label} className={action}><ActionLabel short={short}>{children}</ActionLabel></Tag>;
  }
  return href.startsWith("/") ? <Link href={href} className={className}>{children}</Link> : <a href={href} className={className} data-away={away} target={newTab ? "_blank" : undefined} rel={newTab ? "noopener noreferrer" : undefined}>{children}</a>;
}

/** A list of links alone, out of any card: shared action pills. */
const row = (base: Components): Components => ({
  ...base,
  ul({ children }) {
    return <ul className="flex flex-wrap items-center justify-between gap-3">{children}</ul>;
  },
  li({ children }) {
    return <li className="last:ml-auto">{children}</li>;
  },
  a(props) {
    return jsx((base.a ?? ActionLink) as ElementType<LinkProps>, { ...props, title: props.title ?? "button" });
  },
});

function components(words: Words): Components {
  return {
    // A fenced block: each shell statement gets a copy button; other languages stay code.
    pre({ children }) {
      const code = isValidElement(children)
        ? (children as ReactElement<{ className?: string; children?: ReactNode }>)
        : undefined;
      const body = text(code?.props.children ?? children).replace(/\n$/, "");
      if (code?.props.className?.split(" ").includes("language-sh")) {
        return <Command text={body} copy={words.copy} copied={words.copied} />;
      }
      return (
        <pre className="not-prose my-4 overflow-x-auto rounded-lg bg-zinc-900/5 px-4 py-3 font-mono text-sm leading-6 text-zinc-900 ring-1 ring-zinc-900/10">
          <code>{body}</code>
        </pre>
      );
    },
    a: (props) => <ActionLink {...props} />,
    img(props) {
      const { "data-page-cover": title, src, ...rest } = props as ComponentProps<"img"> & { "data-page-cover"?: string };
      if (title) return <Cover title={title} src={typeof src === "string" ? src : undefined} />;
      // eslint-disable-next-line @next/next/no-img-element -- Markdown images may use any public content bucket
      return <img {...rest} src={src} alt={rest.alt ?? ""} />;
    },
    // Wide tables scroll inside their card.
    table({ children }) {
      return (
        <div className="overflow-x-auto">
          <table>{children}</table>
        </div>
      );
    },
  };
}

export default function Markdown({ tree, words, pills, components: custom = {} }: { tree: Root; words: Words; pills?: boolean; components?: Components }) {
  const combined = { ...components(words), ...custom };
  return toJsxRuntime(tree, { Fragment, jsx, jsxs, components: pills ? row(combined) : combined });
}
