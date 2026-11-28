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
      { source: "/en/agents/link/:path+", destination: "/agents/harnesses/link/:path+", permanent: true },
      { source: "/agents/link/:path+", destination: "/agents/harnesses/link/:path+", permanent: true },
      { source: "/:lang/agents/link/:path+", destination: "/:lang/agents/harnesses/link/:path+", permanent: true },
      { source: "/en/security/sandbox/:path*", destination: "/tooling/sandbox/:path*", permanent: true },
      { source: "/security/sandbox/:path*", destination: "/tooling/sandbox/:path*", permanent: true },
      { source: "/:lang/security/sandbox/:path*", destination: "/:lang/tooling/sandbox/:path*", permanent: true },
      ...(await shared.redirects?.() ?? []),
    ];
  },
};

export default config;
