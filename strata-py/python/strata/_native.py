"""The compiled extension, `strata._strata`, which runs the database in
this process. The HTTP client (`strata.Client`, `strata.connect`) needs
only the standard library, so without the extension (a checkout's
``strata-py/python`` on the path, with no build) the package still imports
and the client works: `_strata` then stands in for the extension and says
what to install when anything of it is used."""

from __future__ import annotations

from typing import Any

WHEELS = "https://github.com/chloetech-ai/strata/releases/expanded_assets/strata-py-v0.1.0"


class _Missing:
    """In place of the extension: any use of it raises ImportError."""

    def __init__(self, error: ImportError):
        self._error = error

    def __getattr__(self, name: str) -> Any:
        raise ImportError(
            f"strata's compiled extension is not installed ({self._error}), and {name} needs it: "
            f"install a wheel (pip install strata-db -f {WHEELS}) or build it (maturin develop, in strata-py). "
            "strata.Client and strata.connect, which talk to a strata-server, work without it."
        )


try:
    from . import _strata

    StrataError = _strata.StrataError
    NATIVE = True
except ImportError as e:  # the HTTP client alone
    _strata = _Missing(e)  # type: ignore[assignment]
    NATIVE = False

    class StrataError(Exception):  # type: ignore[no-redef]
        """An error Strata reports. A client's has the answer's HTTP
        `status`, and its JSON body as `answer`."""
