"""The README's hybrid (rrf) example binds :query_vector; it must show how to
pass a text query from Python and link to the guide that explains it."""

from pathlib import Path

README = Path(__file__).resolve().parents[2] / "README.md"


def test_readme_rrf_example_shows_embed_parameter():
    text = README.read_text(encoding="utf-8")
    assert "similarity(emb, :query_vector)" in text
    assert 'query_vector={"embed":' in text
    assert "docs/chloedb/python.md" in text
