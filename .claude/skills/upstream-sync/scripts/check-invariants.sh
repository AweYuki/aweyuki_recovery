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
check_literal 'AWE.project' crates/aweyuki-backup/src/archive.rs
check_literal 'AWE.project-inherit' crates/aweyuki-backup/src/archive.rs
check_literal 'AWE.key-root' crates/aweyuki-backup/src/archive.rs

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
  grep -vF 'Kept from Skyward' |
  grep -vE '^devices/[^:]+:[0-9]+:#.*https?://' || true)
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

echo "the Vietnamese locale and 15-language TOML catalog are intact"

if grep -q 'language("Vi", "vi"' crates/aweyuki-ui/build.rs && [[ $(ls -1 crates/aweyuki-ui/locales/*.toml | wc -l) -eq 15 ]]; then
  pass '15 language TOML catalogs including Vietnamese are present'
else
  bad 'missing Vietnamese or unexpected locale count in crates/aweyuki-ui/locales/'
fi
if grep -q 'lang: Lang::En,' crates/aweyuki-ui/src/app/state.rs && ! grep -q 'info\.locale' crates/aweyuki/src/ui.rs; then
  pass 'recovery defaults to English and ignores ro.product.locale'
else
  bad 'default language reverted from English or ro.product.locale was put back in ui.rs'
fi

python3 - "${PWD}" <<'PY' || fail=1
import re, sys, pathlib, tomllib
root = pathlib.Path(sys.argv[1])
vi_path = root / 'crates/aweyuki-ui/locales/vi.toml'
en_path = root / 'crates/aweyuki-ui/locales/en.toml'
try:
    vi_data = tomllib.loads(vi_path.read_text())
    en_data = tomllib.loads(en_path.read_text())
except Exception as e:
    print(f'  FAIL  TOML parse error: {e}')
    sys.exit(1)

def flatten(d, prefix=""):
    out = {}
    for k, v in d.items():
        p = f"{prefix}.{k}" if prefix else k
        if isinstance(v, dict):
            out.update(flatten(v, p))
        else:
            out[p] = v
    return out

vi_flat = flatten(vi_data)
en_flat = flatten(en_data)
missing = sorted(set(en_flat) - set(vi_flat))
# Plural keys in en.toml have .one/.other while vi.toml has .other; compare base keys for plurals
missing = [k for k in missing if not (k.rsplit('.', 1)[-1] in ('zero', 'one', 'two', 'few', 'many', 'other') and f"{k.rsplit('.', 1)[0]}.other" in vi_flat)]
placeholders = [
    (k, en_flat[k], vi_flat[k])
    for k in sorted(set(en_flat) & set(vi_flat))
    if isinstance(en_flat[k], str) and isinstance(vi_flat[k], str)
    and k.rsplit('.', 1)[-1] not in ('zero', 'one', 'two', 'few', 'many', 'other')
    and sorted(re.findall(r'\{[^}]+\}', en_flat[k])) != sorted(re.findall(r'\{[^}]+\}', vi_flat[k]))
]
problems = 0
if missing:
    print(f'  FAIL  {len(missing)} key(s) missing in locales/vi.toml: {missing[:5]}')
    problems += 1
else:
    print(f'  ok    locales/vi.toml covers all {len(vi_flat)} entries with no duplicate keys')
if placeholders:
    print(f'  FAIL  {len(placeholders)} entry/entries lost or renamed a {{placeholder}}')
    for k, e, v in placeholders[:5]:
        print(f'          {k}: {e[:40]!r} -> {v[:40]!r}')
    problems += 1
else:
    print('  ok    every entry in locales/vi.toml keeps its placeholders')
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
