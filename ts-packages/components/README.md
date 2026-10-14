# @quarto/components

Type-only contract for React components bound to Quarto document elements
(`components:` in a document's metadata; see the components guide in
`docs/guides/authoring/components.qmd`). A component receives
`{ attrs, props, children, node?, host }`, the same shape whether the page
runtime mounts it over `q2 render`'s static markup or the q2 preview
dispatches on the element.

```tsx
import { defineComponent } from "@quarto/components";

export default defineComponent<{ interval?: number }>(({ props, children }) => (
  <div className="carousel" data-interval={props.interval ?? 3000}>{children}</div>
));
```

The runtime that calls components lives in
`resources/js/quarto-components/quarto-components.js`; the constants here
name the attributes and event it uses.
