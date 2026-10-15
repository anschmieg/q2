#!/usr/bin/env bash
set -euo pipefail
root="$PWD"
q2="$root/target/debug/q2"
out="$root/m1-verification"
mkdir -p "$out"
test -x "$q2" || { echo "native q2 binary missing" >&2; exit 1; }
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
proj="$tmp/project"
mkdir -p "$proj"
cat >"$proj/_quarto.yml" <<'EOF'
project:
  type: website
  output-dir: _site
website:
  title: M1 Native Parity
  sidebar:
    contents: [index.qmd, sibling.qmd]
EOF
cat >"$proj/index.qmd" <<'EOF'
---
title: Home
---

# Root

{{< include outer.qmd >}}
EOF
cat >"$proj/outer.qmd" <<'EOF'
## Outer

{{< include leaf.qmd >}}
EOF
printf 'M1-LEAF-INITIAL\n' >"$proj/leaf.qmd"
cat >"$proj/sibling.qmd" <<'EOF'
---
title: Sibling
---

# Sibling

Unrelated stable content.
EOF
export SOURCE_DATE_EPOCH=1760000000 TZ=UTC
"$q2" --version >"$out/toolchain.txt"
rustc --version >>"$out/toolchain.txt"
printf 'mode\twall_ms\n' >"$out/timing.tsv"
render() {
  label="$1"
  shift
  t0="$(date +%s%N)"
  if ! (cd "$proj" && QUARTO_PERF_STATS=1 "$q2" render "$@") >"$out/$label.log" 2>&1; then
    echo "M1 ERROR: $label" >&2
    tail -n 80 "$out/$label.log" >&2
    return 1
  fi
  t1="$(date +%s%N)"
  printf '%s\t%s\n' "$label" "$(((t1-t0)/1000000))" >>"$out/timing.tsv"
}
digest() { sha256sum "$1" | awk '{print $1}'; }
die() { echo "M1 PARITY FAILED: $*" >&2; exit 1; }
render full_initial --clean-cache
index="$proj/_site/index.html"
sibling="$proj/_site/sibling.html"
test -s "$index" || die "missing index.html"
test -s "$sibling" || die "missing sibling.html"
grep -q M1-LEAF-INITIAL "$index" || die "initial include missing"
h_initial="$(digest "$index")"
h_sibling="$(digest "$sibling")"
t_sibling="$(stat -c %y "$sibling")"
printf 'M1-LEAF-REVISED\n' >"$proj/leaf.qmd"
render targeted_after_edit index.qmd
grep -q M1-LEAF-REVISED "$index" || die "nested include failed to update"
h_target="$(digest "$index")"
test "$h_target" != "$h_initial" || die "source edit failed to change output"
test "$h_sibling" = "$(digest "$sibling")" || die "sibling bytes changed"
test "$t_sibling" = "$(stat -c %y "$sibling")" || die "sibling was needlessly rendered"
render clean_full_final --clean-cache
h_clean="$(digest "$index")"
test "$h_target" = "$h_clean" || die "targeted HTML differs from clean full HTML"
render warm_targeted index.qmd
h_warm="$(digest "$index")"
test "$h_warm" = "$h_clean" || die "warm HTML differs from clean full HTML"
{
  echo "initial=$h_initial"
  echo "targeted=$h_target"
  echo "clean_final=$h_clean"
  echo "warm=$h_warm"
  echo "unrelated_sibling=$h_sibling"
  echo "index_html_byte_match=true"
  echo "unrelated_sibling_rewrites=0"
  echo "scope=deterministic_nested_include_html_single_artifact"
} >"$out/native-parity.txt"
cat "$out/native-parity.txt"
cat "$out/timing.tsv"
echo 'M1_NATIVE_PARITY_PASS'
