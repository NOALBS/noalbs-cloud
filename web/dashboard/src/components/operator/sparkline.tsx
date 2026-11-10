import { t } from "@lingui/core/macro";
import { type BitratePoint, HISTORY_MS } from "./history";

export function Sparkline({
  points = [],
  now,
  className,
}: {
  points?: BitratePoint[];
  now: number;
  className?: string;
}) {
  const visible = points.filter(
    (point) =>
      point.at >= now - HISTORY_MS &&
      point.at <= now &&
      Number.isFinite(point.bitrate) &&
      point.bitrate >= 0,
  );
  const max = Math.max(1, ...visible.map((point) => point.bitrate));
  const line = visible
    .map(
      (point) =>
        `${Math.max(0, (point.at - now + HISTORY_MS) / HISTORY_MS) * 200},${36 - (point.bitrate / max) * 30}`,
    )
    .join(" ");
  return (
    <svg
      viewBox="0 0 200 40"
      preserveAspectRatio="none"
      role="img"
      aria-label={t`Bitrate over the last two minutes`}
      className={className}
    >
      <path
        d="M0 36 H200"
        stroke="var(--divider)"
        strokeWidth="1"
        vectorEffect="non-scaling-stroke"
      />
      {visible.length > 1 && (
        <polyline
          points={line}
          fill="none"
          stroke="currentColor"
          strokeWidth="1.5"
          vectorEffect="non-scaling-stroke"
        />
      )}
    </svg>
  );
}
