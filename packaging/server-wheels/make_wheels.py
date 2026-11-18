#!/usr/bin/env python3
"""chloedb-server's wheels for PyPI, made from a release's archives.

`pip install chloedb-server` (or `uvx chloedb-server`, `pipx run
chloedb-server`) installs the prebuilt server. Each wheel holds the binary of
the release's archive for its platform, unchanged, as a script of the wheel
(chloedb_server-<version>.data/scripts/chloedb-server), so the installer
puts it beside the environment's other programs, as ruff's wheels do; and a
small module, chloedb_server (this folder's), that finds it:
`python -m chloedb_server` runs it, and `chloedb.serve()` starts it.

The wheels have no Python code to compile and no Python ABI: their tag is
py3-none-<platform>, the platform read from the binary itself (its
architecture; on Linux the newest glibc symbol version it needs, as
manylinux_<major>_<minor>; on macOS the version it was built for), so pip
takes one only where the binary runs. A binary that needs a shared library
beyond glibc's own and libgcc_s is refused: it would not run everywhere its
tag says.

    python3 packaging/server-wheels/make_wheels.py --version 0.3.7 --out wheels dist/chloedb-server-*.tar.gz
    python3 packaging/server-wheels/make_wheels.py --self-test

Each archive is a release's chloedb-server-<platform>.tar.gz, as
server-release.yml packages it: a folder of that name holding the server and
its LICENSE. server-release.yml makes each platform's wheel beside its
archive, installs it and runs it, and a tag uploads them to PyPI.
"""
import argparse
import base64
import hashlib
import os
import pathlib
import re
import struct
import sys
import tarfile
import tempfile
import zipfile

HERE = pathlib.Path(__file__).resolve().parent
NAME = "chloedb-server"
DIST = "chloedb_server"  # the name in the wheel's file names
PLATFORMS = ["linux-x86_64", "linux-aarch64", "macos-arm64", "macos-x86_64"]
SUMMARY = ("ChloeDB's server, prebuilt: a search database (full text, vectors, graphs, SQL) "
           "over HTTP and gRPC, with a command center in the browser")
# The libraries a manylinux wheel may take from the system.
SYSTEM_LIBS = {"libc.so.6", "libm.so.6", "libgcc_s.so.1", "libpthread.so.0", "libdl.so.2", "librt.so.1",
               "ld-linux-x86-64.so.2", "ld-linux-aarch64.so.1"}
# Each zip entry's time: fixed, so that an archive makes the same wheel.
STAMP = (1980, 1, 1, 0, 0, 0)


class Refused(Exception):
    pass


# ---------------------------------------------------------------- the binary


def elf_platform(data):
    """(arch, (glibc major, minor)) of a Linux ELF binary."""
    if data[:4] != b"\x7fELF" or data[4] != 2 or data[5] != 1:
        raise Refused("not a 64-bit little-endian ELF binary")
    machine, = struct.unpack_from("<H", data, 18)
    arch = {62: "x86_64", 183: "aarch64"}.get(machine)
    if not arch:
        raise Refused(f"an ELF binary for machine {machine}, neither x86_64 nor aarch64")
    shoff, = struct.unpack_from("<Q", data, 0x28)
    shentsize, shnum = struct.unpack_from("<HH", data, 0x3A)
    sections = []
    for i in range(shnum):
        _name, typ, _flags, _addr, off, size, link, info, _align, entsize = struct.unpack_from("<IIQQQQIIQQ", data, shoff + i * shentsize)
        sections.append((typ, off, size, link, info, entsize))

    def string(table, at):
        off = sections[table][1] + at
        return data[off:data.index(b"\0", off)].decode()

    needed, versions = [], set()
    for typ, off, size, link, info, entsize in sections:
        if typ == 6:  # SHT_DYNAMIC: DT_NEEDED entries name the libraries it loads
            for at in range(off, off + size, 16):
                tag, val = struct.unpack_from("<qQ", data, at)
                if tag == 0:
                    break
                if tag == 1:
                    needed.append(string(link, val))
        elif typ == 0x6FFFFFFE:  # SHT_GNU_verneed: the symbol versions it needs of each
            at = off
            for _ in range(info):
                _v, cnt, _file, aux, nxt = struct.unpack_from("<HHIII", data, at)
                a = at + aux
                for _ in range(cnt):
                    _hash, _flags, _other, name, anext = struct.unpack_from("<IHHII", data, a)
                    versions.add(string(link, name))
                    a += anext
                at += nxt
    if "libc.so.6" not in needed:
        raise Refused(f"a binary that does not load glibc (it loads {', '.join(needed) or 'nothing'}): static, or musl")
    extra = sorted(set(needed) - SYSTEM_LIBS)
    if extra:
        raise Refused(f"a binary that needs {', '.join(extra)}, which a manylinux system need not have")
    glibc = [tuple(int(x) for x in m.groups()) for v in versions if (m := re.fullmatch(r"GLIBC_(\d+)\.(\d+)(?:\.\d+)?", v))]
    return arch, max(glibc, default=(2, 17))


def macho_platform(data):
    """(arch, (macOS major, minor)) of a macOS Mach-O binary."""
    magic, cputype, _sub, _filetype, ncmds, _size = struct.unpack_from("<IiiIII", data, 0)
    if magic != 0xFEEDFACF:
        raise Refused("not a 64-bit Mach-O binary (a universal one is not taken)")
    arch = {0x0100000C: "arm64", 0x01000007: "x86_64"}.get(cputype & 0xFFFFFFFF)
    if not arch:
        raise Refused(f"a Mach-O binary for CPU type {cputype:#x}, neither arm64 nor x86_64")
    at = 32
    for _ in range(ncmds):
        cmd, cmdsize = struct.unpack_from("<II", data, at)
        if cmd == 0x32:  # LC_BUILD_VERSION
            _platform, minos = struct.unpack_from("<II", data, at + 8)
            return arch, (minos >> 16, (minos >> 8) & 0xFF)
        if cmd == 0x24:  # LC_VERSION_MIN_MACOSX
            version, = struct.unpack_from("<I", data, at + 8)
            return arch, (version >> 16, (version >> 8) & 0xFF)
        at += cmdsize
    raise Refused("a Mach-O binary that names no minimum macOS")


def wheel_platform(platform, data):
    """The wheel's platform tag for the release platform's binary."""
    if platform.startswith("linux-"):
        arch, (major, minor) = elf_platform(data)
        tag = f"manylinux_{major}_{minor}_{arch}"
        want = platform.removeprefix("linux-")
    elif platform.startswith("macos-"):
        arch, (major, minor) = macho_platform(data)
        # From macOS 11 a wheel names the major version only.
        tag = f"macosx_{major}_{0 if major >= 11 else minor}_{arch}"
        want = platform.removeprefix("macos-")
    else:
        raise Refused(f"no wheels for {platform}")
    if arch != want:
        raise Refused(f"the archive of {platform} holds a binary for {arch}")
    return tag


# ---------------------------------------------------------------- the wheel


def metadata(version, notices=False):
    readme = (HERE / "README.md").read_text()
    head = [
        "Metadata-Version: 2.4",
        f"Name: {NAME}",
        f"Version: {version}",
        f"Summary: {SUMMARY}",
        "License-Expression: Apache-2.0",
        "License-File: LICENSE",
        *(["License-File: THIRD-PARTY-NOTICES.md"] if notices else []),
        "Project-URL: Homepage, https://chloetech-ai.github.io/chloedb/",
        "Project-URL: Documentation, https://github.com/chloetech-ai/chloedb/blob/main/docs/chloedb/server.md",
        "Project-URL: Source, https://github.com/chloetech-ai/chloedb",
        "Project-URL: Changelog, https://github.com/chloetech-ai/chloedb/blob/main/CHANGELOG.md",
        "Keywords: search,database,full-text,vector,bm25,sql,server",
        "Classifier: Development Status :: 4 - Beta",
        "Classifier: Environment :: Console",
        "Classifier: Intended Audience :: Developers",
        "Classifier: Operating System :: MacOS",
        "Classifier: Operating System :: POSIX :: Linux",
        "Classifier: Programming Language :: Rust",
        "Classifier: Topic :: Database :: Database Engines/Servers",
        "Requires-Python: >=3.8",
        "Description-Content-Type: text/markdown; charset=UTF-8; variant=GFM",
    ]
    return ("\n".join(head) + "\n\n" + readme).encode()


def record_line(path, data):
    digest = base64.urlsafe_b64encode(hashlib.sha256(data).digest()).rstrip(b"=").decode()
    return f"{path},sha256={digest},{len(data)}"


def make_wheel(version, platform, binary, license_text, out, notices=None):
    """Writes the wheel of one platform into `out`; returns its path.
    `notices`, the archive's THIRD-PARTY-NOTICES.md (the licences of the
    crates compiled into the binary, from 0.3.9), goes beside LICENSE."""
    if not re.fullmatch(r"\d+\.\d+\.\d+(?:(?:a|b|rc)\d+)?", version):
        raise Refused(f"version {version!r} is not one PyPI takes as it is")
    tag = f"py3-none-{wheel_platform(platform, binary)}"
    data_dir = f"{DIST}-{version}.data"
    info = f"{DIST}-{version}.dist-info"
    module = HERE / "chloedb_server"
    files = [
        (f"{DIST}/__init__.py", (module / "__init__.py").read_text().replace("@VERSION@", version).encode(), 0o644),
        (f"{DIST}/__main__.py", (module / "__main__.py").read_bytes(), 0o644),
        (f"{data_dir}/scripts/{NAME}", binary, 0o755),
        (f"{info}/licenses/LICENSE", license_text, 0o644),
        *([(f"{info}/licenses/THIRD-PARTY-NOTICES.md", notices, 0o644)] if notices is not None else []),
        (f"{info}/METADATA", metadata(version, notices is not None), 0o644),
        (f"{info}/WHEEL", f"Wheel-Version: 1.0\nGenerator: chloedb make_wheels.py\nRoot-Is-Purelib: false\nTag: {tag}\n".encode(), 0o644),
    ]
    record = "\n".join([record_line(p, d) for p, d, _ in files] + [f"{info}/RECORD,,"]) + "\n"
    files.append((f"{info}/RECORD", record.encode(), 0o644))
    path = pathlib.Path(out) / f"{DIST}-{version}-{tag}.whl"
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_suffix(".tmp")
    with zipfile.ZipFile(tmp, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as z:
        for name, data, mode in files:
            entry = zipfile.ZipInfo(name, STAMP)
            entry.external_attr = (0o100000 | mode) << 16
            entry.compress_type = zipfile.ZIP_DEFLATED
            z.writestr(entry, data)
    tmp.replace(path)
    return path


def from_archive(archive):
    """(platform, binary, license, notices) of a release archive
    chloedb-server-<platform>.tar.gz; notices is None in an archive older
    than 0.3.9, which has no THIRD-PARTY-NOTICES.md."""
    m = re.fullmatch(rf"{NAME}-({'|'.join(PLATFORMS)})\.tar\.gz", pathlib.Path(archive).name)
    if not m:
        raise Refused(f"{archive} is not named {NAME}-<platform>.tar.gz, <platform> one of {', '.join(PLATFORMS)}")
    platform = m.group(1)
    folder = f"{NAME}-{platform}"
    with tarfile.open(archive) as t:
        def read(name, needed=True):
            try:
                member = t.getmember(f"{folder}/{name}")
            except KeyError:
                if not needed:
                    return None
                raise Refused(f"{archive} has no {folder}/{name}") from None
            if not member.isfile():
                raise Refused(f"{archive}: {folder}/{name} is not a file")
            return t.extractfile(member).read()
        return platform, read(NAME), read("LICENSE"), read("THIRD-PARTY-NOTICES.md", needed=False)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("archives", nargs="*", help="release archives, chloedb-server-<platform>.tar.gz")
    ap.add_argument("--version", help="the release's version (chloedb-server/Cargo.toml's)")
    ap.add_argument("--out", default="wheels", help="the folder the wheels go to (default: wheels)")
    ap.add_argument("--self-test", action="store_true", help="check this script on made-up binaries, and exit")
    args = ap.parse_args(argv)
    if args.self_test:
        return self_test()
    if not args.version or not args.archives:
        ap.error("--version and at least one archive are needed")
    try:
        for archive in args.archives:
            platform, binary, license_text, notices = from_archive(archive)
            path = make_wheel(args.version, platform, binary, license_text, args.out, notices)
            print(f"{path} ({path.stat().st_size / 1e6:.1f} MB, from {archive})")
    except Refused as e:
        print(f"make_wheels.py: {e}", file=sys.stderr)
        return 1
    return 0


# ---------------------------------------------------------------- self-test


def fake_elf(machine, needed, versions):
    """The smallest ELF file elf_platform reads: a header, a string table, a
    dynamic section and a verneed section."""
    strtab = b"\0"
    offsets = {}
    for s in ["libc.so.6", *needed, *versions]:
        if s not in offsets:
            offsets[s] = len(strtab)
            strtab += s.encode() + b"\0"
    dynamic = b"".join(struct.pack("<qQ", 1, offsets[n]) for n in needed) + struct.pack("<qQ", 0, 0)
    aux = b"".join(struct.pack("<IHHII", 0, 0, 0, offsets[v], 16 if i + 1 < len(versions) else 0) for i, v in enumerate(versions))
    verneed = struct.pack("<HHIII", 1, len(versions), offsets["libc.so.6"], 16, 0) + aux
    header_size, sh_size = 64, 64
    body = strtab + dynamic + verneed
    shoff = header_size + len(body)
    secs = [(0, 0, 0, 0, 0, 0),
            (3, header_size, len(strtab), 0, 0, 0),
            (6, header_size + len(strtab), len(dynamic), 1, 0, 16),
            (0x6FFFFFFE, header_size + len(strtab) + len(dynamic), len(verneed), 1, 1, 0)]
    header = b"\x7fELF" + bytes([2, 1, 1]) + bytes(9) + struct.pack("<HHIQQQIHHHHHH", 3, machine, 1, 0, 0, shoff, 0, header_size, 0, 0, sh_size, len(secs), 1)
    sections = b"".join(struct.pack("<IIQQQQIIQQ", 0, t, 0, 0, off, size, link, info, 8, ent) for t, off, size, link, info, ent in secs)
    return header + body + sections


def fake_macho(cputype, minos):
    cmd = struct.pack("<IIIIII", 0x32, 24, 1, minos, minos, 0)
    return struct.pack("<IIIIIII", 0xFEEDFACF, cputype, 0, 2, 1, len(cmd), 0) + struct.pack("<I", 0) + cmd


def self_test():
    import contextlib
    import io

    with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
        _self_test()
    print("make_wheels.py: self-test passed")
    return 0


def _self_test():
    x86 = fake_elf(62, ["libc.so.6", "libm.so.6", "libgcc_s.so.1"], ["GLIBC_2.2.5", "GLIBC_2.34", "GLIBC_2.17", "GLIBC_2.3.4"])
    assert wheel_platform("linux-x86_64", x86) == "manylinux_2_34_x86_64", wheel_platform("linux-x86_64", x86)
    arm = fake_elf(183, ["libc.so.6"], ["GLIBC_2.17", "GLIBC_2.35"])
    assert wheel_platform("linux-aarch64", arm) == "manylinux_2_35_aarch64"
    assert wheel_platform("macos-arm64", fake_macho(0x0100000C, 11 << 16)) == "macosx_11_0_arm64"
    assert wheel_platform("macos-x86_64", fake_macho(0x01000007, (10 << 16) | (12 << 8))) == "macosx_10_12_x86_64"
    assert wheel_platform("macos-x86_64", fake_macho(0x01000007, (13 << 16) | (3 << 8))) == "macosx_13_0_x86_64"
    for platform, data, why in [
        ("linux-aarch64", x86, "holds a binary for x86_64"),
        ("linux-x86_64", fake_elf(62, ["libc.so.6", "libssl.so.3"], ["GLIBC_2.17"]), "needs libssl.so.3"),
        ("linux-x86_64", fake_elf(62, ["libc.musl-x86_64.so.1"], []), "does not load glibc"),
        ("macos-arm64", x86, "not a 64-bit Mach-O"),
        ("macos-arm64", fake_macho(0x01000007, 11 << 16), "holds a binary for x86_64"),
        ("windows-x86_64", x86, "no wheels for"),
    ]:
        try:
            wheel_platform(platform, data)
            raise AssertionError(f"{platform}: refused nothing, wanted {why!r}")
        except Refused as e:
            assert why in str(e), (platform, str(e))

    with tempfile.TemporaryDirectory() as tmp:
        folder = pathlib.Path(tmp) / f"{NAME}-linux-x86_64"
        folder.mkdir()
        (folder / NAME).write_bytes(x86)
        (folder / "LICENSE").write_text("Apache License\n")
        archive = pathlib.Path(tmp) / f"{NAME}-linux-x86_64.tar.gz"
        with tarfile.open(archive, "w:gz") as t:
            t.add(folder, arcname=folder.name)
        # An archive from 0.3.9 has the third-party notices, which the
        # wheel carries beside LICENSE.
        (folder / "THIRD-PARTY-NOTICES.md").write_text("# Third-party notices\n")
        noticed = pathlib.Path(tmp) / "with-notices" / archive.name
        noticed.parent.mkdir()
        with tarfile.open(noticed, "w:gz") as t:
            t.add(folder, arcname=folder.name)
        assert main(["--version", "0.3.9", "--out", f"{tmp}/w4", str(noticed)]) == 0
        with zipfile.ZipFile(f"{tmp}/w4/{DIST}-0.3.9-py3-none-manylinux_2_34_x86_64.whl") as z:
            assert z.read(f"{DIST}-0.3.9.dist-info/licenses/THIRD-PARTY-NOTICES.md") == b"# Third-party notices\n"
            assert z.read(f"{DIST}-0.3.9.dist-info/licenses/LICENSE") == b"Apache License\n"
            meta = z.read(f"{DIST}-0.3.9.dist-info/METADATA").decode()
            assert "License-File: LICENSE\nLicense-File: THIRD-PARTY-NOTICES.md\n" in meta, meta[:400]
        assert main(["--version", "0.3.7", "--out", f"{tmp}/w1", str(archive)]) == 0
        assert main(["--version", "0.3.7", "--out", f"{tmp}/w2", str(archive)]) == 0
        whl = pathlib.Path(f"{tmp}/w1/{DIST}-0.3.7-py3-none-manylinux_2_34_x86_64.whl")
        assert whl.read_bytes() == pathlib.Path(f"{tmp}/w2/{whl.name}").read_bytes(), "the same archive makes the same wheel"
        with zipfile.ZipFile(whl) as z:
            names = z.namelist()
            assert names[-1] == f"{DIST}-0.3.7.dist-info/RECORD", names
            assert z.read(f"{DIST}-0.3.7.data/scripts/{NAME}") == x86
            assert z.getinfo(f"{DIST}-0.3.7.data/scripts/{NAME}").external_attr >> 16 == 0o100755
            assert b'__version__ = "0.3.7"' in z.read(f"{DIST}/__init__.py")
            meta = z.read(f"{DIST}-0.3.7.dist-info/METADATA").decode()
            assert "Name: chloedb-server\nVersion: 0.3.7\n" in meta and "\n\n# chloedb-server\n" in meta, meta[:300]
            assert "THIRD-PARTY-NOTICES" not in meta and f"{DIST}-0.3.7.dist-info/licenses/LICENSE" in names
            assert "Tag: py3-none-manylinux_2_34_x86_64\n" in z.read(f"{DIST}-0.3.7.dist-info/WHEEL").decode()
            record = z.read(f"{DIST}-0.3.7.dist-info/RECORD").decode().splitlines()
            assert sorted(line.split(",")[0] for line in record) == sorted(names), (record, names)
            for line in record[:-1]:
                path, digest, size = line.split(",")
                assert line == record_line(path, z.read(path)), line
        for bad, why in [(["--version", "0.3", str(archive)], "--version"),
                         (["--version", "v0.3.7", str(archive)], "version"),
                         (["--version", "0.3.7", str(folder / NAME)], "not named")]:
            assert main(bad + ["--out", f"{tmp}/w3"]) == 1, (bad, why)


if __name__ == "__main__":
    sys.exit(main())
