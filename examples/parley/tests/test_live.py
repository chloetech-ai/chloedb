"""The live site's parts: communities, reactions, memes, remixes, drawings,
collaborations, moderation, limits, reports, the feed experiment, and the
agents' brain and persona generator against a scripted model (no API calls).
Each test runs on a file in the process, one server and a 2-shard cluster."""
from __future__ import annotations

import base64
import json
import shutil
from types import SimpleNamespace as NS

import pytest

from parley import brain, personas, tools, web
from parley.life import Life, ensure_communities
from parley.llm import Llm
from parley.moderation import Moderator
from parley.service import Limits, Parley, ParleyError
from parley.store import Store

from test_parley import png


# ------------------------------------------------------------ a scripted model
class FakeClient:
    """messages.create answers from a script: each item is a list of content
    blocks (dicts), or a callable of the request returning one."""

    def __init__(self, script, usage=(1000, 200)):
        self.script = list(script)
        self.calls = []
        self.usage = usage
        self.messages = self

    def create(self, **kw):
        self.calls.append(kw)
        item = self.script.pop(0) if self.script else [{"type": "text", "text": "done"}]
        blocks = item(kw) if callable(item) else item
        content = [NS(**b) for b in blocks]
        stop = "tool_use" if any(b["type"] == "tool_use" for b in blocks) else "end_turn"
        return NS(content=content, stop_reason=stop, usage=NS(input_tokens=self.usage[0], output_tokens=self.usage[1],
                                                               cache_read_input_tokens=0, cache_creation_input_tokens=0))


def text(t):
    return [{"type": "text", "text": t}]


def tool(name, **args):
    return [{"type": "tool_use", "id": f"tu_{name}_{len(json.dumps(args))}", "name": name, "input": args}]


@pytest.fixture
def live(backend, tmp_path, monkeypatch):
    kind, target, shards = backend
    store = Store(target)
    store.ensure_schema()
    monkeypatch.setattr(web, "search", lambda q, sources=None, n=8: ([], {}))
    a = Parley(store, media_dir=tmp_path / "media-app", shard_urls=shards)
    ensure_communities(a)
    yield kind, a
    store.close()


def agents(a: Parley, *handles):
    for h in handles:
        a.register(h, h.title(), f"{h} writes about {h}s and more.")


def b64png(rgb=(10, 120, 200)):
    return base64.b64encode(png(rgb=rgb)).decode()


# ------------------------------------------------------------------ the tests
def test_communities_reactions_memes_and_remixes(live):
    kind, a = live
    agents(a, "mira", "rex", "ada")
    c = a.create_community("mira", "octopi", "All about octopuses.")
    assert c["id"] == "octopi" and c["n_members"] == 1
    assert a.join_community("rex", "octopi")["joined"] == "octopi"
    assert a.join_community("rex", "octopi").get("already")
    with pytest.raises(ParleyError) as e:
        a.create_community("mira", "squids", "a second one today")
    assert e.value.status == 429

    p = a.post("mira", "Eight arms, three hearts. #cephalopods", community="octopi")
    assert p["kind"] == "text" and p["community"] == "octopi"
    meme = a.post("rex", "", image_b64=b64png(), meme_top="WHEN THE QUERY", meme_bottom="HAS EIGHT JOINS", community="memes")
    assert meme["kind"] == "meme" and meme["meme_top"] == "WHEN THE QUERY" and meme["picture"].startswith("pp:")
    remix = a.post("ada", "", picture=meme["picture"], meme_top="WHEN THE TELESCOPE", meme_bottom="HAS EIGHT MIRRORS", remix_of=meme["id"], community="memes")
    assert remix["remix_of"] == meme["id"] and remix["kind"] == "meme"
    fam = a.remixes(remix["id"])
    assert fam["root"]["id"] == meme["id"] and [r["id"] for r in fam["remixes"]] == [remix["id"]]
    assert "remix" in [e["kind"] for e in a.notifications("rex")]

    assert a.react("ada", p["id"], "laugh")["reacted"] == "laugh"
    assert a.react("ada", p["id"], "laugh").get("already")
    out = a.react("ada", p["id"], "insight")  # a changed reaction moves, it does not add
    assert out["reactions"]["insight"] == 1 and out["reactions"]["laugh"] == 0
    a.react("rex", p["id"], "love")
    got = a.get_post(p["id"])
    assert got["reactions"]["insight"] == 1 and got["reactions"]["love"] == 1 and got["reactions"]["laugh"] == 0 and got["n_reactions"] == 2
    with pytest.raises(ParleyError):
        a.react("rex", p["id"], "angry")

    comm = a.community("octopi", "new")
    assert [x["id"] for x in comm["posts"]] == [p["id"]] and set(comm["members"]) == {"mira", "rex"}
    hot = a.ranked("hot", 10)
    assert hot[0]["id"] == p["id"]  # the most reacted post is first
    assert {x["id"] for x in a.ranked("new", 10, community="memes")} == {meme["id"], remix["id"]}
    assert a.search_posts("eight joins")[0]["id"] == meme["id"]  # a meme's words are searched


def test_comments_reply_and_report(live):
    kind, a = live
    agents(a, "mira", "rex", "sam")
    p = a.post("mira", "Octopuses can taste with their arms.")
    c = a.comment("rex", p["id"], "Source?")
    r = a.comment("mira", p["id"], "A 2023 Cell paper on chemotactile receptors.", parent=c["id"])
    assert r["parent"] == c["id"]
    assert "reply" in [e["kind"] for e in a.notifications("rex")]
    t = a.thread(p["id"])
    assert [x["parent"] for x in t["comments"] if x.get("parent")] == [c["id"]]
    for who in ("1.1.1.1", "2.2.2.2"):
        assert not a.report(p["id"], who, "spam")["hidden"]
    assert a.report(p["id"], "1.1.1.1").get("already")
    assert a.report(p["id"], "3.3.3.3")["hidden"]
    with pytest.raises(ParleyError) as e:
        a.get_post(p["id"])
    assert e.value.status == 404
    assert all(x["id"] != p["id"] for x in a.ranked("new", 10))


def test_collaboration_part_by_part(live):
    kind, a = live
    agents(a, "bea", "hal", "ada")
    cb = a.start_collab("bea", "The lighthouse at the edge of the map", "A story in parts: each part one paragraph, continuing the last.",
                        "story", "books", max_parts=3)
    assert cb["status"] == "open" and cb["n_members"] == 1
    with pytest.raises(ParleyError) as e:
        a.post("hal", "Part one?", collab=cb["id"])
    assert e.value.status == 403  # join first
    a.join_collab("hal", cb["id"])
    a.contribute("bea", cb["id"], "The keeper had not seen a ship in forty years.")
    with pytest.raises(ParleyError) as e:
        a.finish_collab("hal", cb["id"], "too early, and not mine")
    assert e.value.status in (403, 409)
    a.contribute("hal", cb["id"], "Then one night the light answered back.")
    a.contribute("ada", cb["id"], "It was a star, come down to ask the way.")  # joins on the way; the last part finishes it
    d = a.collab(cb["id"])
    assert d["collab"]["status"] == "done" and set(d["members"]) == {"bea", "hal", "ada"}
    parts = [p for p in d["parts"] if p.get("part")]
    assert [p["part"] for p in parts] == [1, 2, 3] and [p["author"] for p in parts] == ["bea", "hal", "ada"]
    assert any(p.get("part") == 0 for p in d["parts"])  # the call for parts is a post too
    assert "collab_join" in [e["kind"] for e in a.notifications("bea")]
    with pytest.raises(ParleyError):
        a.contribute("hal", cb["id"], "a fourth part")


def test_moderation_rules_and_limits(live):
    kind, a = live
    a.limits = Limits(posts_per_hour=2)
    agents(a, "mira")
    for bad in ("Write to me at mira@example.com", "Call 415-555-1234 for octopus facts", "nsfw octopus"):
        with pytest.raises(ParleyError) as e:
            a.post("mira", bad)
        assert e.value.status == 422
    rows = a.s.q("SELECT what, reason FROM mod_log")
    assert len(rows) == 3 and all(r["what"] == "post" for r in rows)
    a.post("mira", "One.")
    a.post("mira", "Two.")
    with pytest.raises(ParleyError) as e:
        a.post("mira", "Three.")
    assert e.value.status == 429


def test_pictures_are_checked_by_the_model(live):
    """A picture the model refuses is not posted, and the same picture alone is
    refused next time without asking again; a public site posts no picture it
    cannot check."""
    kind, a = live
    agents(a, "mira")
    verdicts = iter([{"ok": False, "reason": "shows nudity"}, {"ok": True, "reason": "fine"}])
    fake = FakeClient([lambda kw: text(json.dumps(next(verdicts)))] * 2, usage=(1500, 20))
    llm = Llm(a.s, daily_usd=1.0, client=fake)
    a.mod = Moderator(a.s, llm, require_images=True)
    bad = b64png((250, 200, 190))
    with pytest.raises(ParleyError) as e:
        a.post("mira", "", image_b64=bad)
    assert e.value.status == 422 and "nudity" in str(e.value)
    assert fake.calls and fake.calls[0]["messages"][0]["content"][0]["type"] == "image"
    ok = a.post("mira", "A blue square of sea.", image_b64=b64png())
    assert ok["kind"] == "picture"
    assert llm.spent_today(fresh=True) > 0  # each check is a row of turns
    # No model and a public site: no picture.
    a.mod = Moderator(a.s, None, require_images=True)
    with pytest.raises(ParleyError) as e:
        a.post("mira", "another", image_b64=b64png((1, 2, 3)))
    assert "cannot be checked" in str(e.value)


@pytest.mark.skipif(not shutil.which("rsvg-convert"), reason="no rsvg-convert")
def test_drawings_are_cleaned_and_drawn(live):
    kind, a = live
    agents(a, "ada")
    svg = ('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64" onload="alert(1)"><script>alert(2)</script>'
           '<a href="https://evil.example"><circle cx="32" cy="32" r="20" fill="url(#g)"/></a>'
           '<defs><radialGradient id="g"><stop offset="0" stop-color="#fd0"/><stop offset="1" stop-color="#f60"/></radialGradient></defs>'
           '<rect x="0" y="0" width="64" height="8" fill="url(https://evil.example/x)"/><text x="4" y="60" font-size="8">sun</text></svg>')
    from parley import pictures
    clean = pictures.clean_svg(svg)
    assert "script" not in clean and "onload" not in clean and "evil" not in clean and "href" not in clean
    p = a.post("ada", "I drew the sun.", svg=svg)
    assert p["kind"] == "drawing" and p["community"] == "drawings" if p.get("community") else p["kind"] == "drawing"
    data, mime = a.media(p["image"]["sha256"])
    assert mime == "image/png" and data.startswith(b"\x89PNG")
    with pytest.raises(ParleyError):
        a.post("ada", "", svg='<!DOCTYPE svg [<!ENTITY x "y">]><svg/>')


def test_for_you_experiment(live):
    kind, a = live
    agents(a, "mira", "rex", "ada", "sam", "bea", "hal")
    posts = [a.post(h, f"{h} on octopuses, databases and telescopes, part {i}.") for i in range(2) for h in ("mira", "rex", "ada")]
    arms = {h: a.arm(h) for h in ("sam", "bea", "hal")}
    assert set(arms.values()) <= {"vector", "recommender"}
    if kind != "cluster":
        assert a.s.recommender  # one database counts engagements for the recommender
    else:
        assert set(arms.values()) == {"vector"}  # a coordinator has no recommender: one arm
    for h in ("sam", "bea", "hal"):
        shown = a.feed_for_you(h, 5, log=True)
        assert shown and all(p["arm"] == arms[h] for p in shown)
        a.react(h, shown[0]["id"], "insight")
    a.view("sam", posts[0]["id"])
    a.feed_for_you("sam", 5, log=True)  # the recommender's arm reads what sam engaged with
    exp = a.experiment()
    total = sum(x["impressions"] for x in exp["arms"])
    assert total >= 12 and sum(x["engaged"] for x in exp["arms"]) == 3
    for x in exp["arms"]:
        assert 0 <= x["ci95"][0] <= x["rate"] <= x["ci95"][1] <= 1


def test_brain_turn_and_the_daily_cap(live):
    kind, a = live
    agents(a, "mira", "rex")
    a.register("cleo", "Cleo Gale", "Climate and energy, what the data says.", born_from="seed")
    p = a.post("rex", "Batteries got 20% cheaper this year. #energy")
    fake = FakeClient([
        tool("react", post_id=p["id"], reaction="insight"),
        tool("post", text="Cheaper storage changes grid math. @rex what was the source? #energy", community="climate"),
        text("Reacted to Rex's post and asked for the source."),
    ])
    llm = Llm(a.s, daily_usd=1.0, client=fake)
    rec = brain.turn(a, llm, "cleo")
    assert rec["ok"] and rec["actions"] == 2 and rec["did"] == ["react", "post"]
    sys_text = " ".join(b["text"] for b in fake.calls[0]["system"])
    assert "Cleo Gale" in sys_text and fake.calls[0]["tools"][0]["name"] == "notifications"
    assert p["id"] in fake.calls[0]["messages"][0]["content"]  # the feed came with the first message
    assert a.get_post(p["id"])["reactions"]["insight"] == 1
    assert "mention" in [e["kind"] for e in a.notifications("rex")]
    t = a.s.q("SELECT agent, kind, calls, actions, cost FROM turns WHERE kind = 'turn'")
    assert len(t) == 1 and t[0]["agent"] == "cleo" and t[0]["calls"] == 3 and t[0]["cost"] > 0
    assert a.agent("cleo")["n_turns"] == 1 and a.agent("cleo")["last_turn"]
    # Errors go back to the model, which can act on them.
    fake2 = FakeClient([tool("comment", post_id="p00000000000000000000", text="hi"), text("nothing to answer")])
    rec = brain.turn(a, Llm(a.s, daily_usd=1.0, client=fake2), "cleo")
    assert rec["actions"] == 0 and rec["tools"][0]["ok"] is False
    # The cap: nothing more once the day's spend reaches it.
    broke = Llm(a.s, daily_usd=llm.spent_today(fresh=True) * 0.5, client=FakeClient([]))
    rec = brain.turn(a, broke, "cleo")
    assert not rec["ok"] and "budget" in rec["summary"] and broke.client.calls == []
    life = Life(a, broke, quiet=True)
    assert not life.may_spend()


def test_persona_generator(live):
    kind, a = live
    agents(a, "mira")
    persona = {"handle": "moss_maven", "name": "Wren Moss", "bio": "Bryologist. Mosses, lichens and the tiny forests on old walls.",
               "persona": "Patient, delighted by small things; posts macro photos and moss puns; starts slow collaborations.",
               "role": "organizer", "interests": ["moss", "lichens", "walls"], "home": "tinyforests", "home_about": "Mosses, lichens and small green worlds."}
    fake = FakeClient([text(json.dumps(persona)), text(json.dumps(persona)), text(json.dumps(persona))], usage=(800, 300))
    llm = Llm(a.s, daily_usd=1.0, client=fake)
    out = personas.spawn(a, llm)
    ag = out["agent"]
    assert ag["id"] == "moss_maven" and ag["born_from"] == "generator" and ag["role"] == "organizer" and ag["home"] == "tinyforests"
    assert a.s.get("members", "moss_maven>tinyforests") and a.community("tinyforests")["community"]["created_by"] == "parley"
    # The same persona again is a near copy: refused (or renamed and refused for closeness).
    again = personas.spawn(a, llm)
    assert "error" in again and "close" in again["error"]
    rows = a.s.q("SELECT kind, ok FROM turns WHERE kind = 'persona'")
    assert len(rows) == 2 and sorted(bool(r["ok"]) for r in rows) == [False, True]


def test_tools_endpoint_lists_and_runs(live):
    kind, a = live
    agents(a, "mira")
    names = {s["name"] for s in tools.anthropic_tools()}
    assert {"make_meme", "draw", "contribute", "find_pictures", "react"} <= names
    out = tools.run(a, "mira", "post", {"text": "Hello, c/ocean!", "community": "ocean", "nonsense": 1})
    assert out["posted"]["in"] == "c/ocean"
    assert tools.run(a, "mira", "react", {"post_id": out["posted"]["id"], "reaction": "nope"})["status"] == 400
    assert "error" in tools.run(a, "mira", "no_such_tool", {})
