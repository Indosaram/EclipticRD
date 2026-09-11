import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Card } from "@/components/ui/card"

export interface SavedCredential {
  id: string;
  name: string;
  endpoint?: string | null;
}

export interface Props {
  pairings: Array<{ id: string; name: string; endpoint?: string | null }>;
  busy?: boolean;
  onConnect: (id: string) => void;
  onForget: (id: string) => void;
  onRefresh: () => void;
}

export type SavedCredentialsProps = Props;

export function SavedCredentials({
  pairings,
  busy = false,
  onConnect,
  onForget,
  onRefresh,
}: Props) {
  return (
    <section
      id="saved-pairings-section"
      data-testid="saved-credentials"
      aria-labelledby="saved-pairings-heading"
      className="space-y-3"
    >
      <div className="flex items-center justify-between gap-4">
        <h2
          id="saved-pairings-heading"
          className="text-lg font-semibold text-foreground"
        >
          Saved credentials{" "}
          <span
            id="saved-pairings-count"
            className="text-muted-foreground font-normal tabular-nums"
          >
            {pairings.length}
          </span>
        </h2>
        <Button
          id="btn-refresh-pairings"
          variant="outline"
          size="sm"
          onClick={onRefresh}
          disabled={busy}
          title="Refresh saved credentials"
        >
          Refresh credentials
        </Button>
      </div>

      <p className="text-xs text-muted-foreground leading-relaxed">
        Saved credentials connect directly without a PIN. Same-name credentials are distinguished by ID and endpoint metadata.
      </p>

      {pairings.length === 0 ? (
        <div
          id="saved-pairings-empty"
          className="rounded-panel border border-dashed border-border p-6 text-center text-sm text-muted-foreground bg-card"
        >
          No saved credentials yet.
        </div>
      ) : (
        <div
          id="saved-pairings-list"
          role="list"
          className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-3"
        >
          {pairings.map((pairing) => (
            <Card
              key={pairing.id}
              data-pairing-id={pairing.id}
              role="listitem"
              className="flex flex-col justify-between gap-3 p-4 rounded-panel border-border bg-card"
            >
              <div className="space-y-1.5 min-w-0">
                <div className="flex items-center justify-between gap-2">
                  <h3
                    className="font-bold text-base text-foreground truncate"
                    title={pairing.name}
                  >
                    {pairing.name}
                  </h3>
                  <Badge variant="secondary" className="text-xs font-semibold">
                    SAVED
                  </Badge>
                </div>
                <p className="font-mono text-xs text-muted-foreground break-all">
                  ID: {pairing.id}
                </p>
                <p className="font-mono text-xs text-foreground/90 break-all">
                  {pairing.endpoint || "Endpoint not yet recorded"}
                </p>
              </div>

              <div className="flex items-center gap-2 pt-2 mt-auto">
                <Button
                  variant="default"
                  size="sm"
                  className="flex-1"
                  disabled={busy}
                  onClick={() => onConnect(pairing.id)}
                  data-action="connect-saved"
                >
                  Connect
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  disabled={busy}
                  onClick={() => onForget(pairing.id)}
                  data-action="forget-saved"
                >
                  Forget
                </Button>
              </div>
            </Card>
          ))}
        </div>
      )}
    </section>
  )
}
