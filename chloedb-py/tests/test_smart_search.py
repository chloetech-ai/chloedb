"""Search that forgives typos, and answers that cite their rows (db.ask)."""

import http.server
import json
import threading

import pytest

import chloedb
from chloedb.build import col

BOOKS = {"table": "books", "key": "id", "fields": [{"name": "id", "type": "keyword"}, {"name": "title", "type": "text"}, {"name": "year", "type": "int"}]}


@pytest.fixture
def db(tmp_path):
    with chloedb.Database.create(tmp_path / "b.chloedb", BOOKS) as d:
        d.insert([{"id": "b1", "title": "The river and the dragon", "year": 1999}, {"id": "b2", "title": "Winter garden mystery", "year": 2005}])
        yield d


def test_a_word_in_no_row_is_a_typo(db):
    assert [r["id"] for r in db.search("dragn")] == ["b1"]
    assert [r["id"] for r in db.search("dragn", where=col("year") > 1990)] == ["b1"]
    assert [r["id"] for r in db.search("dragn", where="year > :y", params={"y": 1990})] == ["b1"]
    assert len(db.search("dragn", typos=False)) == 0


def test_ask_cites_the_rows_it_answers_from(db, monkeypatch):
    seen = {}

    class Messages(http.server.BaseHTTPRequestHandler):
        def do_POST(self):
            if self.path != "/v1/messages":
                self.send_error(404)
                return
            seen["body"] = json.loads(self.rfile.read(int(self.headers["content-length"])))
            out = json.dumps({"id": "m", "type": "message", "role": "assistant", "model": "x", "content": [{"type": "text", "text": "The river and the dragon [1]."}], "stop_reason": "end_turn", "usage": {"input_tokens": 1, "output_tokens": 1}}).encode()
            self.send_response(200)
            self.send_header("content-type", "application/json")
            self.send_header("content-length", str(len(out)))
            self.end_headers()
            self.wfile.write(out)

        def log_message(self, *a):
            pass

    server = http.server.HTTPServer(("127.0.0.1", 0), Messages)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    try:
        # Anthropic's SDKs' form of the variable: without /v1.
        monkeypatch.setenv("ANTHROPIC_BASE_URL", f"http://127.0.0.1:{server.server_port}")
        monkeypatch.setenv("NO_PROXY", "127.0.0.1")
        monkeypatch.delenv("ANTHROPIC_API_KEY", raising=False)
        r = db.ask("which book has a dragon in it?")
    finally:
        server.shutdown()
    assert r["answer"] == "The river and the dragon [1]."
    assert [(s["n"], s["key"]) for s in r["sources"]] == [(1, "b1")]
    assert r["sources"][0]["row"]["title"] == "The river and the dragon"
    prompt = json.dumps(seen["body"])
    assert "which book has a dragon in it?" in prompt and "[1] (id b1)" in prompt and "Cite each claim" in prompt


def test_ask_with_nothing_found_asks_no_model(db):
    r = db.ask("quantum chromodynamics")
    assert r["sources"] == [] and r["model"] is None and "nothing to answer it from" in r["answer"]
