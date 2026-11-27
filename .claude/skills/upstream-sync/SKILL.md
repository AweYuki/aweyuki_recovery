---
name: upstream-sync
description: Take a feature or fix from the upstream Skyward repository into this Aweyuki Recovery fork without breaking the rename, the Vietnamese locale, or the on-disk formats phones and backups already use. Use when asked to pull, merge, cherry-pick, backport or port an upstream commit or PR, to check what upstream has that this fork lacks, or when a merge from upstream leaves conflicts.
---

# Taking work from upstream and community forks

This repository is a fork of `github.com/kirklin/skyward`, and also aggregates
device configurations and compatibility fixes from community forks under
`github.com/kirklin/skyward/forks`. The remote `upstream` points to
`kirklin/skyward`; `origin` points at `github.com/AweYuki/aweyuki_recovery`.
Upstream and community forks call everything "skyward". This fork renamed it to
"aweyuki", added a Vietnamese locale, AERA `.aerap` plugins, and regenerated the
documentation images.

Almost every file differs from upstream — mostly by the rename. That makes a
plain `git merge upstream/main` unworkable: expect thousands of conflicts for
one upstream change. Take upstream work in small pieces and let the scripts
here do the mechanical part.

## What must not change

These are not style choices. Each one has a reason outside the source tree.

| Thing | Why it stays |
| --- | --- |
| `b"SKYWARD AREA\0\0\0\x01"` in `crates/aweyuki-block/src/area.rs` | The magic written at the head of every area. A phone with a second system already installed has areas carrying it; renaming it makes that system's root and boot store unreadable. |
| `AWE.project`, `AWE.project-inherit`, `AWE.key-root` in `crates/aweyuki-backup/src/archive.rs` (with `SKYWARD.*` fallback on read) | Extended-header names in backup tars. New backups write `AWE.*`, and `SKYWARD.*` is still accepted on restore so backups made before the rename restore their project quota and fscrypt key roots. |
| `Skyward test package` in `crates/aweyuki-install/src/signature.rs` | Text inside `crates/aweyuki-install/tests/signature/signed-*.zip`, which was signed upstream. Rewriting the literal makes the signature test fail for a reason that has nothing to do with the test. |
| 15-language `crates/aweyuki-ui/locales/*.toml` + `build.rs` + `texts.rs` | Fork-only UI strings live under `[fork.*]` and `[backup.part] efisp` in all 15 `locales/*.toml` files (`en.toml` is the reference; `vi.toml` carries Vietnamese; others fall back to English for fork-only keys). |
| `lang: Lang::En` in `crates/aweyuki-ui/src/app/state.rs` and `request.locale.as_deref()` alone in `crates/aweyuki/src/ui.rs` | Recovery defaults to English unless a language was chosen on the Language page, requested via `--locale` in BCB, or spoken last; `ro.product.locale` is ignored because China ROMs pin it to `zh-CN`. |
| `AweYuki/aweyuki_recovery` and the `aweyuki-` release prefix in `crates/aweyuki-net/src/github.rs` | That is where this fork's releases are. Upstream's address would offer upstream's images to phones running this build. |

`scripts/check-invariants.sh` tests all of these. Run it before every commit.

## Before you start

1. **The working tree must be clean.** `git status --short` has to print
   nothing. The fork's own work has to be committed, or there is no base to
   apply upstream work onto. If it is not, stop and ask whether to commit it
   first — do not stash someone's work silently.
2. The toolchain is pinned by `rust-toolchain.toml` (Rust 1.100.0 with
   clippy). `rustup toolchain install` uses it. CI also needs the Android
   NDK r29 and `cargo-ndk 4.1.2`; only the on-host checks below are needed to
   accept a merge.
3. `git fetch upstream --tags` before looking at anything, so you are not
   reasoning about a stale `upstream/main`.
4. To take commits from a **community fork** (such as `Flicker-Android-Devices/skyward-mikona`
   or any fork under `github.com/kirklin/skyward/forks`), add it as a temporary
   remote (`git remote add fork-<name> <url> && git fetch fork-<name>`) or fetch
   into a bare scratch repository, then run `rewrite-patch.sh <sha> --apply`
   against those commits in topological order.

## Automated fork scanning for new devices

To discover new `devices/*.toml` files across all forks of `kirklin/skyward`,
normalize any incompatible sections against `aweyuki-config`, and update the
community device tables in all three READMEs (`README.md`, `README.vi.md`,
`README.zh-CN.md`):

```sh
# Report newly discovered devices across all forks
python3 .claude/skills/upstream-sync/scripts/scan-forks.py --report

# Fetch, normalize, and write new devices/*.toml files, then update README tables
python3 .claude/skills/upstream-sync/scripts/scan-forks.py --apply --update-readme
```

Every ported `devices/*.toml` keeps a `# Ported from: https://github.com/<owner>/<repo>/tree/<branch>`
attribution comment at the top of the file; `check-invariants.sh` explicitly
allows `# Source:` and `# Ported from:` comments in `devices/*.toml`.

## Procedure

### 1. Survey

```sh
git log --oneline HEAD..upstream/main
git diff --stat HEAD...upstream/main | tail -1
```

When the user names a pull request rather than a commit, find it by its
number in the merge commit's subject:

```sh
git log --oneline --grep='#52' upstream/main
```

Settle the unit of work with the user if it is not obvious: one upstream
commit, one PR's commits, a range, or "everything since last sync".

### 2. Pick a route

**Route A — cherry-pick, when the commit is small and untouched by the
rename.** Try it first; it is the only route that keeps the upstream commit
id and author.

```sh
git cherry-pick -x <sha>
```

Git's rename detection often finds `crates/skyward-sys/...` →
`crates/aweyuki-sys/...` because the content is otherwise identical. If it
does, resolve any textual conflicts and skip to step 4.

**Route B — rewrite the patch, when cherry-pick produces conflicts.** This is
the default for anything touching more than one or two files.

```sh
.claude/skills/upstream-sync/scripts/rewrite-patch.sh <sha> --apply
# or, for several commits at once or only some paths:
.claude/skills/upstream-sync/scripts/rewrite-patch.sh upstream/main~5..upstream/main -- crates/skyward/src
```

The script maps crate directories, crate identifiers, binary names, runtime
paths and prose from skyward to aweyuki, holds the on-disk formats aside so
they survive verbatim, and points the old repository address at this fork.
Without `--apply` it prints the patch and says where it was saved.

**Route C — read and retype, when the change is a rewrite upstream rather
than an addition.** If upstream restructured the file you need to change,
a patch will apply in the wrong shape. Read the upstream version, then make
the same change here by hand:

```sh
git show upstream/main:crates/skyward-ui/src/lang.rs
```

### 3. Resolve what the rewrite cannot know

The script handles names, not meaning. Work through the cookbook below.

### 4. Verify

```sh
.claude/skills/upstream-sync/scripts/check-invariants.sh --build
```

This checks the invariants above and runs `cargo test --workspace` and
`cargo clippy --workspace --all-targets -- -D warnings`, the same gates CI
uses. For a change that only touched the host-only crates, `cargo check
--workspace --all-targets` first is faster feedback.

If the change touched anything the pages draw, also render the Vietnamese
pages and look at one:

```sh
cargo run --release -p aweyuki-preview -- target/preview --lang vi
```

### 5. Commit

Say where it came from in the message, and keep upstream's commit id when
Route A was used (`-x` does that). Route B and C lose the author, so record
it:

```
fix(dada): find its key programs' libraries where Android looks

Upstream-Commit: cc68936f4b6f454da1a3e0aa8aa990f009567415
```

Do not commit generated images unless the change is about those images.
Afterwards `git log --oneline upstream/main ^HEAD` should be shorter by
exactly what you took.

## The rename, in one table

`scripts/rewrite-patch.sh` applies this. Use it when editing by hand (Route C).

| Upstream | Here |
| --- | --- |
| `crates/skyward-*/` | `crates/aweyuki-*/` |
| `skyward_install::…` and every other `skyward_` identifier | `aweyuki_…` |
| binaries `skyward`, `skyward-pack`, `skyward-preview`, `skyward-system-init` | `aweyuki`, `aweyuki-pack`, `aweyuki-preview`, `aweyuki-system-init` |
| `/skyward/…` in the ramdisk | `/aweyuki/…` |
| `/metadata/skyward`, `/data/skyward` | `/metadata/aweyuki`, `/data/aweyuki` |
| `Skyward/backups`, `Skyward/logs` | `Aweyuki/backups`, `Aweyuki/logs` |
| `Skyward` in UI text, docs, comments | `Aweyuki` |
| `kirklin/skyward` in URLs | `AweYuki/aweyuki_recovery` |

Everything not in the right column stays as upstream wrote it, including
the four `SKYWARD` spellings listed above and the signed fixture's text.

## Conflict cookbook

| Upstream change | What to do here |
| --- | --- |
| A new `l.pick([hans, hant, en])` call | Compiles as is, and `every_text_picked_has_a_vietnamese_translation` in `lang.rs` flags it in `cargo test`. Add the entry to `crates/aweyuki-ui/src/lang/vi.rs` (under `(hans, en)` when `en` collides with another meaning, otherwise under `en`). |
| A new `tr!(l, hans, hant, en)` call | Add a fourth argument. `tr!` rejects three here. Run `cargo check` to find every one. |
| `pick` used with a non-`&str` value, as upstream's generic `pick<T>` allows | Keep the narrowed signature and adapt the call site — map the value to a `&'static str` before `pick`, as `soc()` in `pages.rs` does. Do not re-generalise `pick`: `T` cannot return the table's `&'static str` fallback. |
| A new `Lang` variant or a new `Text` type | Adds a match arm everywhere `Lang` is matched. Keep `Lang::ALL` at four entries unless the user is adding a language, and extend `Lang::for_locale`, `name` and `tag` together. |
| New UI text from upstream | Add it to `lang/vi.rs` keyed by the exact English string, placeholders unchanged. The typeface test only catches missing glyphs, not missing translations. |
| Changes to `slides/en/pages/*.html` | These are hand-translated into `slides/vi/pages/*.html`; a change does not carry over. Update both, re-render (see the repo README), and keep the Vietnamese headline within two lines — `slides/readme.css` gives `lang="vi"` a smaller headline for that. |
| Changes to `README.md` | Mirror the substance into `README.zh-CN.md` and `README.vi.md`. The Vietnamese README points at `slides/vi/out` and `assets/vi`. |
| A change to a test fixture that hardcodes the old name | Fixtures are data, not names. Upstream's SSID test uses a hex-encoded name; our copy encodes `Aweyuki` instead. Recompute the bytes rather than editing the expected hex by eye: `python3 -c "print('Aweyuki'.encode().hex())"`. |
| A change to `crates/skyward-pack/tests/avb-footer-*.bin` | Those binaries carry `aweyuki-pack` in their release string, same length as upstream's. Apply the same one-word substitution to the new bytes, or the byte-for-byte test fails. |
| Upstream adds or renames a file | The rewrite creates the new file under `crates/aweyuki-…`. If upstream renamed a file, `git mv` our copy to match, so later upstream changes keep lining up. |
| Upstream bumps a dependency | Let `cargo` regenerate `Cargo.lock`. Do not hand-merge it; the package names differ from upstream's. |
| Upstream changes what goes on disk | Stop. Ask the user. Extending the area or backup formats means also making old data readable, which is a design decision, not a merge. |

## Traps

- **Do not `git merge upstream/main` wholesale**, and do not `git rebase` onto
  it. Fourteen thousand renames against a hundred-odd modified files makes
  conflict resolution guesswork. One commit at a time is slower to type and
  far faster to finish.
- **Do not run the rename in reverse.** `rewrite-patch.sh` rewrites upstream's
  text forward. Renaming this repo's files back to `skyward` to "make the
  patch apply" loses the fork.
- **Watch for the catch-all.** Upstream prose that mentions `Skyward` becomes
  `Aweyuki` in the patch. That is right for UI text and wrong for the four
  on-disk spellings — the script protects those, but check
  `check-invariants.sh` output rather than trusting it.
- **`/aweyuki/...` paths matter at runtime.** A missed path under `/skyward`
  builds and tests fine and fails on the phone when the recovery looks for its
  own files. `git grep -n '/skyward'` after merging.
- **A missing Vietnamese entry is silent.** It falls back to English, which
  looks like a translation bug only on the phone.
