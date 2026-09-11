import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { cn } from "@/lib/utils"

export type FilterOption = "all" | "available" | "favorites";

export interface Props {
  query: string;
  onQueryChange: (q: string) => void;
  onClearSearch: () => void;
  filter: "all" | "available" | "favorites";
  onFilterChange: (f: "all" | "available" | "favorites") => void;
  className?: string;
}

export type LibraryToolbarProps = Props;

export function LibraryToolbar({
  query,
  onQueryChange,
  onClearSearch,
  filter,
  onFilterChange,
  className,
}: Props) {
  return (
    <div className={cn("library-toolbar flex flex-wrap items-end gap-2 mb-6", className)}>
      <Label htmlFor="host-search" className="basis-full text-sm font-medium text-foreground">
        Search computers
      </Label>
      <Input
        id="host-search"
        type="search"
        placeholder="Name, address or OS"
        spellCheck={false}
        value={query}
        onChange={(e) => onQueryChange(e.target.value)}
        className="flex-1 min-w-[240px] bg-background"
      />
      <Button
        id="btn-clear-search"
        type="button"
        variant="outline"
        onClick={onClearSearch}
      >
        Clear search
      </Button>
      <div
        className="filter-group flex flex-wrap items-center gap-2"
        role="group"
        aria-label="Filter computers"
      >
        <Button
          id="filter-all"
          type="button"
          variant="outline"
          aria-pressed={filter === "all" ? "true" : "false"}
          className={cn(filter === "all" && "text-primary border-primary")}
          onClick={() => onFilterChange("all")}
        >
          All
        </Button>
        <Button
          id="filter-available"
          type="button"
          variant="outline"
          aria-pressed={filter === "available" ? "true" : "false"}
          className={cn(filter === "available" && "text-primary border-primary")}
          onClick={() => onFilterChange("available")}
        >
          Available
        </Button>
        <Button
          id="filter-favorites"
          type="button"
          variant="outline"
          aria-pressed={filter === "favorites" ? "true" : "false"}
          className={cn(filter === "favorites" && "text-primary border-primary")}
          onClick={() => onFilterChange("favorites")}
        >
          Favorites
        </Button>
      </div>
    </div>
  )
}
