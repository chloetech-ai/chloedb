"""ChloeDB from Python, under its new name: `import chloedb` is `import strata`.

ChloeDB was called Strata, and its package is still `strata`. This one
answers to the new name: `chloedb` is the `strata` module itself, and each
of its modules answers to both names as the same module (`chloedb.train`
is `strata.train`), so `from chloedb import open` and `import strata` mix
freely.
"""
import importlib
import importlib.abc
import importlib.util
import sys

import strata as _strata


class _NewNames(importlib.abc.MetaPathFinder, importlib.abc.Loader):
    """Imports chloedb.X as the module strata.X."""

    def find_spec(self, name, path, target=None):
        if name.startswith("chloedb."):
            return importlib.util.spec_from_loader(name, self)
        return None

    def create_module(self, spec):
        return importlib.import_module("strata" + spec.name[len("chloedb"):])

    def exec_module(self, module):
        pass


if not any(isinstance(f, _NewNames) for f in sys.meta_path):
    sys.meta_path.insert(0, _NewNames())
sys.modules[__name__] = _strata
