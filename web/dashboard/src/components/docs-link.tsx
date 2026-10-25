import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

/** The NOALBS Cloud docs (github.com/NOALBS/noalbs-cloud-docs). */
export const DOCS_URL = "https://docs.noalbs.com";

/** A link to a docs page, e.g. `page="guides/troubleshooting" anchor="it-doesnt-switch"`. */
export function DocsLink({
  page,
  anchor,
  className,
  children,
}: {
  page: string;
  anchor?: string;
  className?: string;
  children: ReactNode;
}) {
  return (
    <a
      href={`${DOCS_URL}/${page}/${anchor ? `#${anchor}` : ""}`}
      target="_blank"
      rel="noreferrer"
      className={cn("font-medium text-primary hover:underline", className)}
    >
      {children}
    </a>
  );
}
