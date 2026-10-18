import { ChevronLeft, ChevronRight, Search } from "lucide-react";
import { type ReactNode, useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { useDebouncedValue } from "@/hooks/use-debounced-value";
import { pageCount } from "@/lib/admin";
import { cn } from "@/lib/utils";

/** A search field that reports its text once typing pauses. */
export function SearchBox({
  value,
  onSearch,
  placeholder,
}: {
  value: string;
  onSearch: (value: string) => void;
  placeholder: string;
}) {
  const [text, setText] = useState(value);
  const debounced = useDebouncedValue(text);
  useEffect(() => {
    if (debounced !== value) onSearch(debounced);
  }, [debounced, value, onSearch]);

  return (
    <div className="relative w-full sm:max-w-xs">
      <Search className="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground" />
      <Input
        value={text}
        onChange={(event) => setText(event.target.value)}
        placeholder={placeholder}
        aria-label={placeholder}
        className="pl-8"
      />
    </div>
  );
}

export function Pager({
  page,
  total,
  perPage,
  onPage,
}: {
  page: number;
  total: number;
  perPage: number;
  onPage: (page: number) => void;
}) {
  const pages = pageCount(total, perPage);
  if (pages <= 1) return null;
  return (
    <div className="flex items-center justify-end gap-2 text-sm text-muted-foreground">
      <span className="font-mono tabular">
        {page} / {pages}
      </span>
      <Button
        size="icon-sm"
        variant="outline"
        aria-label="Previous page"
        disabled={page <= 1}
        onClick={() => onPage(page - 1)}
      >
        <ChevronLeft />
      </Button>
      <Button
        size="icon-sm"
        variant="outline"
        aria-label="Next page"
        disabled={page >= pages}
        onClick={() => onPage(page + 1)}
      >
        <ChevronRight />
      </Button>
    </div>
  );
}

/** A number with a label, for the overview and detail pages. */
export function Stat({
  label,
  value,
  hint,
  className,
}: {
  label: string;
  value: ReactNode;
  hint?: ReactNode;
  className?: string;
}) {
  return (
    <div className={cn("flex min-w-0 flex-col gap-1 rounded-2xl border bg-card p-4", className)}>
      <span className="truncate text-xs tracking-wider text-muted-foreground uppercase">
        {label}
      </span>
      <span className="font-mono text-2xl leading-none font-semibold tabular">{value}</span>
      {hint && <span className="text-xs text-muted-foreground">{hint}</span>}
    </div>
  );
}
