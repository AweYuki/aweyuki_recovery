#!/usr/bin/env bash
# Rewrite an upstream Skyward patch so it applies to Aweyuki Recovery.
#
# Upstream (github.com/kirklin/skyward) still calls everything "skyward".
# This fork renamed crates, binaries and runtime paths to "aweyuki", and
# kept four on-disk formats spelled "SKYWARD" so that existing phones and
# backups stay readable. This script maps the former and protects the
# latter, then either prints the patch or applies it.
#
# Usage:
#   rewrite-patch.sh <rev> [--apply] [-- <pathspec>...]
#
#   <rev>       a commit, or a range such as upstream/main~3..upstream/main
#   --apply     apply the rewritten patch with `git apply -3` instead of
#               printing it to stdout
#   -- <paths>  only the named paths, handed to `git diff`
#
# Examples:
#   rewrite-patch.sh upstream/main~1 > /tmp/up.patch
#   rewrite-patch.sh 1a2b3c4 --apply
#   rewrite-patch.sh upstream/main~5..upstream/main -- crates/skyward/src
#
# The rewritten patch is left at $PATCH_OUT when it does not apply.

set -euo pipefail

rev=""
apply=0
paths=()

while (($#)); do
  case "$1" in
    --apply) apply=1; shift ;;
    --) shift; paths=("$@"); break ;;
    *) rev="$1"; shift ;;
  esac
done

if [[ -z "$rev" ]]; then
  echo "usage: rewrite-patch.sh <rev> [--apply] [-- <pathspec>...]" >&2
  exit 64
fi

# `git diff A..B` on a single commit would compare it with the working
# tree; a lone commit means "that commit and its parent".
if [[ "$rev" != *..* ]]; then
  rev="$rev^..$rev"
fi

PATCH_OUT="${PATCH_OUT:-$(mktemp -t upstream-XXXXXX.patch)}"

if ((${#paths[@]})); then
  git diff --binary --full-index "$rev" -- "${paths[@]}" >"$PATCH_OUT"
else
  git diff --binary --full-index "$rev" >"$PATCH_OUT"
fi

if [[ ! -s "$PATCH_OUT" ]]; then
  echo "nothing changed in $rev" >&2
  exit 0
fi

# The formats already written on phones and in backups. Renaming any of
# these makes a second system's area unreadable or a backup unrestorable,
# so they are held aside under a number and put back verbatim.
#
# `Skyward test package` is text inside a zip that was signed upstream;
# the signature covers it, so it cannot be rewritten either.
#
# The placeholders hold no letters on purpose: a placeholder carrying the
# word "skyward" would be rewritten by the catch-all rules below.
if command -v perl >/dev/null 2>&1; then
  perl -0pi -e '
    # Hold the on-disk formats, the signed fixture and the old repository
    # address aside.
    s{SKYWARD AREA}{\x01P0\x01}g;
    s{SKYWARD\.project-inherit}{\x01P1\x01}g;
    s{SKYWARD\.project}{\x01P2\x01}g;
    s{SKYWARD\.key-root}{\x01P3\x01}g;
    s{Skyward test package}{\x01P4\x01}g;
    s{kirklin/skyward}{\x01P5\x01}g;

    # Crate directories, then crate identifiers.
    s{crates/skyward-}{crates/aweyuki-}g;
    s{crates/skyward/}{crates/aweyuki/}g;
    s{\bskyward_}{aweyuki_}g;

    # Binaries, longest name first so `skyward-pack` does not become `aweyuki-pack`.
    s{\bskyward-system-init\b}{aweyuki-system-init}g;
    s{\bskyward-preview\b}{aweyuki-preview}g;
    s{\bskyward-pack\b}{aweyuki-pack}g;
    s{\bskyward\b}{aweyuki}g;

    # Runtime paths in the recovery ramdisk and on /metadata and /data.
    s{/skyward/}{/aweyuki/}g;
    s{/metadata/skyward}{/metadata/aweyuki}g;
    s{/data/skyward}{/data/aweyuki}g;
    s{Skyward/backups}{Aweyuki/backups}g;
    s{Skyward/logs}{Aweyuki/logs}g;

    # Prose and UI text last, so the paths above are already done.
    s{Skyward}{Aweyuki}g;
    s{skyward}{aweyuki}g;

    # Put the held strings back. The old repository address points at this
    # fork instead, since that is where the updater looks for releases.
    s{\x01P0\x01}{SKYWARD AREA}g;
    s{\x01P1\x01}{SKYWARD.project-inherit}g;
    s{\x01P2\x01}{SKYWARD.project}g;
    s{\x01P3\x01}{SKYWARD.key-root}g;
    s{\x01P4\x01}{Skyward test package}g;
    s{\x01P5\x01}{AweYuki/aweyuki_recovery}g;
  ' "$PATCH_OUT"
else
  python3 - "$PATCH_OUT" <<'PY'
import pathlib, re, sys
p = pathlib.Path(sys.argv[1])
s = p.read_bytes().decode("latin1")
holds = [
    ("SKYWARD AREA", "\x01P0\x01", "SKYWARD AREA"),
    ("SKYWARD.project-inherit", "\x01P1\x01", "SKYWARD.project-inherit"),
    ("SKYWARD.project", "\x01P2\x01", "SKYWARD.project"),
    ("SKYWARD.key-root", "\x01P3\x01", "SKYWARD.key-root"),
    ("Skyward test package", "\x01P4\x01", "Skyward test package"),
    ("kirklin/skyward", "\x01P5\x01", "AweYuki/aweyuki_recovery"),
]
for src, ph, _ in holds:
    s = s.replace(src, ph)
s = s.replace("crates/skyward-", "crates/aweyuki-").replace("crates/skyward/", "crates/aweyuki/")
s = re.sub(r"\bskyward_", "aweyuki_", s)
s = re.sub(r"\bskyward-system-init\b", "aweyuki-system-init", s)
s = re.sub(r"\bskyward-preview\b", "aweyuki-preview", s)
s = re.sub(r"\bskyward-pack\b", "aweyuki-pack", s)
s = re.sub(r"\bskyward\b", "aweyuki", s)
s = s.replace("/skyward/", "/aweyuki/").replace("/metadata/skyward", "/metadata/aweyuki").replace("/data/skyward", "/data/aweyuki")
s = s.replace("Skyward/backups", "Aweyuki/backups").replace("Skyward/logs", "Aweyuki/logs")
s = s.replace("Skyward", "Aweyuki").replace("skyward", "aweyuki")
for _, ph, dst in holds:
    s = s.replace(ph, dst)
p.write_bytes(s.encode("latin1"))
PY
fi

if ((apply)); then
  if git apply -3 --whitespace=nowarn "$PATCH_OUT"; then
    echo "applied $rev" >&2
    rm -f "$PATCH_OUT"
  else
    echo "did not apply cleanly; the rewritten patch is at $PATCH_OUT" >&2
    echo "resolve the files it names, then: git apply -3 $PATCH_OUT" >&2
    exit 1
  fi
else
  cat "$PATCH_OUT"
  echo "rewritten patch: $PATCH_OUT" >&2
fi
