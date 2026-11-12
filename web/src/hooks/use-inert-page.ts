import { useEffect } from "react"

let open = 0

/** While a modal overlay is mounted, the page behind it (`#root`; overlays are portalled outside it) is `inert`: nothing in
 * it can take the focus, whatever the engine's Tab order. Radix traps the focus, but Safari's Tab (which skips buttons by
 * default) went past its guards into the page behind; `inert` is the browser's own rule. Counted, so nested overlays work. */
export function useInertPage() {
  useEffect(() => {
    const root = document.getElementById("root")
    if (!root) return
    open += 1
    root.inert = true
    return () => {
      open -= 1
      if (open === 0) root.inert = false
    }
  }, [])
}

/** Renders nothing; mounted inside an overlay's content (which exists only while it is open), it makes the page inert. */
export function InertPage() {
  useInertPage()
  return null
}

