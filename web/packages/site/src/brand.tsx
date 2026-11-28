// The wordmark above every page: the sunflower and the site's name,
// linking to the home page, www.local.link, from every website. Its look is globals.css's `.site-brand`, and its name
// look.ts's, which an app that can't read CSS draws from too.

import Link from "next/link";
import { FlowerSvg } from "@link-flowers/browser";
import { brand } from "./look";

export default function Brand({ href = brand.home }: { href?: string }) {
  return (
    <header className="site-brand">
      <Link href={href}>
        <span className="site-brand-flower" aria-hidden>
          <FlowerSvg framing="head" paused interactive={false} />
        </span>
        <span>{brand.name}</span>
      </Link>
    </header>
  );
}
