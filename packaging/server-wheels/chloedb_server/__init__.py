"""chloedb-server, prebuilt.

This package holds the binary of ChloeDB's server for this machine,
installed as the program `chloedb-server` beside the environment's other
programs (its `bin`). `find_bin()` is the binary's path,
`python -m chloedb_server ARGS` runs it, and `chloedb.serve()` (in the
`chloedb` package) starts one and returns a client of it.

The binary is the one in the release's archive for this platform,
chloedb-server-<platform>.tar.gz, of the same version.
"""

import os
import sys
import sysconfig

__version__ = "@VERSION@"
__all__ = ["__version__", "find_bin"]

EXE = "chloedb-server" + (sysconfig.get_config_var("EXE") or "")


def _bin_dirs():
    """Where an installer puts the program of a wheel: the scripts folder
    of this environment, of a user install (pip install --user), beside the
    package for pip install --target, and of the prefix this package was
    installed under (when it is imported from another environment)."""
    yield sysconfig.get_path("scripts")
    try:
        user = sysconfig.get_preferred_scheme("user")
    except (AttributeError, KeyError):  # before Python 3.10
        user = "osx_framework_user" if sys.platform == "darwin" and sys._framework else f"{os.name}_user"
    try:
        yield sysconfig.get_path("scripts", scheme=user)
    except KeyError:
        pass
    here = os.path.dirname(os.path.abspath(__file__))
    yield os.path.join(os.path.dirname(here), "bin")  # --target DIR: DIR/chloedb_server, DIR/bin
    # <prefix>/lib/pythonX.Y/site-packages/chloedb_server
    yield os.path.join(here, os.pardir, os.pardir, os.pardir, os.pardir, "bin")


def find_bin() -> str:
    """The path of the chloedb-server program this package installed.
    Raises FileNotFoundError when it is not where an installer puts it."""
    seen = []
    for d in _bin_dirs():
        if not d:
            continue
        d = os.path.normpath(d)
        if d in seen:
            continue
        seen.append(d)
        path = os.path.join(d, EXE)
        if os.path.isfile(path) and os.access(path, os.X_OK):
            return path
    raise FileNotFoundError(f"{EXE} is not in any of {', '.join(seen)}: reinstall it (pip install --force-reinstall chloedb-server)")
