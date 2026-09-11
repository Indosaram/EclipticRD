import { useState } from "react"
import { Sidebar } from "@/features/shell/Sidebar"
import { ComputersPage } from "@/features/library/ComputersPage"

export default function App() {
  const [activeView, setActiveView] = useState<"computers" | "favorites">("computers")

  return (
    <div
      className="flex h-screen w-screen overflow-hidden bg-background text-foreground"
      data-app-shell
    >
      <Sidebar active={activeView} onSelect={setActiveView} />
      <main
        className="flex-1 min-w-0 h-full overflow-y-auto p-8 max-[1040px]:p-6 max-[720px]:p-4"
        id="main-view"
        data-ui-scope
      >
        <ComputersPage view={activeView} />
      </main>
    </div>
  )
}
