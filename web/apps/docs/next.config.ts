import type { NextConfig } from "next";
import shared from "@link-web/site/config";
import relocations from "./docs/relocations.json";

const config: NextConfig = {
  ...shared,
  async redirects() {
    return [
      ...Object.entries(relocations.pages).flatMap(([path, destination]) => [
        { source: `/en${path}`, destination, permanent: true },
        { source: path, destination, permanent: true },
        { source: `/:lang${path}`, destination: `/:lang${destination}`, permanent: true },
      ]),
      // Match English first so an old /en address reaches its canonical
      // destination directly rather than passing through another redirect.
      { source: "/en/agents/harnesses/link/:path*", destination: "/agents/link/:path*", permanent: true },
      { source: "/agents/harnesses/link/:path*", destination: "/agents/link/:path*", permanent: true },
      { source: "/:lang/agents/harnesses/link/:path*", destination: "/:lang/agents/link/:path*", permanent: true },
      { source: "/en/tooling/sandbox/:path*", destination: "/security/sandbox/:path*", permanent: true },
      { source: "/tooling/sandbox/:path*", destination: "/security/sandbox/:path*", permanent: true },
      { source: "/:lang/tooling/sandbox/:path*", destination: "/:lang/security/sandbox/:path*", permanent: true },
      ...(await shared.redirects?.() ?? []),
    ];
  },
};

export default config;
