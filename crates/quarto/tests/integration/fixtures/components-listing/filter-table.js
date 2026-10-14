// A filterable table over listing items passed as props (no build step).
import { useState, createElement as h } from "react";

export default function FilterTable({ props, children }) {
  const [q, setQ] = useState("");
  const rows = (props.rows || []).filter((r) =>
    r.title.toLowerCase().includes(q.toLowerCase()),
  );
  return h(
    "div",
    { className: "filter-table" },
    h("input", {
      type: "search",
      placeholder: "Filter posts",
      value: q,
      onChange: (e) => setQ(e.target.value),
    }),
    h(
      "table",
      null,
      h("thead", null, h("tr", null, h("th", null, "Title"), h("th", null, "Date"))),
      h(
        "tbody",
        null,
        rows.map((r) =>
          h("tr", { key: r.href }, h("td", null, h("a", { href: r.href }, r.title)), h("td", null, r.date)),
        ),
      ),
    ),
    h("details", null, h("summary", null, "Static list"), children),
  );
}
