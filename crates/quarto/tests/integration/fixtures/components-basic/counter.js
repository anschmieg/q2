// A bound component with no build step: plain ES module, no JSX.
// Wraps its no-script children and counts clicks on a button.
import { useState, createElement as h } from "react";

export default function Counter({ attrs, children }) {
  const [n, setN] = useState(0);
  return h(
    "div",
    { className: "counter", "data-count": String(n) },
    h("button", { type: "button", onClick: () => setN(n + 1) }, `clicked ${n} times`),
    h("p", { className: "counter-attrs" }, `id=${attrs.id} classes=${attrs.classes.join(",")}`),
    children,
  );
}
