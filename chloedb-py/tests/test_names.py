"""ChloeDB was called Strata: the package answers to both names."""
import subprocess
import sys
import warnings

import chloedb


def test_strata_is_chloedb():
    with warnings.catch_warnings(record=True):
        warnings.simplefilter("always")
        import strata
        import strata.train
        from strata import Database, ngrams

    assert strata is chloedb
    assert strata.train is chloedb.train
    assert ngrams is chloedb.ngrams
    assert Database is chloedb.Database
    assert chloedb.StrataError is chloedb.ChloeDBError


def test_strata_first_in_a_fresh_interpreter(tmp_path):
    # Imported first by its old name, then by the new one: one package, and
    # the old name warns.
    code = (
        "import warnings\n"
        "with warnings.catch_warnings(record=True) as w:\n"
        "    warnings.simplefilter('always')\n"
        "    import strata.ngrams\n"
        "assert any(issubclass(x.category, DeprecationWarning) and 'chloedb' in str(x.message) for x in w), w\n"
        "import chloedb, chloedb.ngrams\n"
        "assert chloedb is strata and chloedb.ngrams is strata.ngrams\n"
        "db = strata.open(%r, schema={'table': 't', 'key': 'id', 'fields': [{'name': 'id', 'type': 'keyword'}, {'name': 'body', 'type': 'text'}]})\n"
        "db.insert([{'id': 'a', 'body': 'a river'}])\n"
        "assert db.query(\"SELECT id FROM t WHERE MATCH(body, 'river')\").scalar() == 'a'\n"
    ) % str(tmp_path / "n.strata")
    subprocess.run([sys.executable, "-c", code], check=True)
