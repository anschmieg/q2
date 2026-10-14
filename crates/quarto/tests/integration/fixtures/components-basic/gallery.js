// A `content: ast` component: walks the node's Pandoc JSON for images and
// renders them as a strip, keeping the static list underneath.
import { createElement as h } from "react";

function images(node, out = []) {
  if (Array.isArray(node)) {
    for (const n of node) images(n, out);
  } else if (node && typeof node === "object") {
    if (node.t === "Image") out.push({ src: node.c[2][0], alt: node.c[1].map((i) => i.c || " ").join("") });
    else if (node.c !== undefined) images(node.c, out);
  }
  return out;
}

export default function Gallery({ node, children }) {
  const imgs = images(node);
  return h(
    "div",
    { className: "gallery", "data-image-count": String(imgs.length) },
    h("div", { className: "strip" }, imgs.map((i) => h("img", { key: i.src, src: i.src, alt: i.alt, width: 32 }))),
    children,
  );
}
