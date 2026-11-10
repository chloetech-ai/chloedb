"""The compiled extension, `chloedb._chloedb`, which runs the database in
this process. The HTTP client (`chloedb.Client`, `chloedb.connect`) needs
only the standard library, so without the extension (a checkout's
``chloedb-py/python`` on the path, with no build) the package still imports
and the client works: `_strata` then stands in for the extension and says
what to install when anything of it is used."""

from __future__ import annotations

from typing import Any

WHEELS = "https://github.com/chloetech-ai/chloedb/releases/expanded_assets/strata-py-v0.1.0"


class _Missing:
    """In place of the extension: any use of it raises ImportError."""

    def __init__(self, error: ImportError):
        self._error = error

    def __getattr__(self, name: str) -> Any:
        raise ImportError(
            f"chloedb's compiled extension is not installed ({self._error}), and {name} needs it: "
            f"install a wheel (pip install strata-db -f {WHEELS}) or build it (maturin develop, in chloedb-py). "
            "chloedb.Client and chloedb.connect, which talk to a chloedb-server, work without it."
        )


try:
    from . import _chloedb as _strata

    ChloeDBError = _strata.ChloeDBError
    NATIVE = True
except ImportError as e:  # the HTTP client alone
    _strata = _Missing(e)  # type: ignore[assignment]
    NATIVE = False

    class ChloeDBError(Exception):  # type: ignore[no-redef]
        """An error ChloeDB reports. A client's has the answer's HTTP
        `status`, and its JSON body as `answer`."""


# Its name when ChloeDB was called Strata: the same class.
StrataError = ChloeDBError
