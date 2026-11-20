#!/usr/bin/env python3
"""The bill of materials and third-party notices of one prebuilt program.

Each archive of a release holds one program, built from one crate of this
repository and the crates its Cargo.lock pins. For each, server-release.yml
writes, before it packs the archive:

- a CycloneDX 1.6 SBOM, <program>-<platform>.cdx.json, beside the archive:
  every crate the build compiles for that platform (name, version, purl,
  licence, the SHA-256 crates.io publishes for it, its repository) and what
  depends on what. A crate that is only compiled to build (a build script's
  dependency, a procedural macro and what it uses) has scope "excluded": it
  is not in the binary. The program itself (metadata.component) carries the
  binary's SHA-256. The release attests the SBOM for its archive
  (`gh attestation verify <archive> --predicate-type https://cyclonedx.org/bom`);
- THIRD-PARTY-NOTICES.md, into the archive: each crate with its licence and
  the licence texts its package ships (each different text once, with the
  crates that use it), and, for the few packages that ship none, the
  standard text of their licence.

It reads `cargo tree` (the crates the release's build compiles for the
platform, with its features: `cargo metadata`'s graph has the
dev-dependencies' features too, and with them crates the build never
compiles), `cargo metadata` (each crate's licence, source and folder) and
the crate's Cargo.lock (the checksums), so it needs only Python 3.8 and the
cargo that builds the program, and it writes the same files for the same
commit and platform.

    python3 packaging/sbom/sbom.py --crate chloedb-server --binary chloedb-server/target/release/chloedb-server \\
        --platform linux-x86_64 --cdx dist/chloedb-server-linux-x86_64.cdx.json --notices dist/chloedb-server-linux-x86_64/THIRD-PARTY-NOTICES.md
    python3 packaging/sbom/sbom.py --self-test
"""
import argparse
import hashlib
import json
import os
import pathlib
import re
import subprocess
import sys
import tempfile
import uuid

ROOT = pathlib.Path(__file__).resolve().parents[2]
REPO = "chloetech-ai/chloedb"
TOOL_VERSION = "1"
# Files at a package's top that hold its licence or its notices.
LICENSE_FILE = re.compile(r"^(LICEN[CS]E|COPYING|COPYRIGHT|NOTICE|UNLICENSE)([-._].*)?$", re.I)
NAMESPACE = uuid.UUID("2b6f0cc9-0d1c-4f49-9a52-62f2f3a8c1d7")

MIT = """Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
"""


BSD_2 = """Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice, this
   list of conditions and the following disclaimer.

2. Redistributions in binary form must reproduce the above copyright notice,
   this list of conditions and the following disclaimer in the documentation
   and/or other materials provided with the distribution.
"""

BSD_3_CLAUSE = """
3. Neither the name of the copyright holder nor the names of its
   contributors may be used to endorse or promote products derived from
   this software without specific prior written permission.
"""

BSD_DISCLAIMER = """
THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
"""

ISC = """Permission to use, copy, modify, and/or distribute this software for any
purpose with or without fee is hereby granted, provided that the above
copyright notice and this permission notice appear in all copies.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
"""


class Refused(Exception):
    pass


def spdx(expr):
    """A crate's `license` as an SPDX expression ("MIT/Apache-2.0", the old
    way of writing it, is "MIT OR Apache-2.0")."""
    if not expr:
        return None
    return re.sub(r"\s*/\s*", " OR ", expr.strip())


def ids(expr):
    """The licence identifiers an SPDX expression names."""
    return sorted({t for t in re.split(r"[\s()]+", expr or "") if t and t not in ("AND", "OR", "WITH")})


def checksums(lock_text):
    """{(name, version): sha256} from a Cargo.lock's [[package]] entries."""
    out = {}
    for block in lock_text.split("[[package]]")[1:]:
        fields = dict(re.findall(r'^(name|version|checksum) = "([^"]*)"', block, re.M))
        if "checksum" in fields:
            out[(fields.get("name"), fields.get("version"))] = fields["checksum"]
    return out


def metadata(crate_dir, target):
    cmd = ["cargo", "metadata", "--format-version", "1", "--locked", "--color", "never"]
    if target:
        cmd += ["--filter-platform", target]
    r = subprocess.run(cmd, cwd=crate_dir, capture_output=True, text=True)
    if r.returncode != 0:
        raise Refused(f"cargo metadata in {crate_dir} failed:\n{r.stderr}")
    return json.loads(r.stdout)


def host_target():
    r = subprocess.run(["rustc", "-vV"], capture_output=True, text=True)
    m = re.search(r"^host: (\S+)", r.stdout, re.M)
    if not m:
        raise Refused("rustc -vV names no host")
    return m.group(1)


def tree(crate_dir, target, edges):
    """`cargo tree` of the crate for `target`, following `edges` (normal,
    build): the graph cargo's resolver builds for the release, features
    and all, which `cargo metadata` does not give (it unifies the features
    the dev-dependencies ask for too)."""
    # --color never: the release workflow sets CARGO_TERM_COLOR=always, which
    # colours cargo tree's "(*)" marks.
    cmd = ["cargo", "tree", "--locked", "--color", "never", "-e", edges, "--target", target, "--prefix", "depth", "-f", "{p}"]
    r = subprocess.run(cmd, cwd=crate_dir, capture_output=True, text=True)
    if r.returncode != 0:
        raise Refused(f"cargo tree in {crate_dir} failed:\n{r.stderr}")
    return r.stdout


TREE_LINE = re.compile(r"^(\d+)(\S+) v(\S+)((?: \([^)]*\))*)$")


def parse_tree(text):
    """(root, {(name, version): {children}}, {proc-macro crates}) of
    `cargo tree --prefix depth -f {p}`."""
    children, procs, stack, root = {}, set(), [], None
    for line in text.splitlines():
        if not line.strip():
            continue
        m = TREE_LINE.match(line)
        if not m:
            raise Refused(f"cargo tree printed a line this does not read: {line!r}")
        depth, key, notes = int(m.group(1)), (m.group(2), m.group(3)), m.group(4)
        if "(proc-macro)" in notes:
            procs.add(key)
        children.setdefault(key, set())
        del stack[depth:]
        if depth == 0:
            root = key
        elif len(stack) == depth:
            children[stack[-1]].add(key)
        else:
            raise Refused(f"cargo tree's line {line!r} has no parent")
        stack.append(key)
    if root is None:
        raise Refused("cargo tree printed no crate")
    return root, children, procs


def walk(meta):
    """(root id, {id: package}, {id: [dep ids]}, set of ids in the binary):
    the crates the build compiles (meta["trees"]["full"], cargo tree's
    normal and build dependencies), dev-dependencies left out. One is in
    the binary when the root reaches it by normal dependencies alone
    (meta["trees"]["normal"]), through no procedural macro."""
    packages = {p["id"]: p for p in meta["packages"]}
    by_key = {}
    for p in meta["packages"]:
        by_key.setdefault((p["name"], p["version"]), []).append(p["id"])

    def pid(key):
        found = by_key.get(key, [])
        if len(found) != 1:
            raise Refused(f"{key[0]} {key[1]} is {'in cargo metadata more than once' if found else 'not in cargo metadata'}")
        return found[0]

    root_key, full, _ = parse_tree(meta["trees"]["full"])
    normal_root, normal, procs = parse_tree(meta["trees"]["normal"])
    if normal_root != root_key:
        raise Refused(f"the two trees start at {root_key} and {normal_root}")
    built = {pid(k): sorted({pid(c) for c in kids}) for k, kids in full.items()}
    linked, seen = set(), [root_key]
    while seen:
        k = seen.pop()
        if k in linked:
            continue
        linked.add(k)
        seen.extend(c for c in normal.get(k, ()) if c not in procs)
    return pid(root_key), packages, built, {pid(k) for k in linked}


def purl(p):
    if (p.get("source") or "").startswith("registry+https://github.com/rust-lang/crates.io-index"):
        return f"pkg:cargo/{p['name']}@{p['version']}"
    return None


def ref(p):
    return purl(p) or f"{p['name']}@{p['version']} (this repository)"


def bom(meta, lock, name, version, platform, target, binary_sha, stamp):
    root, packages, built, linked = walk(meta)
    sums = checksums(lock)
    components = []
    for nid in sorted(built, key=lambda i: (packages[i]["name"], packages[i]["version"])):
        if nid == root:
            continue
        p = packages[nid]
        c = {"type": "library", "bom-ref": ref(p), "name": p["name"], "version": p["version"],
             "scope": "required" if nid in linked else "excluded"}
        if p.get("description"):
            c["description"] = " ".join(p["description"].split())
        if purl(p):
            c["purl"] = purl(p)
        lic = spdx(p.get("license"))
        if lic:
            c["licenses"] = [{"expression": lic}]
        elif p.get("license_file"):
            c["licenses"] = [{"license": {"name": f"see {p['license_file']} in the crate"}}]
        if (p["name"], p["version"]) in sums:
            c["hashes"] = [{"alg": "SHA-256", "content": sums[(p["name"], p["version"])]}]
        refs = []
        if p.get("repository"):
            refs.append({"type": "vcs", "url": p["repository"]})
        if purl(p):
            refs.append({"type": "distribution", "url": f"https://crates.io/crates/{p['name']}/{p['version']}"})
        if refs:
            c["externalReferences"] = refs
        if not p.get("source"):
            c["properties"] = [{"name": "cdx:cargo:source", "value": f"https://github.com/{REPO} (a crate of this repository)"}]
        components.append(c)
    main = packages[root]
    component = {
        "type": "application", "bom-ref": f"{name}@{version}", "name": name, "version": version,
        "purl": f"pkg:github/{REPO}@chloedb-server-v{version}",
        "licenses": [{"expression": spdx(main.get("license")) or "Apache-2.0"}],
        "externalReferences": [{"type": "vcs", "url": f"https://github.com/{REPO}"}],
        "properties": [{"name": "cdx:rust:target", "value": target}, {"name": "chloedb:platform", "value": platform},
                       {"name": "chloedb:archive", "value": f"{name}-{platform}.tar.gz"}],
    }
    if binary_sha:
        component["hashes"] = [{"alg": "SHA-256", "content": binary_sha}]
    deps = [{"ref": component["bom-ref"] if nid == root else ref(packages[nid]),
             "dependsOn": [ref(packages[d]) for d in built[nid]]} for nid in sorted(built)]
    serial = uuid.uuid5(NAMESPACE, f"{name}/{version}/{platform}/{binary_sha or ''}")
    return {
        "$schema": "http://cyclonedx.org/schema/bom-1.6.schema.json",
        "bomFormat": "CycloneDX", "specVersion": "1.6", "serialNumber": f"urn:uuid:{serial}", "version": 1,
        "metadata": {
            "timestamp": stamp,
            "tools": {"components": [{"type": "application", "name": "packaging/sbom/sbom.py", "version": TOOL_VERSION,
                                      "externalReferences": [{"type": "vcs", "url": f"https://github.com/{REPO}"}]}]},
            "component": component,
        },
        "components": components,
        "dependencies": sorted(deps, key=lambda d: d["ref"]),
    }


def licence_files(p):
    d = pathlib.Path(p["manifest_path"]).parent
    names = set()
    if d.is_dir():
        names = {f.name for f in d.iterdir() if f.is_file() and LICENSE_FILE.match(f.name)}
    if p.get("license_file"):
        names.add(p["license_file"])
    out = []
    for n in sorted(names):
        f = d / n
        if f.is_file():
            out.append((pathlib.PurePath(n).name, f.read_text(encoding="utf-8", errors="replace")))
    return out


def standard_text(spdx_id, p):
    """The standard text of a licence, for a package that ships none, with
    its authors as the copyright holders."""
    holders = ", ".join(re.sub(r"\s*<[^>]*>", "", a) for a in p.get("authors") or []) or f"the {p['name']} authors"
    if spdx_id == "MIT":
        return f"MIT License\n\nCopyright (c) {holders}\n\n{MIT}"
    if spdx_id in ("BSD-2-Clause", "BSD-3-Clause"):
        third = BSD_3_CLAUSE if spdx_id == "BSD-3-Clause" else ""
        return f"{spdx_id}\n\nCopyright (c) {holders}\n\n{BSD_2}{third}{BSD_DISCLAIMER}"
    if spdx_id == "ISC":
        return f"ISC License\n\nCopyright (c) {holders}\n\n{ISC}"
    if spdx_id == "Apache-2.0":
        return (ROOT / "LICENSE").read_text()
    return None


def notices(meta, name, version, platform, sbom=True):
    root, packages, built, linked = walk(meta)
    crates = sorted((packages[i] for i in built if i != root and packages[i].get("source")), key=lambda p: (p["name"], p["version"]))
    ours = sorted(packages[i]["name"] for i in built if i != root and not packages[i].get("source"))
    texts = {}  # text -> (file name, [crate@version])
    missing = []
    for p in crates:
        files = licence_files(p)
        if not files:
            if not ids(spdx(p.get("license"))):
                raise Refused(f"{p['name']} {p['version']} names no licence and ships no licence file")
            for lid in ids(spdx(p.get("license"))):
                t = standard_text(lid, p)
                if t:
                    files.append((f"{lid} (standard text)", t))
                else:
                    # MPL-2.0's own notice is a reference to its text, as here.
                    files.append((f"{lid} (by reference)", f"{p['name']} {p['version']} is under {lid}: its text is at https://spdx.org/licenses/{lid}.html,\n"
                                                           f"and its source at https://crates.io/crates/{p['name']}/{p['version']}.\n"))
            missing.append(p)
        for fname, t in files:
            key = t.strip()
            texts.setdefault(key, (fname, []))[1].append(f"{p['name']} {p['version']}")
    lines = [
        f"# Third-party notices: {name} {version} ({platform})",
        "",
        f"{name} is Apache-2.0 (its LICENSE file), built from https://github.com/{REPO}"
        + (f" with this repository's crates {', '.join(ours)}" if ours else "") + ".",
        f"It is compiled with the {len(crates)} crates below, from crates.io, whose licences follow. "
        "A crate marked (build) is only compiled to build it (a build script's dependency or a procedural macro) and is not in the binary.",
    ]
    if sbom:
        lines.append(f"The release's SBOM, {name}-{platform}.cdx.json, lists the same crates with their checksums.")
    lines += [
        "",
        "| crate | version | licence |",
        "|---|---|---|",
    ]
    for p in crates:
        mark = "" if p["id"] in linked else " (build)"
        lines.append(f"| {p['name']}{mark} | {p['version']} | {spdx(p.get('license')) or 'see its licence file'} |")
    if missing:
        lines += ["", "These packages ship no licence file; the standard text of their licence (or where to read it) is below: "
                  + ", ".join(f"{p['name']} {p['version']}" for p in missing) + "."]
    lines += ["", "## Licence texts", ""]
    for i, (t, (fname, users)) in enumerate(sorted(texts.items(), key=lambda kv: kv[1][1][0]), 1):
        lines += [f"### {i}. {fname}: {', '.join(users)}", "", "```text", t.replace("```", "'''"), "```", ""]
    return "\n".join(lines).rstrip() + "\n"


def stamp_of(crate_dir):
    if os.environ.get("SOURCE_DATE_EPOCH", "").isdigit():
        import datetime
        return datetime.datetime.fromtimestamp(int(os.environ["SOURCE_DATE_EPOCH"]), datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    r = subprocess.run(["git", "log", "-1", "--format=%cd", "--date=format-local:%Y-%m-%dT%H:%M:%SZ"], cwd=crate_dir,
                       capture_output=True, text=True, env={**os.environ, "TZ": "UTC"})
    if r.returncode == 0 and r.stdout.strip():
        return r.stdout.strip()
    import datetime
    return datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def write(crate, name, platform, binary, cdx, notices_path, target=None):
    crate_dir = ROOT / crate if not pathlib.Path(crate).is_absolute() else pathlib.Path(crate)
    target = target or host_target()
    meta = metadata(crate_dir, target)
    meta["trees"] = {"full": tree(crate_dir, target, "normal,build"), "normal": tree(crate_dir, target, "normal")}
    lock = (crate_dir / "Cargo.lock").read_text()
    root = next(p for p in meta["packages"] if p["id"] == meta["resolve"]["root"])
    version = root["version"]
    sha = hashlib.sha256(pathlib.Path(binary).read_bytes()).hexdigest() if binary else None
    doc = bom(meta, lock, name, version, platform, target, sha, stamp_of(crate_dir))
    if cdx:
        pathlib.Path(cdx).parent.mkdir(parents=True, exist_ok=True)
        pathlib.Path(cdx).write_text(json.dumps(doc, indent=1) + "\n")
    if notices_path:
        pathlib.Path(notices_path).parent.mkdir(parents=True, exist_ok=True)
        pathlib.Path(notices_path).write_text(notices(meta, name, version, platform, sbom=bool(cdx)))
    return doc


# ---------------------------------------------------------------- self-test


def self_test():
    """Checks the graph, the scopes, the checksums and the notices on a
    made-up `cargo metadata` and Cargo.lock (no cargo, no network)."""
    with tempfile.TemporaryDirectory() as d:
        d = pathlib.Path(d)

        def pkg(name, version, licence, files=(), proc=False, source="registry+https://github.com/rust-lang/crates.io-index", authors=()):
            folder = d / f"{name}-{version}"
            folder.mkdir()
            for f, text in files:
                (folder / f).write_text(text)
            return {"id": f"{name}@{version}", "name": name, "version": version, "license": licence, "license_file": None,
                    "source": source, "repository": f"https://github.com/x/{name}", "description": f"the {name}  crate",
                    "manifest_path": str(folder / "Cargo.toml"), "authors": list(authors),
                    "targets": [{"kind": ["proc-macro" if proc else "lib"]}]}

        apache = "Apache License\nVersion 2.0\n"
        packages = [
            pkg("app", "1.0.0", "Apache-2.0", source=None),
            pkg("ours", "1.0.0", "Apache-2.0", source=None),
            pkg("serde", "1.0.1", "MIT OR Apache-2.0", [("LICENSE-MIT", "MIT serde\n"), ("LICENSE-APACHE", apache)]),
            pkg("derive", "1.0.0", "MIT/Apache-2.0", [("LICENSE-MIT", "MIT derive\n"), ("LICENSE-APACHE", apache)], proc=True),
            pkg("syn", "2.0.0", "MIT OR Apache-2.0", [("LICENSE-APACHE", apache)]),
            pkg("cc", "1.2.0", "MIT", [("LICENSE", "MIT cc\n")]),
            pkg("bare", "0.1.0", "MIT", authors=["Ann Author <ann@example.com>"]),
            pkg("testonly", "9.0.0", "MIT", [("LICENSE", "MIT testonly\n")]),
        ]
        # As `cargo tree --prefix depth -f {p}` prints them: the dev-dependency
        # (testonly) is in neither, the build-dependency (cc) in the full one.
        full = ("0app v1.0.0 (/src/app)\n1ours v1.0.0 (/src/ours)\n2serde v1.0.1\n2bare v0.1.0\n"
                "1derive v1.0.0 (proc-macro)\n2syn v2.0.0\n1cc v1.2.0\n2serde v1.0.1 (*)\n")
        normal = "0app v1.0.0 (/src/app)\n1ours v1.0.0 (/src/ours)\n2serde v1.0.1\n2bare v0.1.0\n1derive v1.0.0 (proc-macro)\n2syn v2.0.0\n"
        meta = {"packages": packages, "resolve": {"root": "app@1.0.0"}, "trees": {"full": full, "normal": normal}}
        lock = '[[package]]\nname = "serde"\nversion = "1.0.1"\nsource = "registry+..."\nchecksum = "abc123"\n\n[[package]]\nname = "app"\nversion = "1.0.0"\n'
        doc = bom(meta, lock, "app", "1.0.0", "linux-x86_64", "x86_64-unknown-linux-gnu", "f00d", "2026-11-18T00:00:00Z")
        by = {c["name"]: c for c in doc["components"]}
        assert sorted(by) == ["bare", "cc", "derive", "ours", "serde", "syn"], sorted(by)  # no dev-dependency, no root
        assert {n: c["scope"] for n, c in by.items()} == {"bare": "required", "cc": "excluded", "derive": "excluded", "ours": "required", "serde": "required", "syn": "excluded"}
        assert by["serde"]["purl"] == "pkg:cargo/serde@1.0.1" and by["serde"]["hashes"] == [{"alg": "SHA-256", "content": "abc123"}]
        assert by["derive"]["licenses"] == [{"expression": "MIT OR Apache-2.0"}], by["derive"]
        assert "purl" not in by["ours"] and by["ours"]["bom-ref"] == "ours@1.0.0 (this repository)"
        assert by["serde"]["description"] == "the serde crate"
        assert doc["metadata"]["component"]["hashes"] == [{"alg": "SHA-256", "content": "f00d"}]
        deps = {x["ref"]: x["dependsOn"] for x in doc["dependencies"]}
        assert deps["app@1.0.0"] == ["pkg:cargo/cc@1.2.0", "pkg:cargo/derive@1.0.0", "ours@1.0.0 (this repository)"], deps
        refs = {c["bom-ref"] for c in doc["components"]} | {"app@1.0.0"}
        assert all(r in refs for x in doc["dependencies"] for r in [x["ref"], *x["dependsOn"]]), "a dependency names no component"
        again = bom(meta, lock, "app", "1.0.0", "linux-x86_64", "x86_64-unknown-linux-gnu", "f00d", "2026-11-18T00:00:00Z")
        assert json.dumps(doc) == json.dumps(again), "the same input makes the same SBOM"

        text = notices(meta, "app", "1.0.0", "linux-x86_64")
        assert "| serde | 1.0.1 | MIT OR Apache-2.0 |" in text and "| derive (build) | 1.0.0 |" in text, text
        assert "testonly" not in text and "this repository's crates ours" in text, text
        assert text.count("Apache License\nVersion 2.0") == 1, "one Apache text, once"
        assert "derive 1.0.0, serde 1.0.1, syn 2.0.0" in text, text
        assert "Copyright (c) Ann Author\n" in text and "ship no licence file" in text, text
        assert spdx("Apache-2.0/MIT") == "Apache-2.0 OR MIT" and ids("(MIT OR Apache-2.0) AND Unicode-3.0") == ["Apache-2.0", "MIT", "Unicode-3.0"]
        packages.append(pkg("odd", "1.0.0", "MPL-2.0"))
        meta["trees"] = {k: v + "1odd v1.0.0\n" for k, v in meta["trees"].items()}
        text = notices(meta, "app", "1.0.0", "linux-x86_64")
        assert "odd 1.0.0 is under MPL-2.0: its text is at https://spdx.org/licenses/MPL-2.0.html" in text, text
        assert "https://crates.io/crates/odd/1.0.0" in text, text
        packages[-1]["license"] = None
        try:
            notices(meta, "app", "1.0.0", "linux-x86_64")
            raise AssertionError("a crate that names no licence and ships none is refused")
        except Refused as e:
            assert "odd 1.0.0" in str(e), e
    print("sbom.py self-test: ok")
    return 0


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--crate", help="the crate's folder, from the repository's root (chloedb-server)")
    ap.add_argument("--name", help="the program's name (default: the crate's folder name)")
    ap.add_argument("--platform", help="the release's platform name (linux-x86_64, macos-arm64, ...)")
    ap.add_argument("--target", help="the Rust target (default: rustc's host)")
    ap.add_argument("--binary", help="the built binary, whose SHA-256 the SBOM carries")
    ap.add_argument("--cdx", help="where the CycloneDX SBOM goes")
    ap.add_argument("--notices", help="where THIRD-PARTY-NOTICES.md goes")
    ap.add_argument("--self-test", action="store_true", help="check this script on made-up crates, and exit")
    args = ap.parse_args(argv)
    if args.self_test:
        return self_test()
    if not args.crate or not args.platform or not (args.cdx or args.notices):
        ap.error("--crate, --platform and --cdx or --notices are needed")
    try:
        doc = write(args.crate, args.name or pathlib.PurePath(args.crate).name, args.platform, args.binary, args.cdx, args.notices, args.target)
    except Refused as e:
        print(f"sbom.py: {e}", file=sys.stderr)
        return 1
    n = len(doc["components"])
    inside = sum(1 for c in doc["components"] if c["scope"] == "required")
    print(f"{doc['metadata']['component']['name']} {doc['metadata']['component']['version']} ({args.platform}): {n} crates, {inside} in the binary"
          + (f"; {args.cdx}" if args.cdx else "") + (f"; {args.notices}" if args.notices else ""))
    return 0


if __name__ == "__main__":
    sys.exit(main())
