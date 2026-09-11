import { Monitor, Star } from "lucide-react"
import { Card } from "@/components/ui/card"
import { Button } from "@/components/ui/button"
import { cn } from "@/lib/utils"

export interface Host {
  id: string;
  name: string;
  ip: string;
  os?: string | null;
  online?: boolean | null;
  paired?: boolean | null;
}

export interface Props {
  hosts: Array<{
    id: string;
    name: string;
    ip: string;
    os?: string | null;
    online?: boolean | null;
    paired?: boolean | null;
  }>;
  favoriteIps: string[];
  busy?: boolean;
  onConnect: (host: { id: string; ip: string; name: string }) => void;
  onToggleFavorite: (ip: string) => void;
  statusMessage?: string | null;
}

export type HostGridProps = Props;

export function HostGrid({
  hosts,
  favoriteIps,
  busy = false,
  onConnect,
  onToggleFavorite,
  statusMessage,
}: Props) {
  const normalizedFavorites = new Set(favoriteIps.map((ip) => ip.trim().toLowerCase()));

  return (
    <section className="space-y-4" aria-labelledby="library-heading">
      <div className="space-y-1">
        <h2 id="library-heading" className="text-xl font-bold tracking-tight text-foreground">
          Listed computers <span id="library-count" className="text-base font-normal text-muted-foreground">{hosts.length}</span>
        </h2>
        <p className="text-sm text-muted-foreground">
          Availability is a discovery hint, not an ERD readiness check. Default testbeds may appear available without a probe.
        </p>
      </div>

      {statusMessage ? (
        <div id="library-status" role="status" className="py-6 text-sm text-muted-foreground">
          {statusMessage}
        </div>
      ) : (
        <div
          id="host-grid"
          role="list"
          aria-busy={busy}
          className="grid grid-cols-[repeat(auto-fill,minmax(18rem,1fr))] max-[1040px]:grid-cols-2 max-[720px]:grid-cols-1 gap-4"
        >
          {hosts.map((host) => {
            const isFavorite =
              favoriteIps.includes(host.ip) ||
              normalizedFavorites.has(host.ip.trim().toLowerCase());

            const osText = host.os || "Computer";
            const availabilityText = host.online === true ? "Available" : "Offline";
            const pairingText = host.paired ? "Saved pairing" : "PIN may be needed";
            const metaLine = `${osText} · ${availabilityText} · ${pairingText}`;

            return (
              <Card
                key={host.id}
                role="listitem"
                data-host-id={host.id}
                data-availability={host.online === true ? "available" : "offline"}
                className="flex flex-col justify-between gap-4 p-4 rounded-panel border-border bg-card shadow-sm"
              >
                <div className="flex items-start gap-3 min-w-0 max-[400px]:flex-wrap">
                  <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-lg bg-secondary text-muted-foreground">
                    <Monitor className="h-5 w-5" aria-hidden="true" />
                  </div>
                  <div className="min-w-0 flex-1 space-y-1 max-[400px]:basis-[calc(100%-48px)]">
                    <div
                      className="font-bold truncate text-foreground text-base leading-snug"
                      title={host.name}
                    >
                      {host.name}
                    </div>
                    <div className="text-sm text-muted-foreground font-mono truncate">
                      {host.ip}
                    </div>
                    <div className="text-xs text-muted-foreground">
                      {metaLine}
                    </div>
                  </div>
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon-sm"
                    data-action="favorite"
                    aria-label={`Favorite ${host.name}`}
                    aria-pressed={isFavorite}
                    title={`Favorite ${host.name}`}
                    onClick={() => onToggleFavorite(host.ip)}
                    className={cn(
                      "shrink-0 text-muted-foreground hover:text-foreground",
                      isFavorite && "text-warning hover:text-warning"
                    )}
                  >
                    <Star
                      className={cn("size-4", isFavorite && "fill-current")}
                      aria-hidden="true"
                    />
                  </Button>
                </div>

                <div className="mt-auto pt-1">
                  <Button
                    type="button"
                    data-action="connect"
                    className="w-full"
                    disabled={host.online !== true || busy}
                    onClick={() => onConnect({ id: host.id, ip: host.ip, name: host.name })}
                    title={
                      host.online !== true
                        ? "Offline discovery record. Use Direct connection to try an address explicitly."
                        : undefined
                    }
                  >
                    Connect
                  </Button>
                </div>
              </Card>
            );
          })}
        </div>
      )}
    </section>
  );
}

export default HostGrid;
