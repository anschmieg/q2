# Quarto 1 `search.json` sizes on live sites (measured 2026-10-14)

Fetched `<site>/search.json` with curl; gzip -9 for the wire size. Entry = one
Quarto 1 index record (a page or a level-2 section). Text chars = sum of `text`.

| site | pages | entries | raw | gzip -9 | mean text/entry |
|---|---:|---:|---:|---:|---:|
| https://quarto.org | 241 | 1738 | 2.54 MB | 591 KB | 1134 chars |
| https://r4ds.hadley.nz | 38 | 254 | 977 KB | 275 KB | ~3.8 KB |
| https://mlr3book.mlr-org.com | 23 | 167 | 1.08 MB | 306 KB | ~6.4 KB |

Observations:
- quarto.org's index is ~2 M chars of text for 241 pages; the longest single
  entry is 39,794 chars (uncapped section text).
- Books (r4ds, mlr3book) have few pages but very long sections, so bytes per
  page is ~25-45 KB.
- Quarto 1 fetches the whole file on the first keystroke and builds a Fuse index
  in memory; nothing is prebuilt. A 5,000-page site at quarto.org's density
  would be ~50 MB raw / ~12 MB gzipped.
