import { t } from "@lingui/core/macro";
import { type CSSProperties, createContext, type ReactNode, useContext, useState } from "react";
import { createPortal } from "react-dom";
import { cn } from "@/lib/utils";

const HeaderSlot = createContext<HTMLDivElement | null>(null);
export const TileEditing = createContext(false);
export function TileControls({ children }: { children: ReactNode }) {
  const slot = useContext(HeaderSlot);
  return slot ? createPortal(children, slot) : null;
}

export function LiveTile({
  title,
  right,
  needsObs = false,
  children,
  style,
  className,
}: {
  title: string;
  right?: ReactNode;
  needsObs?: boolean;
  children: ReactNode;
  style?: CSSProperties;
  className?: string;
}) {
  const [slot, setSlot] = useState<HTMLDivElement | null>(null);
  const editing = useContext(TileEditing);
  return (
    <HeaderSlot.Provider value={slot}>
      <section
        aria-label={title}
        style={style}
        className={cn(
          "@container min-w-0 rounded-2xl border bg-card shadow-tile",
          needsObs && "opacity-50",
          className,
        )}
      >
        {!editing && (
          <header className="flex items-center justify-between gap-3 px-4 pt-4 pb-3">
            <h2 className="text-sm font-semibold">{title}</h2>
            <div className="flex items-center gap-2 text-xs text-muted-foreground">
              {needsObs && <span>{t`Needs OBS`}</span>}
              {right}
              <div ref={setSlot} className="flex flex-wrap items-center justify-end gap-2" />
            </div>
          </header>
        )}
        <div className={cn("px-4 pb-4", editing && "pt-4")}>{children}</div>
      </section>
    </HeaderSlot.Provider>
  );
}
