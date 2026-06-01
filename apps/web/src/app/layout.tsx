import type { Metadata } from "next";
import { Geist, Geist_Mono } from "next/font/google";
import { Toaster } from "sonner";
import { Shell } from "@/components/shell";
import "./globals.css";

const geistSans = Geist({ variable: "--font-geist-sans", subsets: ["latin"] });
const geistMono = Geist_Mono({ variable: "--font-geist-mono", subsets: ["latin"] });

export const metadata: Metadata = {
  title: "pdfkit — PDF toolkit",
  description: "Read-first PDF extraction, chunking, rendering, and editing.",
};

// Runs before paint to set the theme class, avoiding a flash of the wrong theme.
const themeInit = `(function(){try{var t=localStorage.getItem('theme');if(t==='dark'||(!t&&window.matchMedia('(prefers-color-scheme: dark)').matches)){document.documentElement.classList.add('dark');}}catch(e){}})();`;

export default function RootLayout({ children }: Readonly<{ children: React.ReactNode }>) {
  return (
    <html
      lang="en"
      suppressHydrationWarning
      className={`${geistSans.variable} ${geistMono.variable} antialiased`}
    >
      <body>
        <script dangerouslySetInnerHTML={{ __html: themeInit }} />
        <Shell>{children}</Shell>
        <Toaster
          position="top-right"
          toastOptions={{
            classNames: {
              toast:
                "!rounded-lg !border !border-border !bg-surface-elevated !text-foreground !shadow-lg !text-[13px]",
              description: "!text-muted-foreground",
            },
          }}
        />
      </body>
    </html>
  );
}
