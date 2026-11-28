// Visit counts by Plausible Analytics, which keeps no cookies and no
// personal data, counted in production alone. Each website has a
// Plausible script of its own and the hostnames it's served at in
// production, named in that site's constants. The page loads the script
// only when the browser is on one of those hostnames, so scratch, a
// preview and a machine running the site count nothing.

/** A site's Plausible script and the production hostnames it counts visits on. */
export type Counting = { script: string; hosts: readonly string[] };

/** Plausible's queue, so a call made before its script loads waits for it, then its start. */
export const plausibleStart =
  "window.plausible=window.plausible||function(){(plausible.q=plausible.q||[]).push(arguments)},plausible.init=plausible.init||function(i){plausible.o=i||{}};plausible.init()";

/** The inline script that loads `script`, without blocking the page, and starts it, on one of `hosts` alone. */
export function countingScript({ script, hosts }: Counting): string {
  return (
    `if(${JSON.stringify(hosts)}.indexOf(location.hostname)>=0){` +
    `var s=document.createElement("script");s.async=true;s.src=${JSON.stringify(script)};document.head.appendChild(s);` +
    `${plausibleStart}}`
  );
}
