import { Shell } from "@/components/shell";

// The product surface (extract / chunks / render / edit / docs) shares the
// sidebar Shell. The marketing landing page at `/` lives outside this group and
// renders full-bleed with its own nav + footer.
export default function AppLayout({ children }: { children: React.ReactNode }) {
  return <Shell>{children}</Shell>;
}
