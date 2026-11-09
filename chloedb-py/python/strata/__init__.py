"""ChloeDB's old name: `import strata` is `import chloedb`.

ChloeDB was called Strata, and its package was `strata`. It is `chloedb`
now; this one keeps the old name working for a release: `strata` is the
`chloedb` module itself, and each of its modules answers to both names as
the same module (`strata.train` is `chloedb.train`), so `import strata`
and `from chloedb import open` mix freely. Importing it gives a
DeprecationWarning.
"""
import importlib
import importlib.abc
import importlib.util
import sys
import warnings

import chloedb as _chloedb

warnings.warn("the strata package is called chloedb now: import chloedb", DeprecationWarning, stacklevel=2)


class _OldNames(importlib.abc.MetaPathFinder, importlib.abc.Loader):
    """Imports strata.X as the module chloedb.X."""

    def find_spec(self, name, path, target=None):
        if name.startswith("strata."):
            return importlib.util.spec_from_loader(name, self)
        return None

    def create_module(self, spec):
        return importlib.import_module("chloedb" + spec.name[len("strata"):])

    def exec_module(self, module):
        pass


if not any(isinstance(f, _OldNames) for f in sys.meta_path):
    sys.meta_path.insert(0, _OldNames())
sys.modules[__name__] = _chloedb
