import { Monitor, Star } from "lucide-react"
import { Button } from "@/components/ui/button"
import { cn } from "@/lib/utils"

export interface Props {
  active: "computers" | "favorites"
  onSelect: (view: "computers" | "favorites") => void
}

export type SidebarProps = Props

export function Sidebar({ active, onSelect }: Props) {
  return (
    <aside className="flex h-full w-64 max-[1040px]:w-[72px] shrink-0 flex-col border-r border-border bg-sidebar">
      <header className="flex h-16 items-center gap-3 p-4 max-[1040px]:justify-center max-[1040px]:p-0">
        <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-primary font-bold text-primary-foreground">
          E
        </div>
        <span className="font-bold text-foreground max-[1040px]:hidden">EclipticRD</span>
      </header>

      <nav aria-label="Primary" className="flex flex-1 flex-col gap-1 px-3 py-2 max-[1040px]:px-2">
        <Button
          id="nav-computers"
          type="button"
          variant="ghost"
          aria-label="Computers"
          title="Computers"
          className={cn(
            "w-full justify-start gap-2 max-[1040px]:justify-center max-[1040px]:px-0",
            active === "computers" && "bg-accent text-accent-foreground"
          )}
          aria-current={active === "computers" ? "page" : "false"}
          onClick={() => onSelect("computers")}
        >
          <Monitor className="size-4 shrink-0" />
          <span className="max-[1040px]:sr-only">Computers</span>
        </Button>
        <Button
          id="nav-favorites"
          type="button"
          variant="ghost"
          aria-label="Favorites"
          title="Favorites"
          className={cn(
            "w-full justify-start gap-2 max-[1040px]:justify-center max-[1040px]:px-0",
            active === "favorites" && "bg-accent text-accent-foreground"
          )}
          aria-current={active === "favorites" ? "page" : "false"}
          onClick={() => onSelect("favorites")}
        >
          <Star className="size-4 shrink-0" />
          <span className="max-[1040px]:sr-only">Favorites</span>
        </Button>
      </nav>

      <footer className="mt-auto p-4 text-xs text-muted-foreground max-[1040px]:hidden">
        EclipticRD
      </footer>
    </aside>
  )
}

export default Sidebar
