"use client";

import { useEffect } from "react";
import { useParams, usePathname } from "next/navigation";
import { relocatedFragment } from "../../relocations";

export default function FragmentRedirect() {
  const pathname = usePathname();
  const { lang } = useParams<{ lang: string }>();
  useEffect(() => {
    const follow = () => {
      const target = relocatedFragment(pathname, window.location.hash, window.location.search, lang);
      if (target) window.location.replace(target);
    };
    follow();
    window.addEventListener("hashchange", follow);
    return () => window.removeEventListener("hashchange", follow);
  }, [pathname, lang]);
  return null;
}
