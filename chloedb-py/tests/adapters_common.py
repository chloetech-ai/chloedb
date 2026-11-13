"""What the framework adapters' tests share: an embedding of words that
puts texts with words in common near each other, and stores of both kinds
(a database in the process, and one behind chloedb-server when it is
built)."""

import math
import re
import zlib

import pytest

import chloedb
from conftest import server_binary

DIMS = 64


def words_vector(text, dims=DIMS):
    """A bag of words, each word hashed to a dimension: texts that share
    words point the same way."""
    v = [0.0] * dims
    for w in re.findall(r"[a-z0-9]+", text.lower()):
        v[zlib.crc32(w.encode()) % dims] += 1.0
    n = math.sqrt(sum(x * x for x in v)) or 1.0
    return [x / n for x in v]


@pytest.fixture(params=["local", "server"])
def target(request, tmp_path):
    """Where a store goes: a database's path, or a server's URL."""
    if request.param == "local":
        return str(tmp_path / "adapters.chloedb")
    if not server_binary():
        pytest.skip("chloedb-server is not built")
    serve = request.getfixturevalue("serve")
    path = tmp_path / "served.chloedb"
    with chloedb.Database.create(path, {"table": "first", "key": "id", "fields": [{"name": "id", "type": "keyword"}]}):
        pass
    return serve(path)
