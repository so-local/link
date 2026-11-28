// A site's visit counts, in production alone (`analytics-script.ts`),
// passed to the shared layout's `analytics` slot.

import { countingScript, type Counting } from "./analytics-script";

export { plausibleStart, type Counting } from "./analytics-script";

/** The inline script that counts this site's visits on its production hostnames. */
export function Analytics({ counting }: { counting: Counting }) {
  return <script dangerouslySetInnerHTML={{ __html: countingScript(counting) }} />;
}
