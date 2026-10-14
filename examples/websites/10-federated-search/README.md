# 10-federated-search — One search box, three websites

Demonstrates federated website search: an umbrella site whose search
box also queries two separately rendered product sites.

## What this demonstrates

- **Three independent websites.** `umbrella/`, `product-a/`, and
  `product-b/` are ordinary Quarto 2 website projects. Each renders on
  its own and gets its own search index (`site_libs/quarto-search/`).
- **`website.search.federate`.** The umbrella's `_quarto.yml` lists
  the two product sites by URL. Its search box queries all three
  indexes at once; results from a product site carry that site's
  name, and following one leaves the umbrella for that site.
- **One output tree.** Each project's `output-dir` points into a
  shared `www/` directory (`../www`, `../www/product-a`,
  `../www/product-b`), so one static server serves the three sites
  under one origin and the umbrella can reach the others with the
  relative URLs `/product-a/` and `/product-b/`.
- **The manifest check.** Before merging a site's index, the browser
  reads that site's `site_libs/quarto-search/manifest.json` and skips
  it when the Pagefind version differs from the umbrella's. All three
  sites here are rendered by the same `q2`, so they always match.

## How to run

From the repository root, render the three sites (products first is
not required; any order works):

```bash
cargo build --bin q2
for site in product-a product-b umbrella; do
  cargo run --bin q2 -- render examples/websites/10-federated-search/$site
done
```

Serve the shared tree with any static server, for example:

```bash
cd examples/websites/10-federated-search/www
python3 -m http.server 8000
```

Open <http://localhost:8000/> and use the search box in the navbar.
The home page lists words to try:

| Query | Expected |
|---|---|
| `umbrella` | Only the umbrella home page. |
| `lighthouse` | Product A's *Collector guide*, labelled "Product A". |
| `tidepool` | Product B's *Chart reference*, labelled "Product B". |
| `install` | One result per site, each with its site label. |

Search needs a web server; opening `www/index.html` from the file
system shows an error card instead of results.

`www/` and the projects' `.quarto/` caches are gitignored.

## Serving the sites on different origins

The same configuration works when the product sites live elsewhere,
with two changes:

1. Use absolute URLs in `federate:` (`https://docs.example.org/product-a/`).
2. Have the product sites serve `site_libs/quarto-search/` with an
   `Access-Control-Allow-Origin` header that admits the umbrella's
   origin. `q2 preview --static` does this for you, so you can try it
   locally: preview each product site on its own port, point the
   umbrella's `federate:` URLs at those ports, and preview the
   umbrella.

See `docs/guides/projects/website-search.qmd` for the full option
reference.
