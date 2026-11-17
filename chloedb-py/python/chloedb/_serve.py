"""`chloedb.serve`: a chloedb-server started on this machine, and a `Client`
of it.

The program comes from the `chloedb-server` package (`pip install
chloedb-server`, or `pip install "chloedb[server]"`, which installs the
prebuilt server of the same version), `$CHLOEDB_SERVER_BIN`, or the PATH.
Like the client, it needs only the standard library."""

from __future__ import annotations

import atexit
import os
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time
import urllib.error
import urllib.request
from typing import Any, Iterable

from ._native import ChloeDBError
from .client import Client

INSTALL = ("there is no prebuilt chloedb-server for Windows: run it in WSL 2 or Docker" if sys.platform == "win32" else
           "pip install chloedb-server (or pip install \"chloedb[server]\") installs it prebuilt")
# Requests to the server it started go to it, never through $HTTP_PROXY.
_DIRECT = urllib.request.build_opener(urllib.request.ProxyHandler({}))


def find_server(binary: str | os.PathLike | None = None) -> str:
    """The chloedb-server program `serve` runs: `binary` (a path, or a name
    on the PATH), else `$CHLOEDB_SERVER_BIN`, else the one the
    `chloedb-server` package installed, else `chloedb-server` on the PATH.
    Raises FileNotFoundError when there is none."""
    if binary is None:
        binary = os.environ.get("CHLOEDB_SERVER_BIN") or None
    if binary is not None:
        path = os.fspath(binary)
        if os.sep not in path and not os.path.exists(path):
            path = shutil.which(path) or path
        if not (os.path.isfile(path) and os.access(path, os.X_OK)):
            raise FileNotFoundError(f"{path} is not a program to run as chloedb-server")
        return os.path.abspath(path)
    try:
        import chloedb_server  # type: ignore[import-not-found]
    except ImportError:
        pass
    else:
        try:
            return chloedb_server.find_bin()
        except FileNotFoundError:
            pass
    path = shutil.which("chloedb-server")
    if path:
        return path
    raise FileNotFoundError(f"chloedb-server is not installed here: {INSTALL}; or pass binary= or set CHLOEDB_SERVER_BIN to one")


def _free_port(host: str) -> int:
    with socket.socket(socket.AF_INET6 if ":" in host else socket.AF_INET) as s:
        s.bind((host, 0))
        return s.getsockname()[1]


def _stop(process: subprocess.Popen, workdir: str | None, timeout: float) -> int | None:
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
    if workdir:
        shutil.rmtree(workdir, ignore_errors=True)
    return process.returncode


# The servers started here and not stopped yet, stopped when Python exits.
_RUNNING: set = set()
_RUNNING_LOCK = threading.Lock()


def _stop_all() -> None:
    with _RUNNING_LOCK:
        servers = list(_RUNNING)
    for server in servers:
        server.stop(timeout=5.0)


atexit.register(_stop_all)


class Server:
    """A chloedb-server this process started (`chloedb.serve` returns a
    `Client` of it, as its `.server`). `url` is where it answers, `db` the
    database it serves, `process` its `subprocess.Popen`, `log` the file
    its output goes to (until it stops). It runs until `stop()`, closing
    the client, the end of the client's `with` block, or the end of this
    Python process."""

    def __init__(self, process: subprocess.Popen, url: str, db: str, log: str, workdir: str, temporary_db: bool):
        self.process = process
        self.url = url
        self.db = db
        self.log = log
        self.temporary_db = temporary_db
        # The folder of the log, and of the database when it is temporary:
        # removed when the server stops.
        self._workdir = workdir
        with _RUNNING_LOCK:
            _RUNNING.add(self)

    @property
    def running(self) -> bool:
        return self.process.poll() is None

    def output(self) -> str:
        """What the server printed so far (its log lines)."""
        try:
            with open(self.log, errors="replace") as f:
                return f.read()
        except OSError:
            return ""

    def stop(self, timeout: float = 10.0) -> int | None:
        """Stops the server (SIGTERM, then SIGKILL after `timeout`
        seconds) and returns its exit status. A temporary database goes
        with it."""
        with _RUNNING_LOCK:
            if self not in _RUNNING:
                return self.process.returncode
            _RUNNING.discard(self)
        return _stop(self.process, self._workdir, timeout)

    def __repr__(self) -> str:
        return f"<chloedb.Server {self.url} {'running' if self.running else 'stopped'}, db {self.db}>"


def _start(exe: str, db: str, listen: str, args: list[str], env: dict, log: str) -> subprocess.Popen:
    with open(log, "ab") as out:
        return subprocess.Popen(
            [exe, "--db", db, "--listen", listen, *args],
            stdin=subprocess.DEVNULL, stdout=out, stderr=subprocess.STDOUT, env=env,
            # Its own session: a Ctrl-C meant for this process (a notebook's
            # interrupt) does not stop it; this process does, at its end.
            start_new_session=True,
        )


def _answers(url: str) -> bool:
    try:
        with _DIRECT.open(url + "/livez", timeout=1):
            return True
    except urllib.error.HTTPError:
        return True  # it answers, behind a token
    except OSError:
        return False


def serve(
    db: str | os.PathLike | None = None,
    *,
    listen: str | None = None,
    token: str | None = None,
    args: Iterable[str] = (),
    binary: str | os.PathLike | None = None,
    wait: float = 60.0,
    timeout: float = 30.0,
    retry_for: float = 10.0,
) -> Client:
    """Starts a chloedb-server on this machine and returns a `Client` of it.

    `db` is the database it serves, made if it does not exist; without one,
    a new database in a temporary folder, removed when the server stops.
    The server runs until the client is closed (or its `with` block ends)
    or this Python process ends; the `Server` is the client's `.server`,
    and `client.server.stop()` stops it too.

    `listen` is its HTTP address, "127.0.0.1:<a free port>" unless given;
    its gRPC API is off, and it starts no apps (`--start-apps off`).
    `token` is the server's token, which the client sends (a server on
    another address than loopback needs one). `args` are more of the
    server's flags (``["--demo", "1000"]``), which win over those. `binary`
    is the program (see `find_server`). `wait` is how many seconds it may
    take to answer; `timeout` and `retry_for` are the client's."""
    exe = find_server(binary)
    extra = [os.fspath(a) for a in args]
    flags = {a.split("=", 1)[0] for a in extra if a.startswith("--")}
    if "--grpc" not in flags:
        extra = ["--grpc", "off", *extra]
    if "--start-apps" not in flags:
        extra = ["--start-apps", "off", *extra]
    env = dict(os.environ)
    if token is not None:
        env["CHLOEDB_TOKEN"] = token  # in its environment, not in `ps`
    workdir = tempfile.mkdtemp(prefix="chloedb-serve-")
    log = os.path.join(workdir, "server.log")
    temporary_db = db is None
    path = os.path.join(workdir, "db.chloedb") if temporary_db else os.path.abspath(os.fspath(db))
    host = "127.0.0.1"
    deadline = time.monotonic() + wait
    try:
        # A free port can be taken between asking for it and the server
        # binding it: then it tries another, a few times.
        for attempt in range(5):
            if listen is not None:
                address = listen
            else:
                address = f"{host}:{_free_port(host)}"
            shown = address.rsplit(":", 1)
            if shown[0] in ("0.0.0.0", "", "[::]", "::"):
                shown[0] = "127.0.0.1"
            url = f"http://{shown[0]}:{shown[1]}"
            process = _start(exe, path, address, extra, env, log)
            while True:
                if _answers(url):
                    server = Server(process, url, path, log, workdir, temporary_db)
                    client = Client(url, token=token, timeout=timeout, retry_for=retry_for)
                    client.server = server
                    return client
                if process.poll() is not None:
                    break
                if time.monotonic() > deadline:
                    _stop(process, None, 5.0)
                    raise ChloeDBError(f"{exe} did not answer at {url} within {wait:g} s; it printed:\n{_tail(log)}")
                time.sleep(0.05)
            said = _tail(log)
            in_use = "in use" in said.lower() or "os error 98" in said or "os error 48" in said
            if listen is not None or not in_use or attempt == 4:
                raise ChloeDBError(f"{exe} stopped with status {process.returncode} before it answered; it printed:\n{said}")
            with open(log, "ab") as f:
                f.write(f"--- port taken, starting again (attempt {attempt + 2})\n".encode())
        raise AssertionError("unreachable")
    except BaseException:
        shutil.rmtree(workdir, ignore_errors=True)
        raise


def _tail(log: str, lines: int = 30) -> str:
    try:
        with open(log, errors="replace") as f:
            return "".join(f.readlines()[-lines:]).rstrip() or "(nothing)"
    except OSError:
        return "(nothing)"


__all__ = ["serve", "find_server", "Server"]
