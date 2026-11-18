#!/usr/bin/env bash
# Check that this fork still keeps what makes it a fork.
#
# Run after taking anything from upstream, before committing. It reports
# what upstream's naming would have clobbered, and what the Vietnamese
# locale needs to stay valid.
#
# Usage:
#   check-invariants.sh [--build]
#
#   --build   also run the tests and lints CI runs
#
# Exit status is 1 when something is wrong.

set -uo pipefail

build=0
[[ "${1:-}" == "--build" ]] && build=1

cd "$(git rev-parse --show-toplevel)" || exit 2

fail=0
pass() { printf '  ok    %s\n' "$*"; }
bad()  { printf '  FAIL  %s\n' "$*"; fail=1; }

echo "on-disk formats kept from before the rename"

# A phone that already has a second system installed, or a backup made
# before the rename, is read through these exact spellings.
check_literal() {
  if grep -rqF "$1" "$2"; then pass "$1 in $2"; else bad "$1 is gone from $2"; fi
}
check_literal 'SKYWARD AREA' crates/aweyuki-block/src/area.rs
check_literal 'SKYWARD.project' crates/aweyuki-backup/src/archive.rs
check_literal 'SKYWARD.project-inherit' crates/aweyuki-backup/src/archive.rs
check_literal 'SKYWARD.key-root' crates/aweyuki-backup/src/archive.rs

echo "the signed fixture is still the text that was signed"

# The zip under crates/aweyuki-install/tests/signature/ was signed upstream
# with this text inside it; rewriting it makes the signature test fail for
# the wrong reason.
if grep -rqF 'Skyward test package' crates/aweyuki-install/src/signature.rs; then
  pass 'the fixture text still reads Skyward'
else
  bad 'the fixture text was rewritten; the signed zip no longer matches'
fi

echo "no upstream naming left in code"

# Everything the rename covers. Mentions in prose that explain the history
# are allowed and listed below.
leftovers=$(git grep -In -i 'skyward' -- 'crates' '*.toml' 'Cargo.lock' '*.yml' |
  grep -vF 'SKYWARD AREA' |
  grep -vF 'SKYWARD.project' |
  grep -vF 'SKYWARD.project-inherit' |
  grep -vF 'SKYWARD.key-root' |
  grep -vF 'Skyward test package' |
  grep -vF 'Kept from Skyward' || true)
if [[ -z "$leftovers" ]]; then
  pass 'crates and manifests name only Aweyuki'
else
  bad 'upstream naming survives:'
  printf '%s\n' "$leftovers" | head -20 | sed 's/^/          /'
fi

echo "the updater looks at this fork"

if grep -qF 'AweYuki/aweyuki_recovery' crates/aweyuki-net/src/github.rs; then
  pass 'GitHub releases are asked of this fork'
else
  bad 'the updater points somewhere else'
fi
if grep -qF 'format!("aweyuki-{tag}-{codename}-")' crates/aweyuki-net/src/github.rs; then
  pass 'release assets are matched by the fork prefix'
else
  bad "release asset prefix is not aweyuki-; upstream's zips would be offered"
fi

echo "the Vietnamese locale is intact"

if grep -q 'Vi,' crates/aweyuki-ui/src/lang.rs && grep -q '\[Lang; 4\]' crates/aweyuki-ui/src/lang.rs; then
  pass 'Lang has four languages'
else
  bad 'Lang lost Vietnamese or gained a language; check Lang::ALL and every match on Lang'
fi
if grep -qF '$vi:literal' crates/aweyuki-ui/src/lang.rs; then
  pass 'tr! takes the Vietnamese text'
else
  bad 'tr! no longer takes a fourth argument; every call site needs it'
fi
if grep -q 'Lang::Vi => vi::text(hans, en)' crates/aweyuki-ui/src/lang.rs; then
  pass 'pick looks Vietnamese up by its Chinese and English texts'
else
  bad 'pick no longer falls back to the table; untranslated text will not show'
fi
if grep -q 'lang: Lang::En,' crates/aweyuki-ui/src/app/state.rs && ! grep -q 'info\.locale' crates/aweyuki/src/ui.rs; then
  pass 'recovery defaults to English and ignores ro.product.locale'
else
  bad 'default language reverted from English or ro.product.locale was put back in ui.rs'
fi

python3 - "${PWD}" <<'PY' || fail=1
import re, sys, pathlib
root = pathlib.Path(sys.argv[1])
table = root / 'crates/aweyuki-ui/src/lang/vi.rs'
text = table.read_text()
ctx_arms = re.findall(r'^\s*\("((?:[^"\\]|\\.)*)",\s*"((?:[^"\\]|\\.)*)"\)\s*=>\s*"((?:[^"\\]|\\.)*)",', text, re.M)
arms = re.findall(r'^\s*"((?:[^"\\]|\\.)*)" => "((?:[^"\\]|\\.)*)",', text, re.M)
if not arms:
    print('  FAIL  the Vietnamese table holds no entry')
    sys.exit(1)
keys = [k for k, _ in arms]
ctx_keys = [(h, e) for h, e, _ in ctx_arms]
dupes = {k for k in keys if keys.count(k) > 1} | {k for k in ctx_keys if ctx_keys.count(k) > 1}
all_pairs = [(e, v) for _, e, v in ctx_arms] + arms
placeholders = [(k, v) for k, v in all_pairs if sorted(re.findall(r'\{[^}]+\}', k)) != sorted(re.findall(r'\{[^}]+\}', v))]
problems = 0
if dupes:
    print(f'  FAIL  {len(dupes)} duplicate key(s) in the table, which will not compile')
    problems += 1
else:
    print(f'  ok    {len(arms)} entries and {len(ctx_arms)} contextual entries, no duplicate')
if placeholders:
    print(f'  FAIL  {len(placeholders)} entry/entries lost or renamed a {{placeholder}}')
    for k, v in placeholders[:5]:
        print(f'          {k[:60]!r} -> {v[:60]!r}')
    problems += 1
else:
    print('  ok    every entry keeps its placeholders')
if not re.search(r'_ => en,', text):
    print('  FAIL  the table lost its fallback arm')
    problems += 1
sys.exit(1 if problems else 0)
PY

if ((build)); then
  echo "tests and lints"
  cargo test --workspace >/tmp/upstream-sync-test.log 2>&1 && pass 'cargo test --workspace' || { bad 'cargo test failed; see /tmp/upstream-sync-test.log'; }
  cargo clippy --workspace --all-targets -- -D warnings >/tmp/upstream-sync-clippy.log 2>&1 && pass 'cargo clippy -D warnings' || { bad 'clippy failed; see /tmp/upstream-sync-clippy.log'; }
fi

echo
if ((fail)); then
  echo "something a merge can break is broken; see the lines marked FAIL"
  exit 1
fi
echo "the fork's invariants hold"
