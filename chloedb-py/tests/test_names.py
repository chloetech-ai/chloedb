"""ChloeDB was called Strata: the package answers to both names."""
import subprocess
import sys

import strata


def test_chloedb_is_strata():
    import chloedb
    import chloedb.train
    from chloedb import Database, ngrams

    assert chloedb is strata
    assert chloedb.train is strata.train
    assert ngrams is strata.ngrams
    assert Database is strata.Database


def test_chloedb_first_in_a_fresh_interpreter(tmp_path):
    # Imported first by its new name, then by the old one: one package.
    code = (
        "import chloedb.ngrams, strata, strata.ngrams\n"
        "assert chloedb is strata and chloedb.ngrams is strata.ngrams\n"
        "db = chloedb.open(%r, schema={'table': 't', 'key': 'id', 'fields': [{'name': 'id', 'type': 'keyword'}, {'name': 'body', 'type': 'text'}]})\n"
        "db.insert([{'id': 'a', 'body': 'a river'}])\n"
        "assert db.query(\"SELECT id FROM t WHERE MATCH(body, 'river')\").scalar() == 'a'\n"
    ) % str(tmp_path / "n.strata")
    subprocess.run([sys.executable, "-c", code], check=True)
