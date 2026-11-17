#!/usr/bin/env python3
"""Which of CI's jobs a change needs (ci.yml's job `changes`).

Reads the changed paths, one per line, on stdin (or none, with --all, for a
run that tests everything) and prints one line `run=<json>` for
$GITHUB_OUTPUT. With --base <commit>, a change to ci.yml is read job by
job against that commit's ci.yml (below). The JSON names each group of jobs
and whether it runs:

  any        some job besides the documentation site's check runs
  full       the whole matrix: a change to ChloeDB, to this file, to
             ci.yml outside its jobs or to any path this file does not know,
             since everything depends on them
  kernels, datafusion, faults, relevance, cross_encoder, miri, macos,
  audio_chloedb
             the jobs that test ChloeDB alone (relevance is the BEIR gate,
             cross_encoder its cross-encoder's, macos ChloeDB's macOS tests
             of a push to main, audio_chloedb the ChloeDB step of job
             audio): with full, or when ci.yml changes that job
  crates     the crates `test` tests (a list, for its matrix), and test
             whether there is any
  runner     job `runner` places the longest jobs (ChloeDB's or
             chloedb-server's tests, the server's binary): ChloeDB or
             chloedb-server is among the crates
  vision     the parts of job `vision` that run (a list): chloedb with
             full, chloedb-server with a change to the server too
  audio      job `audio`, whose ChloeDB step runs only with full and whose
             step for the server with a change to the server too
  server     a chloedb-server binary is needed (job `server-bin`)
  smoke      the smoke test runs, and which of its parts: smoke_crawl,
             smoke_sheets, smoke_ctx, smoke_apps
  python, python_gate, javascript, java, docs
             those jobs (python is the package's tests, with clients
             against a server; python_gate its recommender gate and
             clippy, which need no server)

A change only under one of the apps or packages runs that one's jobs, plus
the jobs that use it: chloectx, chloedb-crawl and chloedb-sheets are run by
the smoke test and by the guide's pages (job `docs`), and the Python package
by the guide's pages too.

A change under chloedb-server (and to the pages of docs/chloedb it compiles
in) runs what uses the server's code or starts a server of this build: the
server's tests, the parts of vision and audio built with the server, the
smoke test (all of its parts: each starts this build's servers), the Python
and JavaScript packages' tests (their clients run against one) and the
guide's pages. Not ChloeDB's own jobs, which do not build the server; not
the tests of chloectx, chloedb-crawl, chloedb-sheets and chloedb-workbench,
which use no server code and skip their server tests without a binary (the
smoke test runs those against this build); not the Python package's
recommender gate and clippy, nor the Java package, which use no server.

A change to this file runs everything. A change to ci.yml runs the jobs
whose definitions it changes (read from the file parsed, so YAML anchors
count in each job that uses them, and a change to comments alone runs
nothing) and the jobs that use theirs (a change to server-bin runs every
job that starts a server); a change outside `jobs` (the triggers, `env`,
`concurrency`), to `changes` or `CI result`, or a job added or removed, runs
everything, as does a change to ci.yml without --base or PyYAML. A change
to the on-demand runner (.github/ci-runner) runs the jobs that run on it.
The
other workflows and their scripts, the install scripts and packaging, the
review rules (CODEOWNERS, the template) and the scripts that no CI job runs
reach no job here: each is tested by its own workflow, or by none (the
documentation site's check, which reads scripts/check-old-names.py and
check-documented-python.py, runs on every change anyway). The
documentation site, Markdown files and benchmark results run nothing, as
before (see the top of ci.yml).

The demos (chloedb-netops, chloedb-hn, chloedb-sound, chloedb-arxiv, chloedb-gh, chloedb-swarm,
examples/feature-store, examples/parley) are on a release train of their own: CI and its required
check `CI result` do not test them, and a change only under them runs
nothing here. The workflow Demos (demos.yml) tests them, on a pull request
that touches one and every night, and a failure there does not hold a
ChloeDB release (docs/compatibility.md, "Demos").

The TLA+ specifications (spec/) run nothing here either: the workflow
Specs (specs.yml) runs TLC on their models.

`python3 .github/scripts/ci_changes.py --self-test` checks the rules, that
every job of ci.yml has its entry in CI_JOBS, and that every file a crate
compiles in and every script ci.yml runs reaches the jobs that use it.
"""
import json
import os
import re
import subprocess
import sys

ROOT = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", ".."))

CRATES = ["chloedb", "chloectx", "chloedb-server", "chloedb-crawl", "chloedb-sheets", "chloedb-workbench"]
SMOKE = ["smoke_crawl", "smoke_sheets", "smoke_ctx", "smoke_apps"]
JOBS = ["python", "python_gate", "javascript", "java", "docs"]
VISION = ["chloedb", "chloedb-server"]
ENGINE = ["kernels", "datafusion", "faults", "relevance", "cross_encoder", "miri", "macos", "audio_chloedb"]

# Every job that starts a chloedb-server of this build (server-bin's).
STARTS_A_SERVER = {*SMOKE, "python", "javascript", "docs"}

# Directories whose changes only reach the jobs listed; anything else not
# listed below runs it all. Each maps to (crates `test` tests, other groups
# of jobs).
AREAS = {
    "chloedb-server/": ({"chloedb-server"}, {"vision:chloedb-server", "audio"} | STARTS_A_SERVER),
    "chloectx/": ({"chloectx"}, {"smoke_ctx", "docs"}),
    "chloedb-crawl/": ({"chloedb-crawl"}, {"smoke_crawl", "docs"}),
    "chloedb-sheets/": ({"chloedb-sheets"}, {"smoke_sheets", "docs"}),
    "chloedb-workbench/": ({"chloedb-workbench"}, set()),
    "chloedb-apps/": (set(), {"smoke_apps"}),
    "chloedb-py/": (set(), {"python", "python_gate", "docs"}),
    "chloedb-js/": (set(), {"javascript"}),
    "chloedb-java/": (set(), {"java"}),
    # The on-demand runner: job `runner` reads ci_runner.py, and the jobs
    # it places (the server's binary, ChloeDB's and the server's tests)
    # make their build folders with target-dir.sh.
    ".github/ci-runner/": ({"chloedb", "chloedb-server"}, {"server"}),
    # The plugin's marketplace, which chloectx's tests check (src/plugin.rs).
    ".claude-plugin/": ({"chloectx"}, set()),
}

# Files outside a crate's folder, or Markdown in it, that the crate
# compiles in (include_str!): a change to one reaches that crate's tests,
# although it is documentation. The self-test finds every such file.
COMPILED_IN = {
    "docs/chloedb/query-language.md": ({"chloedb-server"}, set()),
    "docs/chloedb/build-an-app.md": ({"chloedb-server"}, set()),
    "docs/chloedb/runbooks.md": ({"chloedb-server"}, set()),
    "chloedb-workbench/GUIDE.md": ({"chloedb-workbench"}, set()),
}

# CI's own rules: everything.
EVERYTHING = {".github/scripts/ci_changes.py"}
CI_YML = ".github/workflows/ci.yml"

# What a change to each of ci.yml's jobs runs: (crates `test` tests,
# groups), the job itself and the jobs that use what it makes. None:
# everything (the job that decides, and the one that sums up).
CI_JOBS = {
    "changes": None,
    "ci-result": None,
    "runner": ({"chloedb", "chloedb-server"}, {"server"}),
    "server-bin": (set(), STARTS_A_SERVER),
    "test": (set(CRATES), set()),
    "macos": ({"chloedb"}, {"macos"}),
    "kernels": (set(), {"kernels"}),
    "vision": (set(), {f"vision:{v}" for v in VISION}),
    "audio": (set(), {"audio", "audio_chloedb"}),
    "datafusion": (set(), {"datafusion"}),
    "faults": (set(), {"faults"}),
    "relevance-gate": (set(), {"relevance"}),
    "relevance-gate-cross-encoder": (set(), {"cross_encoder"}),
    "miri": (set(), {"miri"}),
    "server-smoke": (set(), set(SMOKE)),
    "python": (set(), {"python"}),
    "python-gate": (set(), {"python_gate"}),
    "javascript": (set(), {"javascript"}),
    "java": (set(), {"java"}),
    # It runs on every change.
    "docs-site": (set(), set()),
    "docs-bins": (set(), {"docs"}),
    "docs": (set(), {"docs"}),
    # A push to main's: they run after server-bin, which builds then.
    "main-builds": (set(), {"server"}),
    "main-builds-publish": (set(), {"server"}),
}

# Paths no job here builds, runs or reads. Each is tested by a workflow of
# its own on a pull request that changes it, or runs only on main, on a
# tag or by hand, or nowhere. A path under .github or scripts that is not
# listed (a new script, say) runs everything until it is.
NOTHING = re.compile(
    r"^("
    # The demo train (demos.yml).
    r"chloedb-netops/|chloedb-hn/|chloedb-sound/|chloedb-arxiv/|chloedb-gh/|chloedb-swarm/|examples/feature-store/|examples/parley/"
    # The other workflows; CI calls none of them.
    r"|\.github/workflows/(?!ci\.yml$)[^/]+\.ya?ml$"
    # Their scripts: door_check.py (doors.yml), nightly_report.py
    # (nightly-report.yml); and what GitHub and doors.yml read.
    r"|\.github/scripts/(door_check|nightly_report)\.py$"
    r"|\.github/(CODEOWNERS|pull_request_template\.md|dependabot\.yml)$"
    # The documentation site's check (every change) runs these two; the
    # others run in docs-urls.yml, perf.yml, by hand or on a laptop.
    r"|scripts/(check-old-names|check-documented-python|check-doc-urls|perf-gate|chloedb-rename|chloectx-rename)\.py$"
    r"|scripts/(ci-local|fetch-build|chloedb-port|chloectx-port)\.sh$"
    # The installers and the packaging: server-release.yml builds and runs
    # them on a pull request that changes them; reserve-names.yml and
    # supply-chain.yml (deny.toml) the rest.
    r"|install\.sh$|packaging/|deny\.toml$"
    # Research and ChloeDB's notes, reports and benchmark scripts, which
    # no test reads (perf.yml runs chloedb/scripts/prepare_beir.py).
    r"|research/|chloedb/(notes|report|scripts)/"
    # The TLA+ specifications: specs.yml runs TLC on them.
    r"|spec/"
    r")"
)

# The guide's pages run as scripts: a change to them or to the site's
# generator runs the docs job even though it is documentation.
GUIDE = re.compile(r"^docs/(guide/|quickstart\.md$|site/scripts/build_docs\.py$)")
# Benchmark results nothing builds or reads, except the Python scripts there
# (a job runs chloedb/results/sequence/gate.py) and the workloads
# tests/workload_gen.rs checks.
RESULTS = re.compile(r"^(chloedb|chloedb-server|chloectx)/results/")
RESULTS_CODE = re.compile(r"^(chloedb|chloedb-server|chloectx)/results/.*\.py$|^chloedb/results/workloads/")


def ci_jobs_changed(old_text, new_text):
    """The names of ci.yml's jobs whose definitions differ between two
    versions of the file, or None when anything else differs: a key outside
    `jobs`, or a job added or removed."""
    import yaml

    old, new = yaml.safe_load(old_text), yaml.safe_load(new_text)
    if not isinstance(old, dict) or not isinstance(new, dict):
        return None
    if {k: v for k, v in old.items() if k != "jobs"} != {k: v for k, v in new.items() if k != "jobs"}:
        return None
    oj, nj = old.get("jobs") or {}, new.get("jobs") or {}
    if set(oj) != set(nj):
        return None
    return {j for j in nj if oj[j] != nj[j]}


def ci_yml_reach(base):
    """(everything, crates, groups) the change to ci.yml since commit base
    needs, job by job; everything when it cannot be read that way."""
    if not base:
        return True, set(), set()
    try:
        old = subprocess.run(["git", "show", f"{base}:{CI_YML}"], cwd=ROOT, capture_output=True, text=True, check=True).stdout
        with open(os.path.join(ROOT, CI_YML), encoding="utf-8") as fh:
            changed = ci_jobs_changed(old, fh.read())
    except Exception as e:  # no PyYAML, no such commit, a file that does not parse
        print(f"ci.yml read as a whole ({type(e).__name__}: {e})", file=sys.stderr)
        return True, set(), set()
    return jobs_reach(changed)


def jobs_reach(changed):
    """(everything, crates, groups) for a set of ci.yml's jobs changed."""
    if changed is None:
        return True, set(), set()
    crates, groups = set(), set()
    for job in changed:
        r = CI_JOBS.get(job)
        if r is None:
            return True, set(), set()
        crates |= r[0]
        groups |= r[1]
    return False, crates, groups


def reach(p, base=None):
    """(everything, crates, groups) a change to path p needs."""
    if p in EVERYTHING:
        return True, set(), set()
    if p == CI_YML:
        return ci_yml_reach(base)
    if p in COMPILED_IN:
        return (False, *COMPILED_IN[p])
    if GUIDE.match(p):
        return False, set(), {"docs"}
    if p.startswith("docs/") or p.endswith(".md"):
        return False, set(), set()
    if RESULTS.match(p) and not RESULTS_CODE.match(p):
        return False, set(), set()
    if NOTHING.match(p):
        return False, set(), set()
    for prefix, (cs, gs) in AREAS.items():
        if p.startswith(prefix):
            return False, set(cs), set(gs)
    return True, set(), set()


def plan(paths, everything=False, base=None):
    full = everything
    crates, groups = set(), set()
    for p in paths:
        p = p.strip()
        if not p:
            continue
        f, cs, gs = reach(p, base)
        full |= f
        crates |= cs
        groups |= gs
    return output(full, crates, groups)


def output(full, crates, groups):
    """The JSON for what reaches (full, crates, groups)."""
    if full:
        crates = set(CRATES)
        groups = {*SMOKE, *JOBS, *ENGINE, "audio", *(f"vision:{v}" for v in VISION)}
    out = {"full": full, "crates": [c for c in CRATES if c in crates]}
    for g in ENGINE:
        out[g] = g in groups
    out["vision"] = [v for v in VISION if f"vision:{v}" in groups]
    out["audio"] = "audio" in groups
    for g in SMOKE + JOBS:
        out[g] = g in groups
    out["smoke"] = any(out[g] for g in SMOKE)
    out["server"] = "server" in groups or out["smoke"] or out["python"] or out["javascript"] or out["docs"]
    out["test"] = bool(out["crates"])
    out["runner"] = "chloedb" in crates or "chloedb-server" in crates
    out["any"] = full or bool(out["crates"]) or bool(out["vision"]) or out["audio"] or out["server"] or any(out[g] for g in SMOKE + JOBS + ENGINE)
    return out


def compiled_in():
    """(crate folder, file) for every file a Rust source includes with
    include_str! or include_bytes! from outside its top-level folder, or
    that is Markdown."""
    pat = re.compile(r'include_(?:str|bytes)!\(\s*"([^"]+)"')
    found = set()
    for top in sorted(os.listdir(ROOT)):
        d = os.path.join(ROOT, top)
        if top.startswith(".") or not os.path.isdir(d):
            continue
        for dp, dns, fns in os.walk(d):
            dns[:] = [x for x in dns if x not in ("target", "node_modules", ".venv", ".git")]
            for fn in fns:
                if not fn.endswith(".rs"):
                    continue
                src = os.path.join(dp, fn)
                with open(src, encoding="utf-8", errors="replace") as fh:
                    text = fh.read()
                for m in pat.finditer(text):
                    rel = os.path.relpath(os.path.normpath(os.path.join(dp, m.group(1))), ROOT).replace(os.sep, "/")
                    if not rel.startswith(top + "/") or rel.endswith(".md"):
                        found.add((top, rel))
    return found


def ci_scripts():
    """The paths under .github/ and scripts/ that ci.yml's steps name, and
    that exist (comments left out)."""
    with open(os.path.join(ROOT, ".github/workflows/ci.yml"), encoding="utf-8") as fh:
        lines = [ln for ln in fh if not ln.lstrip().startswith("#")]
    pat = re.compile(r"(?<![\w./-])((?:\.github|scripts)/[\w./-]*[\w-])")
    return {p for ln in lines for p in pat.findall(ln) if os.path.exists(os.path.join(ROOT, p))}


def self_test():
    def on(paths, everything=False):
        r = plan(paths, everything)
        keys = {k for k, v in r.items() if v is True} - {"test"}
        return keys | {f"test:{c}" for c in r["crates"]} | {f"vision:{v}" for v in r["vision"]}

    every = {"any", "full", "runner", "server", "smoke", "audio", *ENGINE, *SMOKE, *JOBS, *(f"test:{c}" for c in CRATES), *(f"vision:{v}" for v in VISION)}
    starts = {"server", "smoke", *SMOKE, "python", "javascript", "docs"}
    server = {"any", "runner", "test:chloedb-server", "vision:chloedb-server", "audio"} | starts
    assert on([], everything=True) == every
    assert on(["chloedb/src/lib.rs"]) == every
    assert on(["chloedb/fuzz/src/lib.rs"]) == every
    assert on(["chloedb/Cargo.lock"]) == every
    assert on([".github/workflows/ci.yml"]) == every  # without a base
    assert on([".github/scripts/ci_changes.py"]) == every
    assert on(["LICENSE"]) == every
    assert on(["something/new.txt"]) == every
    assert on([".github/scripts/new_check.py"]) == every
    assert on(["scripts/new-tool.sh"]) == every
    # The server: its own jobs and every job that starts one, not
    # ChloeDB's (kernels, datafusion, faults, the gates, miri), not the
    # other crates' tests, python-gate or Java.
    assert on(["chloedb-server/src/main.rs"]) == server
    assert on(["chloedb-server/ui/app/core.js", "chloedb-server/Cargo.lock"]) == server
    assert on(["chloedb-server/deploy/k8s/operator/operator.yaml"]) == server
    assert not plan(["chloedb-server/src/main.rs"])["full"]
    assert plan(["chloedb-server/src/main.rs"])["vision"] == ["chloedb-server"]
    assert plan(["chloedb/src/lib.rs"])["vision"] == ["chloedb", "chloedb-server"]
    assert on(["chloedb-server/results/x/run.json"]) == set()
    # The pages the server compiles in reach its tests, the rest of docs nothing.
    assert on(["docs/chloedb/runbooks.md"]) == {"any", "runner", "test:chloedb-server"}
    assert on(["README.md", "docs/chloedb/server.md", "chloedb/results/x/run.json"]) == set()
    assert on(["docs/guide/search.md"]) == {"any", "docs", "server"}
    assert on(["chloedb/results/sequence/gate.py"]) == every
    assert on(["chloedb/notes/text.md", "chloedb/report/graph.html", "chloedb/scripts/prepare_beir.py", "research/cpu-encoders/sparse.py"]) == set()
    # The rest of .github and the scripts.
    assert on([".github/workflows/release-gate.yml", ".github/workflows/demos.yml", ".github/workflows/server-release.yml"]) == set()
    assert on([".github/scripts/door_check.py", ".github/CODEOWNERS", ".github/pull_request_template.md"]) == set()
    assert on(["scripts/check-old-names.py", "scripts/perf-gate.py", "scripts/ci-local.sh"]) == set()
    assert on(["install.sh", "packaging/homebrew/formula.sh", "deny.toml"]) == set()
    assert on([".github/ci-runner/ci_runner.py"]) == {"any", "runner", "server", "test:chloedb", "test:chloedb-server"}
    assert on([".claude-plugin/marketplace.json"]) == {"any", "test:chloectx"}
    # The apps and packages.
    assert on(["chloedb-sheets/src/lib.rs"]) == {"any", "test:chloedb-sheets", "smoke", "smoke_sheets", "docs", "server"}
    assert on(["chloedb-netops/src/lib.rs"]) == set()
    assert on(["chloedb-hn/src/nl.rs", "chloedb-hn/ui/view.js"]) == set()
    assert on(["chloedb-arxiv/src/main.rs", "chloedb-arxiv/ui/view.js"]) == set()
    assert on(["chloedb-gh/src/pages.rs", "chloedb-gh/ui/view.js", "chloedb-gh/deploy/vec.yaml"]) == set()
    assert on(["chloedb-sound/src/sound.rs", "chloedb-sound/ui/view.js", "chloedb-sound/tools/prepare.py"]) == set()
    assert on(["examples/parley/parley/app.py", "examples/feature-store/featurestore.py"]) == set()
    assert on(["chloedb-netops/src/lib.rs", "chloedb-sheets/src/lib.rs"]) == {"any", "test:chloedb-sheets", "smoke", "smoke_sheets", "docs", "server"}
    assert on(["chloedb-crawl/src/lib.rs"]) == {"any", "test:chloedb-crawl", "smoke", "smoke_crawl", "docs", "server"}
    assert on(["chloedb-workbench/src/main.rs"]) == {"any", "test:chloedb-workbench"}
    assert on(["chloedb-workbench/GUIDE.md"]) == {"any", "test:chloedb-workbench"}
    assert on(["chloedb-workbench/README.md"]) == set()
    assert on(["chloectx/src/app.rs"]) == {"any", "test:chloectx", "smoke", "smoke_ctx", "docs", "server"}
    assert on(["chloedb-apps/src/x.js"]) == {"any", "smoke", "smoke_apps", "server"}
    assert on(["chloedb-swarm/x.js"]) == set()
    assert on(["chloedb-py/python/chloedb/x.py"]) == {"any", "python", "python_gate", "docs", "server"}
    assert on(["spec/replication/ShardElection.tla", "spec/check.py"]) == set()
    assert on(["spec/check.py", "chloedb-server/src/cores/shard.rs"]) == on(["chloedb-server/src/cores/shard.rs"])
    assert on(["chloedb-js/client.js"]) == {"any", "javascript", "server"}
    assert on(["chloedb-java/pom.xml"]) == {"any", "java"}
    assert on(["chloedb-java/pom.xml", "chloedb-js/client.js"]) == {"any", "java", "javascript", "server"}
    assert on(["chloedb-java/pom.xml", "chloedb/src/x.rs"]) == every
    assert on(["chloedb-server/src/main.rs", "chloedb-java/pom.xml"]) == server | {"java"}
    assert on(["chloedb-server/src/main.rs", "chloedb-py/src/lib.rs"]) == server | {"python_gate"}
    assert plan(["chloectx/src/a.rs", "chloedb-crawl/src/b.rs"])["crates"] == ["chloectx", "chloedb-crawl"]
    assert plan(["chloectx/src/a.rs"])["runner"] is False
    assert not any(plan(["chloedb-server/src/main.rs"])[g] for g in ENGINE)

    # ci.yml, job by job: its jobs and the jobs that use theirs.
    def jobs(changed):
        r = output(*jobs_reach(changed))
        return ({k for k, v in r.items() if v is True} - {"test"}) | {f"test:{c}" for c in r["crates"]} | {f"vision:{v}" for v in r["vision"]}

    assert jobs(set()) == set()
    assert jobs(None) == every
    assert jobs({"changes"}) == every
    assert jobs({"ci-result"}) == every
    assert jobs({"a-new-job"}) == every
    assert jobs({"miri"}) == {"any", "miri"}
    assert jobs({"kernels", "faults"}) == {"any", "kernels", "faults"}
    assert jobs({"audio"}) == {"any", "audio", "audio_chloedb"}
    assert jobs({"vision"}) == {"any", "vision:chloedb", "vision:chloedb-server"}
    assert jobs({"server-bin"}) == {"any"} | starts
    assert jobs({"test", "macos"}) == {"any", "runner", "macos", *(f"test:{c}" for c in CRATES)}
    assert jobs({"docs-site"}) == set()
    assert jobs({"python-gate"}) == {"any", "python_gate"}
    try:
        import yaml  # noqa: F401
    except ImportError:
        print("PyYAML missing: ci.yml's jobs not compared (a change to ci.yml runs everything)")
    else:
        base = """
on: {push: {branches: [main]}}
env: {RUST_TOOLCHAIN: "1.100.0"}
jobs:
  test:
    runs-on: ubuntu-latest
    steps: &steps
      - run: cargo test
  macos: {runs-on: macos-latest, steps: *steps}
  miri: {runs-on: ubuntu-latest, steps: [{run: cargo miri test}]}
"""
        assert ci_jobs_changed(base, base + "# a comment\n") == set()
        assert ci_jobs_changed(base, base.replace("cargo miri test", "cargo miri test --lib")) == {"miri"}
        assert ci_jobs_changed(base, base.replace("run: cargo test", "run: cargo test --locked")) == {"test", "macos"}
        assert ci_jobs_changed(base, base.replace('"1.100.0"', '"1.101.0"')) is None
        assert ci_jobs_changed(base, base + "  kernels: {runs-on: ubuntu-latest}\n") is None
        # Every key ci.yml's jobs read is one plan() writes.
        with open(os.path.join(ROOT, CI_YML), encoding="utf-8") as fh:
            read = set(re.findall(r"outputs\.run\)\.([a-z_]+)", fh.read()))
        assert read <= set(plan([], everything=True)), read - set(plan([], everything=True))
        # Every job of this ci.yml has its entry, and nothing else does.
        with open(os.path.join(ROOT, CI_YML), encoding="utf-8") as fh:
            assert set(yaml.safe_load(fh)["jobs"]) == set(CI_JOBS), set(yaml.safe_load(open(os.path.join(ROOT, CI_YML)))["jobs"]) ^ set(CI_JOBS)

    # Every file a crate compiles in from outside its folder, or that is
    # Markdown, reaches that crate's tests (or its package's jobs).
    for crate, path in sorted(compiled_in()):
        r = plan([path])
        if crate in CRATES:
            assert r["full"] or crate in r["crates"], f"{crate} compiles in {path}, which does not reach its tests: add it to COMPILED_IN"
        else:
            assert r["any"], f"{crate} compiles in {path}, which reaches no job: add it to COMPILED_IN"
    # Every script ci.yml's steps name reaches a job, or is one the
    # documentation site's check (every change) runs, or is only named
    # (main-builds' release notes name fetch-build.sh).
    every_change = {"scripts/check-old-names.py", "scripts/check-documented-python.py"}
    named_only = {"scripts/fetch-build.sh"}
    for path in sorted(ci_scripts()):
        assert path in every_change | named_only or plan([path])["any"], f"ci.yml runs {path}, which reaches no job: give it the jobs that run it"
    print("ok")


if __name__ == "__main__":
    if "--self-test" in sys.argv:
        self_test()
    else:
        everything = "--all" in sys.argv
        base = sys.argv[sys.argv.index("--base") + 1] if "--base" in sys.argv else None
        print("run=" + json.dumps(plan([] if everything else sys.stdin, everything, base), separators=(",", ":")))
