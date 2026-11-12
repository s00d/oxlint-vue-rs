#!/usr/bin/env bash
# Wall-clock benchmark: oxlint-vue-rs vs eslint-plugin-vue vs oxlint-vue (TS) on corpora of .vue files.
#
# usage: scripts/bench.sh <corpora-dir> <ref-node-dir> <eslint-config> [runs]
#   <corpora-dir>   directory whose subdirectories are corpora
#   <ref-node-dir>  directory with node_modules holding eslint, eslint-plugin-vue, oxlint, oxlint-vue
#   <eslint-config> flat config enabling the same vue/* rules (lives in a parent dir of the corpora)
# Every tool gets a neutral config (project configs are ignored). Prints best-of-N wall seconds and the
# number of reported problems (sanity check that the tool really ran).
set -euo pipefail
CORPORA=${1%/}
REF=${2%/}
ESLINT_CFG=$3
RUNS=${4:-3}
BIN="$(cd "$(dirname "$0")/.." && pwd)/target/release/oxlint-vue-rs"
[ -x "$BIN" ] || { echo "build first: cargo build --release" >&2; exit 2; }
export PATH="$REF/node_modules/.bin:$PATH"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
echo '{}' > "$TMP/empty.oxlintrc.json"
echo '{"plugins":["vue"]}' > "$TMP/vue.oxlintrc.json"

now() { perl -MTime::HiRes=time -e 'printf "%.3f", time'; }
# best <outfile> cmd... : best wall time of N runs; last run's stdout kept in <outfile>
best() {
  local out=$1; shift
  local b=999999 s e
  for _ in $(seq "$RUNS"); do
    s=$(now); "$@" > "$out" 2> "$out.err" || true; e=$(now)
    b=$(perl -e "printf '%.3f', ($e - $s) < $b ? ($e - $s) : $b")
  done
  echo "$b"
}
count_json_oxlint() { node -e 'try{const j=JSON.parse(require("fs").readFileSync(process.argv[1],"utf8"));console.log(Array.isArray(j)?j.length:j.diagnostics.length)}catch{console.log("ERR")}' "$1"; }
count_json_eslint() { node -e 'try{const j=JSON.parse(require("fs").readFileSync(process.argv[1],"utf8"));console.log(j.reduce((a,f)=>a+f.messages.length,0))}catch{console.log("ERR")}' "$1"; }

printf '%-18s %5s | %-16s | %-16s | %-16s | %-16s\n' corpus files "ours (s/probs)" "ours+oxlint" "oxlint-vue (TS)" "eslint-plugin-vue"
for dir in "$CORPORA"/*; do
  [ -d "$dir" ] || continue
  c=$(basename "$dir")
  n=$(find -L "$dir" -name '*.vue' -not -path '*/node_modules/*' | wc -l | tr -d ' ')
  t1=$(best "$TMP/o1" "$BIN" -c "$TMP/empty.oxlintrc.json" --no-ignore --hidden -D all -f json "$dir")
  t2=$(best "$TMP/o2" "$BIN" -c "$TMP/vue.oxlintrc.json" --no-ignore --hidden -D all -f json --with-oxlint "$dir")
  t3=$(cd "$TMP" && best "$TMP/o3" "$REF/node_modules/.bin/oxlint-vue" -c "$TMP/empty.oxlintrc.json" -f json "$dir")
  t4=$(cd "$(dirname "$ESLINT_CFG")" && best "$TMP/o4" "$REF/node_modules/.bin/eslint" --no-config-lookup -c "$ESLINT_CFG" --no-ignore -f json "$dir/**/*.vue" --ignore-pattern '**/node_modules/**')
  printf '%-18s %5s | %7s %8s | %7s %8s | %7s %8s | %7s %8s\n' "$c" "$n" \
    "$t1" "$(count_json_oxlint "$TMP/o1")" "$t2" "$(count_json_oxlint "$TMP/o2")" \
    "$t3" "$(count_json_oxlint "$TMP/o3")" "$t4" "$(count_json_eslint "$TMP/o4")"
done
