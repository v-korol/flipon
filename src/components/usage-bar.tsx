import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { faArrowTrendUp, faEquals } from "@fortawesome/free-solid-svg-icons";
import type { UsageWindow } from "@/lib/api";
import { trendTitle, type Tone } from "@/lib/format";
import { cn } from "@/lib/utils";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";

const FILL: Record<Tone, string> = {
  ok: "bg-primary",
  warn: "bg-warning",
  critical: "bg-destructive",
};

export function UsageBar({
  pct,
  tone,
  large,
  elapsed,
}: {
  pct: number;
  tone: Tone;
  large?: boolean;
  /** Percent of the window's time already elapsed — drawn as a thin marker. */
  elapsed?: number | null;
}) {
  return (
    <div className={cn("relative w-full overflow-hidden rounded-full bg-track", large ? "h-2" : "h-1.5")}>
      <div
        className={cn("absolute inset-y-0 left-0 rounded-full", FILL[tone])}
        style={{ width: `${Math.min(100, Math.max(0, pct))}%` }}
      />
      {elapsed != null && (
        <div className="absolute inset-y-0 w-0.5 bg-foreground/40" style={{ left: `${elapsed}%` }} />
      )}
    </div>
  );
}

export function TrendIcon({ trend }: { trend: UsageWindow["trend"] }) {
  if (!trend) return null;
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span className="ml-[5px] align-[1px] text-[11px] text-muted-foreground">
          <FontAwesomeIcon icon={trend.dir === "up" ? faArrowTrendUp : faEquals} />
        </span>
      </TooltipTrigger>
      <TooltipContent>{trendTitle(trend)}</TooltipContent>
    </Tooltip>
  );
}
