import { Monitor } from "lucide-react"
import { Card } from "@/components/ui/card"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { cn } from "@/lib/utils"

export interface ThisComputerCardProps {
  ip: string | null;
  pin: string | null;
  running: boolean;
  busy?: boolean;
  onToggleSharing: () => void;
  onCopyInfo: () => void;
}

export type Props = ThisComputerCardProps;

export function ThisComputerCard({
  ip,
  pin,
  running,
  busy = false,
  onToggleSharing,
  onCopyInfo,
}: ThisComputerCardProps) {
  const displayIp = ip || "Unavailable"
  const displayPin = pin || "--------"

  return (
    <Card
      id="this-computer-card"
      className="flex flex-row items-center justify-between gap-4 p-5 max-[720px]:flex-col max-[720px]:items-start max-[720px]:gap-3 max-[720px]:p-4 rounded-panel border-border bg-card"
    >
      <div className="flex items-center gap-4 min-w-0 max-[720px]:w-full">
        <div className="flex h-11 w-11 shrink-0 items-center justify-center rounded-lg bg-secondary text-foreground">
          <Monitor className="h-6 w-6" aria-hidden="true" />
        </div>
        <div className="min-w-0 space-y-1">
          <div className="flex items-center gap-2 flex-wrap">
            <h2 id="this-computer-title" className="font-bold text-base text-foreground leading-tight">
              This Computer (Host)
            </h2>
            <Badge
              id="this-computer-badge"
              variant="outline"
              className={cn(
                running
                  ? "text-success border-success/30"
                  : "text-warning border-warning/30"
              )}
            >
              {running ? "READY" : "UNAVAILABLE"}
            </Badge>
          </div>
          <p className="text-sm text-muted-foreground flex items-center gap-2 flex-wrap">
            <span id="this-computer-ip">
              IP: <span className="font-mono text-foreground">{displayIp}</span>
            </span>
            <span className="text-muted-foreground" aria-hidden="true">
              ·
            </span>
            <span>
              PIN:{" "}
              <strong
                id="this-computer-pin"
                className="font-mono font-semibold text-foreground"
              >
                {displayPin}
              </strong>
            </span>
          </p>
        </div>
      </div>

      <div className="flex items-center gap-2 shrink-0 ml-auto max-[720px]:w-full max-[720px]:ml-0">
        <Button
          id="btn-host-toggle"
          variant="outline"
          className="max-[720px]:flex-1"
          onClick={onToggleSharing}
          disabled={busy}
        >
          {running ? "Pause Sharing" : "Resume Sharing"}
        </Button>
        <Button
          id="btn-host-copy"
          variant="outline"
          className="max-[720px]:flex-1"
          onClick={onCopyInfo}
          disabled={busy}
        >
          Copy Info
        </Button>
      </div>
    </Card>
  )
}
