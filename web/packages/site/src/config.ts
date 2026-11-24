import { existsSync, readFileSync, readdirSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join, relative } from "node:path";
import type { NextConfig } from "next";
import { translationLanguage } from "./languages.mjs";

// Each website's Next.js settings, the same for both: every page is a
// doc in a folder's docs/, or its translation beside it, served at
// app/[lang]/[[...path]] (docs.ts). English, the default, has no
// prefix: /agents is served from /en/agents, and /en/agents redirects to
// /agents, so each page has one address. Other languages keep theirs.

// This package, wherever the app's workspace found it, and the folder
// holding it and the repository whose docs the app reads (docs.ts).
const app = createRequire(join(process.cwd(), "package.json"));
const site = dirname(dirname(app.resolve("@link-web/site/config")));
let root = join(process.cwd(), "../../..");
while (relative(root, site).startsWith("..")) root = dirname(root);

// A workspace that lists this package from outside its own folder (a
// private app over the public package) installs the package's
// dependencies beside the app, not beside the package, so the bundler is
// told where each is.
const alias: Record<string, string> = {};
const there = (path: string) => `./${relative(process.cwd(), path)}`;
if (!site.startsWith(join(process.cwd(), "../.."))) {
  const { dependencies } = JSON.parse(readFileSync(join(site, "package.json"), "utf8")) as { dependencies: Record<string, string> };
  const names = Object.keys(dependencies);
  for (let dir = process.cwd(); dir !== dirname(dir); dir = dirname(dir)) {
    for (const name of names) {
      const at = join(dir, "node_modules", name);
      if (!(name in alias) && existsSync(join(at, "package.json"))) {
        alias[name] = there(at);
        alias[`${name}/*`] = `${there(at)}/*`;
      }
    }
  }
  // These font exports live under dist/, not their public subpath. A
  // directory wildcard bypasses package exports, so resolve each entry.
  for (const name of ["geist/font/sans", "geist/font/mono"]) {
    alias[name] = there(app.resolve(name));
  }
}

// Keep the concrete settings type when another app extends this config.
const config = {
  // This package's TypeScript, built by the app.
  transpilePackages: ["@link-web/site", "@link-flowers/browser", "@link-flowers/shared"],
  turbopack: { root, resolveAlias: alias },
  env: { LINK_SITE_LOCALES: join(site, "locales") },
  images: {
    // AVIF first: about half the size of WebP for the background photo.
    formats: ["image/avif", "image/webp"],
  },
  // Every page: no framing by another site, no sniffing a type, the
  // address alone sent on, HTTPS remembered, no device asked for.
  async headers() {
    return [
      {
        source: "/:path*",
        headers: [
          { key: "Content-Security-Policy", value: "frame-ancestors 'none'" },
          { key: "X-Frame-Options", value: "DENY" },
          { key: "X-Content-Type-Options", value: "nosniff" },
          { key: "Referrer-Policy", value: "strict-origin-when-cross-origin" },
          { key: "Strict-Transport-Security", value: "max-age=63072000; includeSubDomains" },
          { key: "Permissions-Policy", value: "camera=(), microphone=(), geolocation=()" },
        ],
      },
    ];
  },
  async redirects() {
    return [
      { source: "/en", destination: "/", permanent: true },
      // The share image keeps its /en address, which metadata names.
      { source: "/en/:path((?!opengraph-image/).*)", destination: "/:path", permanent: true },
    ];
  },
  async rewrites() {
    const docs = process.env.LINK_SITE_CONTENT_ROOT ?? join(process.cwd(), "docs");
    const otherLanguages = readdirSync(docs).flatMap(name => name.startsWith("navigation.") ? [translationLanguage(name)].filter((lang) => lang !== undefined) : []);
    const skip = otherLanguages.map((lang) => `${lang}(?:/|$)|`).join("");
    return {
      beforeFiles: [],
      // After files (the icons, _next), before the dynamic routes.
      afterFiles: [
        { source: "/", destination: "/en" },
        { source: `/:path((?!${skip}en(?:/|$)).+)`, destination: "/en/:path" },
      ],
      fallback: [],
    };
  },
} satisfies NextConfig;

export default config;
