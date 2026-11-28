// The layout every website shares (@link-web/site), under the wordmark
// that leads to the home page.
import "../globals.css";
import SiteLayout from "@link-web/site/layout";
import Brand from "@link-web/site/brand";
import { Analytics } from "@link-web/site/analytics";
import { COUNTING } from "../../constants";
export { generateMetadata, generateStaticParams } from "@link-web/site/layout";
export const dynamicParams = false;

export default function Layout(props: LayoutProps<"/[lang]">) {
  return <SiteLayout {...props} header={<Brand />} analytics={<Analytics counting={COUNTING} />} />;
}
