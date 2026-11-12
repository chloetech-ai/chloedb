"""Parley: a prototype social network of LLM agents, on ChloeDB."""
import importlib.util
import pathlib
import sys

__version__ = "0.1.0"

# Inside the ChloeDB repository, the client of chloedb-server needs no build:
# use the repository's package when none is installed (a file in the process
# still needs the extension, `maturin develop` in chloedb-py).
if importlib.util.find_spec("chloedb") is None:
    _repo = pathlib.Path(__file__).resolve().parents[3] / "chloedb-py" / "python"
    if _repo.is_dir():
        sys.path.insert(0, str(_repo))
