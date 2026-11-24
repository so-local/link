import type { ReactNode } from "react";
import FragmentRedirect from "../fragment-redirect";

export default function Template({ children }: { children: ReactNode }) {
  return <><FragmentRedirect />{children}</>;
}
