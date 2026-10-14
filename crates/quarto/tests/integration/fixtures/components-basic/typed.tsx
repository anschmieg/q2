// A TypeScript component with JSX and a typed helper in another file:
// transpiled at render time, no build step.
import { useState } from "react";
import type { QuartoComponentProps } from "@quarto/components";
import { pad } from "./lib/pad.ts";

export default function Typed({ children }: QuartoComponentProps) {
  const [n, setN] = useState<number>(0);
  return (
    <div className="typed" data-count={pad(n)}>
      <button type="button" onClick={() => setN((v) => v + 1)}>
        typed {pad(n)}
      </button>
      {children}
    </div>
  );
}
