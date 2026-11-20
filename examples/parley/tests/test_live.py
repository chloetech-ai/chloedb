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


def test_solving_a_problem_together(live):
    from parley import problems
    kind, a = live
    for h, role, interests in [("ada", "explainer", ["gardening", "plants"]), ("basil", "hobbyist", ["food chemistry", "tomatoes"]),
                               ("cleo", "optimist", ["weather", "rain"]), ("sam", "skeptic", ["fact checking"]), ("rex", "critic", ["databases"])]:
        a.register(h, h.title(), f"{h} knows about {', '.join(interests)}.", role=role, interests=interests, born_from="seed")
    p = problems.ask(a, "Why do my tomatoes crack after rain?",
                     "Every time it rains after a dry spell our tomatoes split near the top. What should we change?", visitor="1.2.3.4", name="Robin")
    pid = p["id"]
    d = problems.view(a, pid)
    team = {t["agent"] for t in d["team"]}
    assert len(team) == 4 and team & {"sam", "rex"}, team  # always someone whose job is to doubt
    assert d["problem"]["asked_by"] == "visitor" and d["problem"]["asker_name"] == "Robin" and "asker" not in d["problem"]
    invited = a.s.q("SELECT agent FROM events WHERE kind = 'problem_invite'")
    assert {r["agent"] for r in invited} == team
    # Notes: kinds are checked, agreeing needs a note to agree with.
    with pytest.raises(ParleyError):
        problems.note(a, "ada", pid, "rant", "no")
    with pytest.raises(ParleyError):
        problems.note(a, "ada", pid, "agree", "yes")
    q1 = problems.note(a, "ada", pid, "question", "Are the beds watered on a schedule or by hand?")["noted"]
    f1 = problems.note(a, "basil", pid, "fact", "Cracking comes from a sudden uptake of water: the fruit swells faster than its skin.", q1)["noted"]
    assert any(r["agent"] == "ada" for r in a.s.q("SELECT agent FROM events WHERE kind = 'problem_reply'"))
    problems.note(a, "cleo", pid, "idea", "Mulch the beds so the soil moisture swings less after rain.")
    out = tools.run(a, "sam", "add_note", {"problem_id": pid, "kind": "critique", "text": "Mulch helps, but is there evidence it stops cracking?", "reply_to": f1})
    assert out["kind"] == "critique"
    assert problems.needs_synthesis(a) == [pid]
    # The facilitator: too early to be solved, asks only go to the team.
    synth = {"summary": "Cause agreed: uneven water. Remedies proposed: mulch, even watering.", "best_answer": "Water evenly and mulch.",
             "open_questions": ["Which varieties resist cracking?"], "phase": "solved", "confidence": "medium",
             "next_asks": [{"agent": "@basil", "ask": "Which varieties resist cracking?"}, {"agent": "nobody_here", "ask": "x"}],
             "credits": [{"agent": "basil", "contribution": "the cause"}]}
    fake = FakeClient([text(json.dumps(synth))])
    llm = Llm(a.s, daily_usd=1.0, client=fake)
    r = problems.synthesize(a, llm, pid)
    assert r["phase"] == "decide" and [x["agent"] for x in r["asks"]] == ["basil"]
    got = problems.get(a, pid)
    assert got["status"] == "open" and got["answer"] == "Water evenly and mulch." and got["open_questions"] == ["Which varieties resist cracking?"]
    asked = a.s.q("SELECT agent, detail FROM events WHERE kind = 'problem_ask'")
    assert [x["agent"] for x in asked] == ["basil"] and pid in asked[0]["detail"]
    assert problems.needs_synthesis(a) == []
    # The agent's next turn shows the problem and what it was asked.
    ctx = brain.context(a, "basil")
    assert pid in ctx["problems"] and "varieties" in ctx["problems"]
    view = tools.run(a, "basil", "view_problem", {"problem_id": pid})
    assert view["problem"]["phase"] == "decide" and any(n["by"] == "@facilitator" for n in view["notes"])
    p1 = problems.note(a, "basil", pid, "proposal", "Grow Juliet or Mountain Magic, mulch, water deeply twice a week, pick at first blush.")["noted"]
    problems.note(a, "ada", pid, "agree", "Yes: picking at first blush avoids most splits.", p1)
    problems.note(a, "sam", pid, "agree", "Agreed, and the extension service sources back it.", p1)
    problems.note(a, "rex", pid, "agree", "Fine by me.", p1)
    final = dict(synth, phase="solved", confidence="high", best_answer="Resistant varieties, mulch, deep even watering, pick at first blush.",
                 credits=[{"agent": "basil", "contribution": "cause and plan"}, {"agent": "sam", "contribution": "asked for evidence"}])
    llm._client = FakeClient([text(json.dumps(final))])
    r = problems.synthesize(a, llm, pid)
    assert r["phase"] == "solved"
    done = problems.get(a, pid)
    assert done["status"] == "solved" and done["solved"] and [c["agent"] for c in done["credits"]] == ["basil", "sam"]
    with pytest.raises(ParleyError):
        problems.note(a, "ada", pid, "idea", "one more thing")
    assert {p["id"] for p in problems.listing(a, "solved")} == {pid}
    assert a.s.q("SELECT COUNT(*) AS n FROM turns WHERE kind = 'problem'")[0]["n"] == 2
    # Visitors: two problems an hour from one address.
    problems.ask(a, "How do we share one bike among five?", "Five housemates, one cargo bike, everyone needs it on Saturday mornings.", visitor="1.2.3.4")
    with pytest.raises(ParleyError) as e:
        problems.ask(a, "A third problem this hour", "From the same address, which is one too many for now, sorry.", visitor="1.2.3.4")
    assert e.value.status == 429


def test_the_facilitator_picks_a_complementary_team(live):
    from parley import problems
    kind, a = live
    for h, role in [("basil", "hobbyist"), ("cleo", "optimist"), ("sam", "skeptic"), ("marlowe", "memer"), ("wen", "organizer")]:
        a.register(h, h.title(), f"{h} the {role}.", role=role, born_from="seed")
    pick = {"team": [{"agent": "@basil", "angle": "why fruit splits"}, {"agent": "cleo", "angle": "rain and soil moisture"},
                     {"agent": "sam", "angle": "ask for evidence"}, {"agent": "@nobody", "angle": "x"}]}
    a.mod.llm = Llm(a.s, daily_usd=1.0, client=FakeClient([text(json.dumps({"ok": True, "reason": ""})), text(json.dumps(pick))]))
    p = problems.ask(a, "Why do my tomatoes crack after rain?", "They split near the top whenever it rains after a dry spell. What should we change?",
                     visitor="5.6.7.8")
    team = {t["agent"]: t["angle"] for t in problems.view(a, p["id"])["team"]}
    assert team == {"basil": "why fruit splits", "cleo": "rain and soil moisture", "sam": "ask for evidence"}
    assert sorted(r["summary"] for r in a.s.q("SELECT summary FROM turns WHERE kind = 'problem'")) == [f"{p['id']}: team"]


def test_search_like_hn(live):
    """One box over everything: filters typed in the words, typos forgiven,
    facets, highlights, autocomplete, and an answer that cites its posts."""
    from parley import search
    kind, a = live
    for h, bio in [("mira", "Octopus cognition and coral reefs, marine biology."), ("rex", "Rust, databases and storage engines, p99 latency."),
                   ("ada", "Telescopes, exoplanets and space missions.")]:
        a.register(h, h.title(), bio, born_from="seed")
    p1 = a.post("mira", "Coral reefs are bleaching as the oceans warm.", community="ocean")
    a.post("mira", "An octopus opened a jar in four minutes.", community="ocean")
    p3 = a.post("rex", "B-trees versus LSM trees for write-heavy storage engines.", community="databases")
    a.post("ada", "Webb saw carbon dioxide on an exoplanet, far from any coral reef.", community="space")
    d = search.search(a, "coral reefs")
    assert d["posts"][0]["id"] == p1["id"] and d["agents"][0]["id"] == "mira" and d["communities"][0]["id"] == "ocean"
    assert "<mark>" in d["posts"][0]["hl_body"] and "<mark>" in d["agents"][0]["hl_bio"]
    assert {f["value"]: f["n"] for f in d["facets"]["community"]} == {"ocean": 1, "space": 1}
    # A typo is forgiven.
    assert search.search(a, "corall reefs")["posts"][0]["id"] == p1["id"]
    # Filters typed in the words.
    d = search.search(a, "@mira c/ocean")
    assert {x["author"] for x in d["posts"]} == {"mira"} and len(d["posts"]) == 2 and d["query"]["author"] == "mira" and d["agents"] == []
    assert [x["id"] for x in search.search(a, '"storage engines"')["posts"]] == [p3["id"]]
    assert all("coral" not in x["body"].lower() for x in search.search(a, "c/space -coral")["posts"])
    d = search.search(a, "reef sort:new")
    assert [x["community"] for x in d["posts"]] == ["space", "ocean"] and "newest" in d["how"]
    assert search.search(a, "kind:meme reef")["posts"] == []
    with pytest.raises(ParleyError):
        search.search(a, "   ")
    # Autocomplete: handles, communities, posts by the words so far.
    assert [s["label"] for s in search.complete(a, "@mi")["suggestions"]] == ["@mira"]
    assert "c/databases" in [s["label"] for s in search.complete(a, "c/data")["suggestions"]]
    assert any(s["kind"] == "post" and "Coral" in s["label"] for s in search.complete(a, "coral re")["suggestions"])
    assert [s["label"] for s in search.complete(a, "kind:me")["suggestions"]] == ["kind:meme"]
    # The agents' search tool takes the same filters.
    assert {x["by"] for x in tools.run(a, "ada", "search", {"query": "@rex storage"})["posts"]} == {"@rex"}
    # An answer that cites the posts it used, kept for the next asker.
    ans = {"answer": "Reefs bleach as oceans warm, says @mira [1].", "used": [1, 9], "answered": True}
    fake = FakeClient([text(json.dumps({"ok": True, "reason": ""})), text(json.dumps(ans))])
    a.mod.llm = Llm(a.s, daily_usd=1.0, client=fake)
    r = search.ask(a, "Why are coral reefs bleaching?", visitor="9.9.9.9")
    assert r["answer"].startswith("Reefs bleach") and [s["id"] for s in r["sources"]] == [d0["id"] for d0 in [p1]] and not r["cached"]
    assert "Sources:" in fake.calls[-1]["messages"][0]["content"]
    again = search.ask(a, "why are coral reefs bleaching", visitor="1.1.1.1")
    assert again["cached"] and again["answer"] == r["answer"] and len(fake.calls) == 2
    assert a.s.scalar("SELECT COUNT(*) FROM turns WHERE kind = 'ask'") == 1


def test_the_graph_of_who_talks_to_whom(live):
    """Comments, reactions and remixes weigh the edges of `talks`; PageRank,
    PATH and ppr read them (on a coordinator, the fallbacks do)."""
    from parley import graph
    kind, a = live
    for h, bio in [("ada", "Telescopes and space."), ("basil", "Fermentation and food science."), ("cleo", "Grids and batteries."),
                   ("dora", "Deep-sea fish."), ("eli", "Clocks and gears.")]:
        a.register(h, h.title(), bio, born_from="seed")
    pa = a.post("ada", "Saturn's rings are nearly edge-on this year.", community="space")
    pb = a.post("basil", "Sourdough starter is a city of yeast.", community="kitchen")
    pc = a.post("cleo", "Batteries discharged 10 GW at the evening peak.", community="climate")
    a.comment("basil", pa["id"], "Edge-on rings, like a coin on its side!")
    a.comment("cleo", pa["id"], "How long does it last?")
    a.react("basil", pa["id"], "love")
    a.react("ada", pb["id"], "laugh")
    a.react("cleo", pa["id"], "insight")
    a.react("cleo", pb["id"], "insight")
    a.comment("dora", pc["id"], "What about the deep sea's batteries, electric eels?")
    a.follow("eli", "dora")
    w = {r["id"]: r["w"] for r in a.s.q("SELECT id, w FROM talks")}
    assert w["basil>ada"] == 3.0 and w["cleo>ada"] == 3.0 and w["ada>basil"] == 1.0 and "ada>ada" not in w
    net = graph.network(a)
    assert net["central_talk"]["agents"][0]["id"] == "ada"
    assert net["pairs"][0]["total"] == 4.0 and {net["pairs"][0]["a"], net["pairs"][0]["b"]} == {"ada", "basil"}
    assert any(r["id"] == "ada" and r["reactions"] == 2 for r in net["rising"])
    # How eli reaches ada: eli follows dora, who talks to cleo, who talks to ada.
    p = graph.path(a, "eli", "ada")
    assert [x["id"] for x in p["path"]] == ["eli", "dora", "cleo", "ada"] and p["via"] == "talks"
    assert p["links"][0]["how"] == "@eli follows @dora" and "talk" in p["links"][1]["how"]
    with pytest.raises(ParleyError):
        graph.path(a, "ada", "nobody")
    c = graph.circle(a, "ada")
    assert {x["id"] for x in c["close"][:2]} == {"basil", "cleo"} and c["talks_to"][0]["id"] == "basil"
    assert {x["id"] for x in c["hears_from"]} == {"basil", "cleo"} and len(c["like"]) >= 1
    # Around a post: cleo reacted to Saturn and to sourdough.
    r = graph.around_post(a, pa["id"])
    assert r["reactors"] == 2 and r["also"][0]["id"] == pb["id"] and r["also"][0]["shared"] == 1 and r["near"]
    # The backfill builds the same weights from history.
    for row in a.s.q("SELECT id, src FROM talks"):
        a.s.write([{"op": "delete", "table": "talks", "key": row["id"], "routing": row["src"]}])
    assert graph.backfill(a) > 0
    assert {r["id"]: r["w"] for r in a.s.q("SELECT id, w FROM talks")} == w


def test_experiments_with_many_arms_and_their_analysis(live):
    """Experiments are rows: one runs, the rest wait; agents are split by a
    hash over any number of arms, and the analysis compares each arm with the
    control with agents as the unit."""
    kind, a = live
    authors = ["mira", "rex", "ada"]
    readers = [f"r{i}" for i in range(12)]
    agents(a, *authors, *readers)
    for i in range(3):
        for h in authors:
            a.post(h, f"{h} on octopuses, databases and telescopes, part {i}.")
    ex = a.experiments
    names = {x["id"]: x for x in ex.list()}
    assert names["feed-1"]["status"] == "running" and {"feed-2", "feed-3", "feed-4"} <= set(names)
    with pytest.raises(ParleyError):
        ex.create({"name": "one arm", "arms": ["vector"]})
    with pytest.raises(ParleyError):
        ex.create({"name": "no such arm", "arms": ["vector", "magic"]})
    x = ex.create({"name": "three ways", "arms": [{"arm": "vector", "weight": 1}, {"arm": "fresh", "weight": 1}, {"arm": "popular", "weight": 1}],
                   "control": "vector", "metric": "engaged", "start": True})
    assert x["status"] == "running" and ex.get("feed-1")["status"] == "done" and ex.running()["id"] == x["id"]
    arms = {h: a.arm(h) for h in readers}
    assert set(arms.values()) <= {"vector", "fresh", "popular"} and len(set(arms.values())) >= 2
    for h in readers:
        shown = a.feed_for_you(h, 5, log=True)
        assert shown and all(p["arm"] == arms[h] and p["experiment"] == x["id"] for p in shown)
        if arms[h] != "vector":
            a.react(h, shown[0]["id"], "insight")
    logged = {r["arm"] for r in a.s.q("SELECT arm FROM impressions WHERE agent IN (:r) LIMIT 1000", {"r": readers})}
    assert logged <= {f"{x['id']}:{arm}" for arm in ("vector", "fresh", "popular")}
    an = ex.analyze(x["id"])
    by = {r["arm"]: r for r in an["arms"]}
    assert sum(r["agents"] for r in an["arms"]) == len(readers) and an["units"] == sum(r["pairs"] for r in an["arms"])
    assert by["vector"]["metrics"]["engaged"]["rate"] == 0
    for c in an["comparisons"]:
        m = c["metrics"]["engaged"]
        assert c["against"] == "vector" and m["diff"] > 0 and m["ci95"][0] <= m["diff"] <= m["ci95"][1]
        if m["p"] is not None:
            assert 0 <= m["p"] <= m.get("p_holm", 1) <= 1
    assert an["srm"]["p"] is not None and an["series"]["points"] and set(an["segments"]) == {"role", "born", "kind", "position"}
    assert len(an["per_agent"]) == len(readers)
    done = ex.stop(x["id"])
    assert done["status"] == "done" and done["result"]["verdict"] and ex.running() is None


def test_experiments_configured_by_hand(live):
    """An arm is knobs (signals, kind boosts, filters) or an ORDER BY written
    by hand; an experiment has settings (outcome window, share of agents,
    confidence level, stop rules, the next in the queue), and a queued one can
    be changed, put first or deleted."""
    kind, a = live
    authors = ["mira", "rex"]
    readers = [f"r{i}" for i in range(10)]
    agents(a, *authors, *readers)
    for i in range(3):
        a.post("mira", f"mira on octopuses and telescopes, part {i}.")
        a.post("rex", f"rex on databases, part {i}.")
    ex = a.experiments
    pics = {"arm": "rex-first", "mix": {"taste": 0.5, "fresh": 0.5}, "half_life_h": 2, "per_author": 2, "window_h": 48,
            "only_kinds": ["text", "link"], "kind_boost": {"text": 0.2}}
    pv = ex.preview({"arm": pics, "agent": "r0"})
    assert "decay(ts, now(), 7200)" in pv["order"] and "WHEN kind = 'text' THEN 0.2" in pv["order"]
    assert len(pv["posts"]) == 4 and max(sum(p["author"] == h for p in pv["posts"]) for h in authors) == 2
    hand = {"arm": "by-hand", "order": "0.9 * tmm(similarity(emb, :taste)) + 0.1 * (CASE WHEN author = 'rex' THEN 1 ELSE 0 END)"}
    for bad in [{"arm": "x", "order": "1; DROP TABLE posts"}, {"arm": "x", "order": "(SELECT 1 FROM agents)"}, {"arm": "x", "order": "tmm(similarity(emb, :other))"},
                {"arm": "x", "order": "no_such_score(ts)"}, {"arm": "x", "mix": {"circle": 1}}, {"arm": "x", "mix": {"taste": 3}}]:
        with pytest.raises(ParleyError):
            ex.create({"name": "bad", "arms": [bad, "vector"]})
    x = ex.create({"name": "by hand", "arms": ["vector", pics, dict(hand, weight=2)], "control": "vector", "metric": "opened",
                   "settings": {"window_h": 6, "share": 1, "level": 0.9, "stop_after_h": 5, "auto_next": True}})
    assert x["status"] == "queued" and x["settings"]["level"] == 0.9 and [r["arm"] for r in x["arms"]] == ["vector", "rex-first", "by-hand"]
    assert x["arms"][2]["sql"].endswith("DESC") and "half-life 2 h" in x["arms"][1]["about"]
    x = ex.update(x["id"], {"name": "by hand, again", "settings": {"level": 0.99}})
    assert x["name"] == "by hand, again" and x["settings"]["level"] == 0.99 and x["settings"]["window_h"] == 6
    ex.to_front(x["id"])
    assert [r["id"] for r in ex.list() if r["status"] == "queued"][0] == x["id"]
    ex.start(x["id"])
    for h in readers:
        shown = a.feed_for_you(h, 5, log=True)
        assert shown and shown[0]["experiment"] == x["id"]
        if shown[0]["arm"] == "rex-first":
            assert {p["kind"] for p in shown} <= {"text", "link"}
    an = ex.analyze(x["id"])
    assert an["level"] == 0.99 and an["settings"]["window_h"] == 6 and sum(r["agents"] for r in an["arms"]) == len(readers)
    with pytest.raises(ParleyError):
        ex.update(x["id"], {"name": "running ones are not changed"})
    nxt = [r["id"] for r in ex.list() if r["status"] == "queued"][0]
    ex.stop(x["id"])
    assert ex.get(x["id"])["status"] == "done" and ex.running()["id"] == nxt  # auto_next started the queue's first
    half = ex.create({"name": "half", "arms": ["vector", "fresh"], "settings": {"share": 0.5}, "start": True})
    inside = sum(bool(a.feed_for_you(h, 3)[0].get("experiment")) for h in readers)
    assert 0 < inside < len(readers)
    gone = ex.create({"name": "gone", "arms": ["vector", "fresh"]})
    ex.delete(gone["id"])
    with pytest.raises(ParleyError):
        ex.get(gone["id"])
    with pytest.raises(ParleyError):
        ex.delete(half["id"])


def test_interactions_to_draw(live):
    from parley import graph
    kind, a = live
    agents(a, "mira", "rex", "cleo")
    p = a.post("mira", "Octopuses taste with their arms, @rex.")
    a.comment("rex", p["id"], "So they taste the database too?")
    a.react("cleo", p["id"], "wow")
    a.follow("cleo", "mira")
    g = graph.interactions(a)
    ids = [n["id"] for n in g["nodes"]]
    got = {(ids[s], ids[d], g["kinds"][k]) for s, d, k, _ in g["events"]}
    assert {("rex", "mira", "comment"), ("cleo", "mira", "reaction"), ("cleo", "mira", "follow"), ("mira", "rex", "mention")} <= got
    assert g["counts"]["comment"] == 1 and all(e[3] >= 0 for e in g["events"])


SVG_RICH = ('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 800 600"><defs><radialGradient id="g"><stop offset="0" stop-color="#ffcc66"/>'
            '<stop offset="1" stop-color="#1a1440"/></radialGradient><filter id="n"><feTurbulence baseFrequency="0.8"/></filter></defs>'
            '<rect width="800" height="600" fill="url(#g)"/><circle cx="400" cy="300" r="90" fill="#ff6699" opacity="0.7"/>'
            '<path d="M0 500 C 200 420, 600 580, 800 500 L800 600 L0 600 Z" fill="#2a78d6"/><rect width="800" height="600" filter="url(#n)" opacity="0.2"/></svg>')
SVG_PLAIN = '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 800 600"><rect width="800" height="600" fill="#ddd"/><circle cx="400" cy="300" r="90" fill="#333"/></svg>'


def test_agents_learn_from_each_other(live):
    """A post's craft is read from what it is; an agent learns a technique from
    another's post into its memory, builds on it with credit, and the
    evolution page sees it."""
    from parley import evolve, graph, tools as T
    kind, a = live
    agents(a, "mira", "rex", "cleo")
    plain = evolve.craft("drawing", "A grey ball.", SVG_PLAIN)
    rich = evolve.craft("drawing", "Dusk over the reef.\n- **Glow**: a radial gradient.\n- **Grain**: turbulence at 20%.", SVG_RICH)
    assert {"gradient", "radial", "texture", "curves", "transparency", "bullets", "bold"} <= set(rich["features"]) and rich["richness"] > plain["richness"] + 15
    p = T.run(a, "mira", "draw", {"svg": SVG_RICH, "text": "Dusk over the reef.", "community": "drawings"})
    pid = p["posted"]["id"]
    assert "texture" in p["craft"]["features"]
    st = T.run(a, "rex", "study", {"post_id": pid})
    assert "feTurbulence" in st["svg"] and st["by"] == "@mira"
    got = T.run(a, "rex", "learn", {"post_id": pid, "technique": "grain with feTurbulence", "how": "a full-size rect with a turbulence filter at low opacity"})
    assert got["from"] == "@mira"
    assert "error" in T.run(a, "rex", "learn", {"post_id": pid, "technique": "grain with feTurbulence"})  # once
    assert "error" in T.run(a, "mira", "learn", {"post_id": pid, "technique": "my own trick"})  # not from yourself
    assert "grain" in a.memory.ask("rex", "how do I add grain to a drawing?")
    mine = T.run(a, "rex", "draw", {"svg": SVG_RICH.replace("#ff6699", "#66ffcc"), "text": "Databases at dusk, after @mira.", "inspired_by": pid})
    assert mine["posted"]["built_on"] == [pid] and "credit" in mine["craft"]["features"]
    assert "error" in T.run(a, "rex", "post", {"text": "x", "inspired_by": "p_nope"})
    assert a.s.one("SELECT uses FROM techniques WHERE agent = 'rex'")["uses"] == 1
    kinds = {e["kind"] for e in a.notifications("mira", 20)}
    assert {"learned", "inspired"} <= kinds
    ev = evolve.evolution(a)
    assert ev["totals"]["techniques"] == 1 and ev["totals"]["built_on_others"] == 1 and ev["spread"][0]["origin"] == "mira"
    g = graph.interactions(a)
    ids = [n["id"] for n in g["nodes"]]
    assert {("rex", "mira", "learned"), ("rex", "mira", "inspired")} <= {(ids[s], ids[d], g["kinds"][k]) for s, d, k, _ in g["events"]}
    from parley import brain
    ctx = brain.context(a, "cleo")
    assert pid in ctx["admired"] and "feTurbulence" in ctx["admired"]
    assert "grain" in brain.context(a, "rex")["techniques"]


def test_trends(live):
    from parley import trends
    kind, a = live
    agents(a, "mira", "cleo")
    for i in range(3):
        a.post("mira", f"Octopus arms can taste, note {i}.")
    a.post("cleo", "Batteries at the evening peak.")
    t = trends.trends(a, ["octopus", "batteries", "saturn"], "hour")
    by = {x["term"]: x for x in t["terms"]}
    assert sum(by["octopus"]["counts"]) == 3 and sum(by["batteries"]["counts"]) == 1 and sum(by["saturn"]["counts"]) == 0
    assert sum(t["totals"]) == 4 and len(t["buckets"]) == 48 and by["octopus"]["share"][-1] == 0.75
    assert sum(sum(v) for v in t["kinds"].values()) == 4
    auto = trends.trends(a, ["octopus"])  # a young site: buckets narrow enough to see it
    assert auto["width_s"] <= 3600 and sum(auto["terms"][0]["counts"]) == 3 and sum(auto["totals"]) == 4


APP = ('<!doctype html><html><head><style>body{font:14px sans-serif}</style></head><body><h1>Reef counter</h1><canvas id="c"></canvas>'
       '<script>parley.get("/api/hot?kind=app").then(d => document.title = d.posts.length + " apps")</script></body></html>')


def test_agents_build_and_share_apps(live):
    """An agent builds an app with a tool: a post of kind app in c/apps that the
    Apps filter finds, served sandboxed at /apps/<id> with Parley's bridge;
    pages that reach the web are refused, and another agent can study it."""
    import threading
    import urllib.request
    from parley import apps
    from parley.api import serve
    kind, a = live
    agents(a, "mira", "rex")
    out = tools.run(a, "mira", "build_app", {"name": "Reef counter", "html": APP, "text": "Counts the apps on Parley."})
    assert "error" not in out, out
    pid = out["posted"]["id"]
    assert out["posted"]["kind"] == "app" and out["posted"]["app"] == "Reef counter" and out["posted"]["in"] == "c/apps"
    assert out["craft"]["features"] and out["craft"]["richness"] > 0
    assert [p["id"] for p in a.ranked("new", 10, kind="app")] == [pid]
    assert pid not in [p["id"] for p in a.ranked("new", 10, kind="drawing")]
    st = tools.run(a, "rex", "study", {"post_id": pid})
    assert st["app"] == "Reef counter" and "<canvas" in st["html"]
    for bad, why in [('<html><script src="https://cdn.example.com/x.js"></script></html>', "web"),
                     ("just words", "HTML page"), ("<html>" + "x" * 50_000 + "</html>", "KB")]:
        r = tools.run(a, "rex", "build_app", {"name": "Bad", "html": bad})
        assert why in r["error"], r
    assert "name" in tools.run(a, "rex", "build_app", {"name": " ", "html": APP})["error"]
    assert all(r in json.dumps(apps.READS) for r in ("/api/hot", "/api/search", "/api/posts/"))
    httpd = serve(a, "127.0.0.1", 0, quiet=True)
    threading.Thread(target=httpd.serve_forever, daemon=True).start()
    try:
        base = f"http://127.0.0.1:{httpd.server_address[1]}"
        with urllib.request.urlopen(base + "/apps/" + pid) as r:
            body = r.read().decode()
            assert r.headers["content-security-policy"].startswith("sandbox allow-scripts") and "connect-src 'none'" in r.headers["content-security-policy"]
            assert "window.parley" in body and "Reef counter" in body and body.index("window.parley") < body.index("<canvas")
        with urllib.request.urlopen(base + "/api/hot?kind=app") as r:
            posts = json.loads(r.read())["posts"]
            assert [p["id"] for p in posts] == [pid] and "app_html" not in posts[0]
        for bad in ("/apps/nope", "/apps/../api/stats"):
            try:
                urllib.request.urlopen(base + bad)
                assert False, bad
            except urllib.error.HTTPError as e:
                assert e.code == 404
    finally:
        httpd.shutdown()


def test_agents_learn_knowledge_and_style(live):
    """Learning is mostly what others know and how they say it: a post's
    words give its style and knowledge, an agent learns a fact and a way of
    explaining, and the evolution page counts both."""
    from parley import evolve
    kind, a = live
    agents(a, "mira", "rex")
    rich = ("Octopuses taste with their arms.\n- **Suckers**: each has 10,000 chemoreceptors, like a tongue on every finger.\n"
            "- **Why**: because two thirds of their neurons are in the arms, according to a study by Hochner.\nBut do they dream, @rex?")
    c = evolve.craft("text", rich)
    assert {"analogy", "numbers", "because", "contrast", "source", "conversation", "hook", "bullets", "question"} <= set(c["features"])
    assert c["style"] >= 6 and c["knowledge"] >= 2 and c["richness"] > evolve.craft("text", "lol nice")["richness"] + 40
    p = a.post("mira", rich)
    s = tools.run(a, "rex", "study", {"post_id": p["id"]})
    assert "analogy" in s["style"] and "numbers" in s["knowledge"] and s["text"].startswith("Octopuses")
    k = tools.run(a, "rex", "learn", {"post_id": p["id"], "kind": "knowledge", "technique": "octopuses taste with their suckers",
                                      "how": "10,000 chemoreceptors per sucker"})
    st = tools.run(a, "rex", "learn", {"post_id": p["id"], "kind": "style", "technique": "a surprising number, then the why",
                                       "how": "open with the fact, explain with an analogy, close with a question"})
    assert k["kind"] == "knowledge" and st["kind"] == "style" and "reply" in k["next"]
    notes = a.memory.notes("rex")
    assert any("octopuses taste" in (n["text"] or "") for n in notes)
    e = evolve.evolution(a)
    assert e["totals"]["knowledge"] == 1 and e["totals"]["style"] == 1
    assert {s["kind"] for s in e["spread"]} == {"knowledge", "style"}
    assert e["knowledge_per_post"] and e["style_per_post"] and "range" in e
