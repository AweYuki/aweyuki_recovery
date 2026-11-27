#!/usr/bin/env python3
"""Scan github.com/kirklin/skyward/forks for new devices and notable fork commits.

Usage:
  scan-forks.py [--report] [--apply] [--update-readme]

Options:
  --report         List newly discovered devices and notable forks with crates/ changes.
  --apply          Fetch, normalize, and write newly discovered devices/*.toml files.
  --update-readme  Regenerate the community-ported device tables in README.md,
                   README.vi.md, and README.zh-CN.md from devices/*.toml.
"""

from __future__ import annotations

import argparse
import concurrent.futures
import json
import pathlib
import re
import subprocess
import tempfile
import threading
import tomllib
import urllib.request

SKIP_VARIANTS = {
    "RMX5060.recovery.toml",
}

CAPABILITIES = {
    "CHOWN", "DAC_OVERRIDE", "DAC_READ_SEARCH", "FOWNER", "FSETID", "KILL",
    "SETGID", "SETUID", "SETPCAP", "LINUX_IMMUTABLE", "NET_BIND_SERVICE",
    "NET_BROADCAST", "NET_ADMIN", "NET_RAW", "IPC_LOCK", "IPC_OWNER",
    "SYS_MODULE", "SYS_RAWIO", "SYS_CHROOT", "SYS_PTRACE", "SYS_PACCT",
    "SYS_ADMIN", "SYS_BOOT", "SYS_NICE", "SYS_RESOURCE", "SYS_TIME",
    "SYS_TTY_CONFIG", "MKNOD", "LEASE", "AUDIT_WRITE", "AUDIT_CONTROL",
    "SETFCAP", "MAC_OVERRIDE", "MAC_ADMIN", "SYSLOG", "WAKE_ALARM",
    "BLOCK_SUSPEND", "AUDIT_READ", "PERFMON", "BPF", "CHECKPOINT_RESTORE",
}

# Official devices maintained directly in README's primary/under-construction tables.
OFFICIAL_DEVICES = {
    "nezha", "ishtar", "socrates", "nuwa",
    "fuxi", "houji", "shennong", "aurora", "dada", "haotian", "xuanyuan",
    "pudding", "pandora", "popsicle", "byron", "zorn", "miro", "annibale",
    "myron", "warsaw", "athens", "songyuan",
}


def repo_root() -> pathlib.Path:
    out = subprocess.check_output(
        ["git", "-c", "safe.directory=*", "rev-parse", "--show-toplevel"],
        text=True,
    ).strip()
    return pathlib.Path(out)


def is_plain_abs(p: object) -> bool:
    if not isinstance(p, str) or not p.startswith("/") or p == "/":
        return False
    parts = [x for x in p.split("/") if x]
    return bool(parts) and all(x not in (".", "..") for x in parts)


def is_plain_rel(p: object) -> bool:
    if not isinstance(p, str) or not p or p.startswith("/"):
        return False
    parts = [x for x in p.split("/") if x]
    return bool(parts) and all(x not in (".", "..") for x in parts)


def validate_device_toml(text: str) -> tuple[dict | None, list[str]]:
    errs: list[str] = []
    try:
        cfg = tomllib.loads(text)
    except Exception as e:
        return None, [f"syntax:{e}"]
    dev = cfg.get("device")
    if not isinstance(dev, dict) or not isinstance(dev.get("codename"), str) or not dev.get("codename"):
        return cfg, ["codename"]
    if "soc" in dev:
        soc = dev["soc"]
        if not isinstance(soc, dict) or not all(isinstance(soc.get(k), str) for k in ("hans", "hant", "en")):
            errs.append("soc")
    if "haptics" in cfg:
        h = cfg["haptics"]
        has_click = isinstance(h.get("click"), int)
        res = h.get("resonance")
        has_res = isinstance(res, str) and res.startswith("/")
        st = h.get("strength", {})
        ok_strength = (
            isinstance(st, dict)
            and isinstance(st.get("lightest"), (int, float))
            and isinstance(st.get("strongest"), (int, float))
            and 0.0 < st["lightest"] < st["strongest"] <= 1.0
        )
        if not (has_click or has_res) or (res is not None and not has_res) or not ok_strength:
            errs.append("haptics")
    if "stock" in cfg:
        st = cfg["stock"]
        ver, pkg, rec = st.get("version", ""), st.get("package", ""), st.get("recovery", "")
        part = st.get("partition")
        name = pkg.rsplit("/", 1)[-1] if isinstance(pkg, str) else ""
        is_placeholder_url = any(x in str(pkg) for x in ("example.invalid", "example.com", "placeholder"))
        is_zero_hash = isinstance(rec, str) and len(rec) == 64 and set(rec) == {"0"}
        if not (isinstance(pkg, str) and pkg.startswith("https://") and name.endswith(".zip") and ver and ver in name) or is_placeholder_url:
            errs.append("stock")
        elif not (isinstance(rec, str) and len(rec) == 64 and all(c in "0123456789abcdef" for c in rec)) or is_zero_hash:
            errs.append("stock")
        elif part is not None and (not isinstance(part, str) or not part or "/" in part):
            errs.append("stock")
    if "wlan" in cfg:
        w = cfg["wlan"]
        if any(k not in w for k in ("modules", "interface", "supplicant", "service", "probe")):
            errs.append("wlan")
        elif not str(w["probe"]).startswith("http://"):
            errs.append("wlan")
        else:
            for fw in w.get("firmware", []):
                if not is_plain_rel(fw.get("name", "")):
                    errs.append("wlan")
                elif fw.get("partition"):
                    if "/" in fw["partition"] or not is_plain_rel(fw.get("path", "")):
                        errs.append("wlan")
                elif not str(fw.get("path", "")).startswith("/"):
                    errs.append("wlan")
    return cfg, errs


def comment_section(text: str, sec_name: str) -> str:
    lines = text.splitlines(keepends=True)
    out: list[str] = []
    in_sec = False
    sec_re = re.compile(rf"^\s*\[\{{1,2\}}{re.escape(sec_name)}(?:\.[^\]]+)?\]\{{1,2\}}\s*(#.*)?$")
    # Match [sec] or [[sec]] or [sec.sub]
    sec_re = re.compile(rf"^\s*\[{{1,2}}{re.escape(sec_name)}(?:\.[^\]]+)?\]{{1,2}}\s*(#.*)?$")
    any_sec_re = re.compile(r"^\s*\[")
    for line in lines:
        if sec_re.match(line):
            if not in_sec:
                out.append(f"# [disabled: incompatible with aweyuki-config {sec_name} schema]\n")
            in_sec = True
            out.append(f"# {line}")
            continue
        if in_sec:
            if any_sec_re.match(line):
                in_sec = False
            else:
                out.append(f"# {line}" if line.strip() else line)
                continue
        out.append(line)
    return "".join(out)


def rewrite_non_urls(text: str) -> str:
    urls: list[str] = []

    def save_url(m: re.Match[str]) -> str:
        urls.append(m.group(0))
        return f"\x00URL{len(urls) - 1}\x00"

    protected = re.sub(r"https?://[^\s\"'<>]+", save_url, text)
    protected = protected.replace("Skyward", "Aweyuki").replace("skyward", "aweyuki")
    for i, u in enumerate(urls):
        protected = protected.replace(f"\x00URL{i}\x00", u)
    return protected


def fetch_forks() -> list[dict]:
    forks: list[dict] = []
    page = 1
    while True:
        url = f"https://api.github.com/repos/kirklin/skyward/forks?per_page=100&page={page}"
        req = urllib.request.Request(url, headers={"User-Agent": "aweyuki-fork-scan", "Accept": "application/vnd.github+json"})
        with urllib.request.urlopen(req, timeout=20) as resp:
            data = json.loads(resp.read().decode())
        if not data:
            break
        forks.extend(data)
        if len(data) < 100:
            break
        page += 1
    return forks


def scan_forks(root: pathlib.Path, apply: bool = False) -> None:
    forks = fetch_forks()
    existing = {p.name for p in (root / "devices").glob("*.toml")}
    known_shas = set(
        subprocess.check_output(
            ["git", "-C", str(root), "-c", "safe.directory=*", "rev-list", "--all"],
            text=True,
        ).splitlines()
    )

    candidates = [
        f for f in forks
        if f["full_name"].lower() != "aweyuki/aweyuki_recovery"
        and (
            f["pushed_at"] > f["created_at"]
            or not f["full_name"].endswith("/skyward")
            or f.get("stargazers_count", 0) > 0
        )
    ]
    print(f"Found {len(forks)} forks ({len(candidates)} active candidates).")

    def ls_remote(f: dict) -> tuple[dict, list[tuple[str, str]]]:
        try:
            out = subprocess.check_output(
                ["git", "ls-remote", "--heads", f"https://github.com/{f['full_name']}.git"],
                stderr=subprocess.DEVNULL, text=True, timeout=15,
            )
        except Exception:
            return f, []
        ahead = []
        for line in out.splitlines():
            parts = line.split("\t", 1)
            if len(parts) == 2 and parts[0] not in known_shas:
                ahead.append((parts[1].removeprefix("refs/heads/"), parts[0]))
        return f, ahead

    tasks: list[tuple[dict, str, str]] = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=16) as ex:
        for f, ahead in ex.map(ls_remote, candidates):
            ahead.sort(key=lambda b: (0 if b[0] == f["default_branch"] else 1, b[0]))
            for bname, sha in ahead[:3]:
                tasks.append((f, bname, sha))

    tmp = pathlib.Path(tempfile.mkdtemp(prefix="aweyuki-fork-scan-"))
    tls = threading.local()
    lock = threading.Lock()
    wid = 0

    def worker_dir() -> pathlib.Path:
        nonlocal wid
        if not hasattr(tls, "dir"):
            with lock:
                i = wid
                wid += 1
            d = tmp / f"w{i}.git"
            d.mkdir(parents=True, exist_ok=True)
            subprocess.run(["git", "init", "--bare", str(d)], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True)
            tls.dir = d
        return tls.dir

    def inspect_tip(t: tuple[dict, str, str]) -> dict | None:
        f, bname, sha = t
        d = worker_dir()
        url = f"https://github.com/{f['full_name']}.git"
        try:
            subprocess.run(
                ["git", "-C", str(d), "fetch", "--no-tags", "--depth=1", "--filter=blob:none", url, f"refs/heads/{bname}"],
                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=20, check=True,
            )
            devs = subprocess.check_output(
                ["git", "-C", str(d), "ls-tree", "--name-only", "FETCH_HEAD:devices"],
                text=True, stderr=subprocess.DEVNULL, timeout=10,
            ).splitlines()
        except Exception:
            return None
        new_devs = sorted(set(x for x in devs if x.endswith(".toml")) - existing - SKIP_VARIANTS)
        return {"fork": f, "branch": bname, "sha": sha, "new_devices": new_devs}

    new_by_dev: dict[str, list[dict]] = {}
    with concurrent.futures.ThreadPoolExecutor(max_workers=12) as ex:
        for res in ex.map(inspect_tip, tasks):
            if res and res["new_devices"]:
                for dev in res["new_devices"]:
                    new_by_dev.setdefault(dev, []).append(res)

    print(f"New devices not yet in devices/: {len(new_by_dev)}")
    for dev, entries in sorted(new_by_dev.items()):
        entries.sort(key=lambda e: (e["fork"].get("stargazers_count", 0), e["fork"]["pushed_at"]), reverse=True)
        top = entries[0]
        print(f"  {dev}: {top['fork']['full_name']}@{top['branch']} ({top['fork'].get('stargazers_count', 0)}*)")
        if apply:
            url = f"https://raw.githubusercontent.com/{top['fork']['full_name']}/{top['sha']}/devices/{dev}"
            req = urllib.request.Request(url, headers={"User-Agent": "aweyuki-fork-scan"})
            with urllib.request.urlopen(req, timeout=15) as resp:
                text = resp.read().decode("utf-8", errors="replace").replace("\r\n", "\n")
            cfg, errs = validate_device_toml(text)
            if "soc" in errs and cfg:
                soc_val = str(cfg["device"]["soc"]).replace("\\", "\\\\").replace('"', '\\"')
                text = re.sub(
                    r'(?m)^(\s*soc\s*=\s*)"[^"\n]*"\s*(#.*)?$',
                    rf'\1{{ hans = "{soc_val}", hant = "{soc_val}", en = "{soc_val}" }}',
                    text, count=1,
                )
            for sec in ("haptics", "stock", "wlan"):
                if sec in errs:
                    text = comment_section(text, sec)
            if cfg and "kernel" in cfg:
                text = comment_section(text, "kernel")
            text = rewrite_non_urls(text)
            header = f"# Ported from: https://github.com/{top['fork']['full_name']}/tree/{top['branch']}\n"
            if not text.endswith("\n"):
                text += "\n"
            final_text = header + text
            _, errs2 = validate_device_toml(final_text)
            if errs2:
                print(f"    SKIP {dev}: still invalid after normalization ({errs2})")
                continue
            (root / "devices" / dev).write_text(final_text, encoding="utf-8")
            print(f"    WROTE devices/{dev}")


def extract_source_repo(text: str) -> tuple[str, str] | None:
    for line in text.splitlines()[:10]:
        m = re.search(r"https://github\.com/([^/\s]+/[^/\s]+)(?:/(?:tree|blob)/([^\s]+))?", line)
        if m:
            repo = m.group(1).removesuffix(".git")
            return repo, f"https://github.com/{repo}"
    return None


def update_readmes(root: pathlib.Path) -> None:
    rows = []
    for p in sorted((root / "devices").glob("*.toml")):
        codename = p.stem
        if codename in OFFICIAL_DEVICES:
            continue
        text = p.read_text(encoding="utf-8")
        cfg = tomllib.loads(text)
        dev = cfg.get("device", {})
        brand = dev.get("brand", "").strip()
        model = dev.get("model", "").strip()
        if brand and model.lower().startswith(brand.lower()):
            display = model
        elif brand and model:
            display = f"{brand} {model}"
        else:
            display = model or brand or codename
        stock_ver = cfg.get("stock", {}).get("version", "")
        src = extract_source_repo(text)
        src_md = f"[{src[0]}]({src[1]})" if src else "—"
        rows.append((display, codename, stock_ver, src_md))

    def build_table(header_cols: tuple[str, str, str, str], local_label: str) -> str:
        lines = [
            f"| {header_cols[0]} | {header_cols[1]} | {header_cols[2]} | {header_cols[3]} |",
            "| --- | --- | --- | --- |",
        ]
        for display, codename, stock_ver, src_md in rows:
            build_col = f"`{stock_ver}`" if stock_ver else local_label
            lines.append(f"| {display} | `{codename}` | {build_col} | {src_md} |")
        return "\n".join(lines)

    configs = [
        ("README.md", ("Device", "Codename", "Built on", "Source"), "Local pack (no `[stock]`)"),
        ("README.vi.md", ("Thiết bị", "Tên mã", "Dựng trên", "Nguồn"), "Tự đóng gói (chưa có `[stock]`)"),
        ("README.zh-CN.md", ("机型", "代号", "构建版本", "来源仓库"), "本地打包（无 `[stock]`）"),
    ]
    start_marker = "<!-- COMMUNITY-DEVICES-START -->"
    end_marker = "<!-- COMMUNITY-DEVICES-END -->"
    for fname, cols, local_label in configs:
        path = root / fname
        content = path.read_text(encoding="utf-8")
        if start_marker not in content or end_marker not in content:
            print(f"SKIP {fname}: missing {start_marker} / {end_marker}")
            continue
        table = build_table(cols, local_label)
        updated = re.sub(
            rf"{re.escape(start_marker)}.*?{re.escape(end_marker)}",
            f"{start_marker}\n{table}\n{end_marker}",
            content,
            flags=re.DOTALL,
        )
        path.write_text(updated, encoding="utf-8")
        print(f"Updated {fname} with {len(rows)} community-ported devices.")


def main() -> None:
    parser = argparse.ArgumentParser(description="Scan kirklin/skyward forks for devices and sync READMEs.")
    parser.add_argument("--report", action="store_true", help="Scan forks and report new devices")
    parser.add_argument("--apply", action="store_true", help="Fetch and write new devices/*.toml")
    parser.add_argument("--update-readme", action="store_true", help="Regenerate community device tables in READMEs")
    args = parser.parse_args()

    root = repo_root()
    if args.report or args.apply:
        scan_forks(root, apply=args.apply)
    if args.update_readme:
        update_readmes(root)
    if not (args.report or args.apply or args.update_readme):
        parser.print_help()


if __name__ == "__main__":
    main()
