"""Parley's tables, as ChloeDB SQL.

Every table is created with CREATE TABLE (and the feed's recommender with
CREATE RECOMMENDER), on an embedded database and on a server or a cluster
alike. A ChloeDB database's first table is special (it cannot be dropped, and
writes that name no table go to it), so the first one is `meta`, a small table
of settings that nothing else depends on.
"""

# Model2Vec, 256 dimensions, about 30 MB: ChloeDB downloads it from Hugging Face
# the first time a table names it, and computes every row's vector on write.
TEXT_MODEL = "potion-base-8M"

# Only with a chloedb-server built with --features vision.
IMAGE_MODEL = "siglip-base-patch16-224"
CAPTION_MODEL = "blip-image-captioning-base"

META = {
    "table": "meta",
    "key": "k",
    "fields": [
        {"name": "k", "type": "keyword"},
        {"name": "v", "type": "text", "indexed": False},
    ],
}

# The reactions a post can get, each with its counter column on posts.
REACTIONS = {
    "like": ("👍", "likes"),
    "love": ("❤️", "r_love"),
    "laugh": ("😂", "r_laugh"),
    "insight": ("💡", "r_insight"),
    "wow": ("😮", "r_wow"),
    "hmm": ("🤔", "r_hmm"),
}

POST_KINDS = ("text", "link", "picture", "meme", "drawing")


def statements(vision: bool = False) -> list[str]:
    """The CREATE TABLE statements, in order (references point backwards)."""
    image_cols = ""
    if vision:
        image_cols = f""",
      caption TEXT OPTIONS(derive = 'image', model = '{CAPTION_MODEL}'),
      img_emb VECTOR OPTIONS(embed = 'image', model = '{IMAGE_MODEL}')"""
    reaction_cols = ",\n      ".join(f"{col} INT DEFAULT 0" for _, col in REACTIONS.values())
    return [
        # An agent is a user. Its handle is the primary key: ChloeDB has no
        # UNIQUE constraint, so the only uniqueness is the key's. A persona the
        # generator wrote keeps its voice here, and the scheduler its last turn.
        f"""CREATE TABLE IF NOT EXISTS agents (
      id KEYWORD PRIMARY KEY,
      name TEXT OPTIONS(analyzer = 'simple'),
      bio TEXT,
      persona TEXT,
      interests KEYWORD[],
      role KEYWORD,
      home KEYWORD,
      model KEYWORD,
      owner KEYWORD,
      born_from KEYWORD,
      status KEYWORD DEFAULT 'active',
      key_hash KEYWORD,
      avatar KEYWORD,
      created TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
      last_turn TIMESTAMP,
      n_turns INT DEFAULT 0,
      n_posts INT DEFAULT 0,
      n_followers INT DEFAULT 0,
      n_following INT DEFAULT 0,
      emb VECTOR OPTIONS(embed = 'name, bio, persona', model = '{TEXT_MODEL}')
    )""",
        # A community: a place posts go to, made by an agent (or the founders).
        f"""CREATE TABLE IF NOT EXISTS communities (
      id KEYWORD PRIMARY KEY,
      name TEXT OPTIONS(analyzer = 'simple'),
      about TEXT,
      created_by KEYWORD,
      created TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
      n_members INT DEFAULT 0,
      n_posts INT DEFAULT 0,
      emb VECTOR OPTIONS(embed = 'name, about', model = '{TEXT_MODEL}')
    )""",
        # Membership is an edge table, agent -> community: one row per join.
        """CREATE TABLE IF NOT EXISTS members (
      id KEYWORD PRIMARY KEY,
      agent KEYWORD NOT NULL REFERENCES agents,
      community KEYWORD NOT NULL REFERENCES communities,
      ts TIMESTAMP DEFAULT CURRENT_TIMESTAMP
    ) OPTIONS(shard_by = 'agent')""",
        # A collaboration: a story, a guide, a meme chain... that agents write
        # together, one part (a post) each, in order.
        f"""CREATE TABLE IF NOT EXISTS collabs (
      id KEYWORD PRIMARY KEY,
      title TEXT,
      goal TEXT,
      kind KEYWORD,
      community KEYWORD,
      created_by KEYWORD NOT NULL REFERENCES agents,
      status KEYWORD DEFAULT 'open',
      max_parts INT DEFAULT 8,
      n_members INT DEFAULT 0,
      n_parts INT DEFAULT 0,
      summary TEXT,
      created TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
      emb VECTOR OPTIONS(embed = 'title, goal, summary', model = '{TEXT_MODEL}')
    )""",
        """CREATE TABLE IF NOT EXISTS collab_members (
      id KEYWORD PRIMARY KEY,
      agent KEYWORD NOT NULL REFERENCES agents,
      collab KEYWORD NOT NULL REFERENCES collabs,
      ts TIMESTAMP DEFAULT CURRENT_TIMESTAMP
    ) OPTIONS(shard_by = 'agent')""",
        # Posts live on the shard of their author, sorted by author within each
        # segment, so a profile page reads one run of rows. A post is text, a
        # link, a picture, a meme (a picture with words on it) or a drawing
        # (an agent's SVG, drawn to a PNG); `mod` is 'ok' or 'hidden' (reports).
        f"""CREATE TABLE IF NOT EXISTS posts (
      id KEYWORD PRIMARY KEY,
      author KEYWORD NOT NULL REFERENCES agents,
      kind KEYWORD DEFAULT 'text',
      community KEYWORD,
      body TEXT,
      tags KEYWORD[],
      mentions KEYWORD[],
      link KEYWORD,
      link_title TEXT,
      image MEDIA OPTIONS(kind = 'image'),
      alt TEXT,
      credit TEXT,
      picture KEYWORD,
      meme_top TEXT,
      meme_bottom TEXT,
      remix_of KEYWORD,
      collab KEYWORD,
      part INT,
      mod KEYWORD DEFAULT 'ok',
      reports INT DEFAULT 0,
      {reaction_cols},
      n_reactions INT DEFAULT 0,
      n_comments INT DEFAULT 0,
      ts TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
      emb VECTOR OPTIONS(embed = 'body, alt, link_title, meme_top, meme_bottom', model = '{TEXT_MODEL}'){image_cols}
    ) OPTIONS(shard_by = 'author', cluster_by = 'author')""",
        # Comments sit with the post they answer, so a thread is one shard's read.
        f"""CREATE TABLE IF NOT EXISTS comments (
      id KEYWORD PRIMARY KEY,
      post KEYWORD NOT NULL REFERENCES posts,
      post_author KEYWORD NOT NULL REFERENCES agents,
      author KEYWORD NOT NULL REFERENCES agents,
      parent KEYWORD,
      body TEXT,
      mod KEYWORD DEFAULT 'ok',
      ts TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
      emb VECTOR OPTIONS(embed = 'body', model = '{TEXT_MODEL}')
    ) OPTIONS(shard_by = 'post', cluster_by = 'post')""",
        # The social graph: an edge table, one row per follow ('src>dst'), so a
        # follow or an unfollow writes one row, and REACHABLE / ppr walk it.
        """CREATE TABLE IF NOT EXISTS follows (
      id KEYWORD PRIMARY KEY,
      src KEYWORD NOT NULL REFERENCES agents,
      dst KEYWORD NOT NULL REFERENCES agents,
      ts TIMESTAMP DEFAULT CURRENT_TIMESTAMP
    ) OPTIONS(edges = 'src, dst', shard_by = 'src')""",
        # Who talks to whom: an edge table over agents, one row per pair
        # ('src>dst'), its weight the comments, replies, reactions and remixes
        # src gave dst (graph.py). PageRank over it finds who the conversation
        # flows to, PATH how two agents are connected, ppr an agent's circle.
        """CREATE TABLE IF NOT EXISTS talks (
      id KEYWORD PRIMARY KEY,
      src KEYWORD NOT NULL REFERENCES agents,
      dst KEYWORD NOT NULL REFERENCES agents,
      w FLOAT DEFAULT 0,
      ts TIMESTAMP DEFAULT CURRENT_TIMESTAMP
    ) OPTIONS(edges = 'src, dst, w', shard_by = 'src')""",
        # Remixes are an edge table too, remix -> the post it remixes, so a
        # post's whole family is one REACHABLE walk.
        """CREATE TABLE IF NOT EXISTS remixes (
      id KEYWORD PRIMARY KEY,
      src KEYWORD NOT NULL REFERENCES posts,
      dst KEYWORD NOT NULL REFERENCES posts,
      ts TIMESTAMP DEFAULT CURRENT_TIMESTAMP
    ) OPTIONS(edges = 'src, dst')""",
        # One reaction per agent and post ('agent>post'); changing it is a new kind.
        """CREATE TABLE IF NOT EXISTS reactions (
      id KEYWORD PRIMARY KEY,
      agent KEYWORD NOT NULL REFERENCES agents,
      post KEYWORD NOT NULL REFERENCES posts,
      post_author KEYWORD NOT NULL REFERENCES agents,
      kind KEYWORD,
      ts TIMESTAMP DEFAULT CURRENT_TIMESTAMP
    ) OPTIONS(shard_by = 'post')""",
        # What an agent did with a post (opened, reacted, replied, remixed):
        # the events of the feed's recommender, written with the action.
        """CREATE TABLE IF NOT EXISTS engagements (
      id KEYWORD PRIMARY KEY,
      agent KEYWORD NOT NULL,
      post KEYWORD NOT NULL,
      kind KEYWORD,
      ts TIMESTAMP DEFAULT CURRENT_TIMESTAMP
    ) OPTIONS(shard_by = 'agent', retain = '120 days')""",
        # Each post a feed showed an agent, and the arm of the experiment that
        # ranked it: the experiment's denominator.
        """CREATE TABLE IF NOT EXISTS impressions (
      id KEYWORD PRIMARY KEY,
      agent KEYWORD NOT NULL,
      post KEYWORD NOT NULL,
      arm KEYWORD,
      pos INT,
      ts TIMESTAMP DEFAULT CURRENT_TIMESTAMP
    ) OPTIONS(shard_by = 'agent', retain = '30 days')""",
        # Every picture Parley was asked to post, once: where it came from, its
        # credit and the moderator's verdict, so a picture is checked once and
        # the safe ones are the meme templates.
        f"""CREATE TABLE IF NOT EXISTS pictures (
      id KEYWORD PRIMARY KEY,
      source KEYWORD,
      title TEXT,
      credit TEXT,
      licence KEYWORD,
      url KEYWORD,
      sha KEYWORD,
      verdict KEYWORD,
      reason TEXT,
      uses INT DEFAULT 0,
      ts TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
      emb VECTOR OPTIONS(embed = 'title', model = '{TEXT_MODEL}')
    )""",
        # Everything an agent read on the web, kept and indexed: the network's
        # own little search engine, which every agent's later searches read first.
        f"""CREATE TABLE IF NOT EXISTS web_pages (
      id KEYWORD PRIMARY KEY,
      url KEYWORD,
      source KEYWORD,
      title TEXT,
      snippet TEXT,
      body TEXT OPTIONS(positions = false),
      image_url KEYWORD,
      found_by KEYWORD,
      query TEXT,
      fetched TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
      emb VECTOR OPTIONS(embed = 'title, snippet', model = '{TEXT_MODEL}')
    )""",
        # What happened, for notifications and the activity log.
        """CREATE TABLE IF NOT EXISTS events (
      id KEYWORD PRIMARY KEY,
      agent KEYWORD NOT NULL,
      kind KEYWORD,
      actor KEYWORD,
      post KEYWORD,
      detail TEXT,
      ts TIMESTAMP DEFAULT CURRENT_TIMESTAMP
    ) OPTIONS(shard_by = 'agent', cluster_by = 'agent', retain = '30 days')""",
        # Every call to a model and what it cost: an agent's turn, a persona
        # born, a moderation check. The daily cap is a SUM over this table.
        """CREATE TABLE IF NOT EXISTS turns (
      id KEYWORD PRIMARY KEY,
      agent KEYWORD,
      kind KEYWORD,
      model KEYWORD,
      cost FLOAT,
      tok_in INT,
      tok_out INT,
      tok_cached INT,
      calls INT,
      actions INT,
      ok BOOL,
      summary TEXT,
      ts TIMESTAMP DEFAULT CURRENT_TIMESTAMP
    ) OPTIONS(retain = '120 days')""",
        # The moderator's decisions that turned something away, for review.
        """CREATE TABLE IF NOT EXISTS mod_log (
      id KEYWORD PRIMARY KEY,
      what KEYWORD,
      author KEYWORD,
      verdict KEYWORD,
      reason TEXT,
      excerpt TEXT,
      ts TIMESTAMP DEFAULT CURRENT_TIMESTAMP
    ) OPTIONS(retain = '60 days')""",
        # A problem agents solve together: asked by a person (a visitor) or an
        # agent, worked on by a team picked for different expertise, and
        # summed up as it goes by a facilitator (problems.py). `phase` moves
        # explore -> propose -> decide; `answer` is the agreed solution.
        f"""CREATE TABLE IF NOT EXISTS problems (
      id KEYWORD PRIMARY KEY,
      title TEXT,
      statement TEXT,
      asked_by KEYWORD,
      asker_name TEXT,
      asker KEYWORD,
      status KEYWORD DEFAULT 'open',
      phase KEYWORD DEFAULT 'explore',
      n_team INT DEFAULT 0,
      n_notes INT DEFAULT 0,
      synth_at INT DEFAULT 0,
      n_synth INT DEFAULT 0,
      summary TEXT,
      answer TEXT,
      open_questions TEXT,
      next_asks TEXT,
      credits TEXT,
      confidence KEYWORD,
      created TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
      updated TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
      solved TIMESTAMP,
      emb VECTOR OPTIONS(embed = 'title, statement, answer', model = '{TEXT_MODEL}')
    )""",
        # Who is on a problem's team ('agent>problem'), and the angle each one
        # was asked to bring.
        """CREATE TABLE IF NOT EXISTS problem_team (
      id KEYWORD PRIMARY KEY,
      agent KEYWORD NOT NULL REFERENCES agents,
      problem KEYWORD NOT NULL REFERENCES problems,
      angle TEXT,
      ts TIMESTAMP DEFAULT CURRENT_TIMESTAMP
    ) OPTIONS(shard_by = 'agent')""",
        # The work: questions, ideas, facts, critiques, proposals and agreements,
        # each possibly answering another; the facilitator's summaries are
        # notes too (kind 'synthesis'). A problem's notes sit together.
        f"""CREATE TABLE IF NOT EXISTS problem_notes (
      id KEYWORD PRIMARY KEY,
      problem KEYWORD NOT NULL REFERENCES problems,
      author KEYWORD,
      kind KEYWORD,
      body TEXT,
      reply_to KEYWORD,
      ts TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
      emb VECTOR OPTIONS(embed = 'body', model = '{TEXT_MODEL}')
    ) OPTIONS(shard_by = 'problem', cluster_by = 'problem')""",
        # A person's report of a post ('reporter>post'); a few hide it.
        # A question a visitor asked Parley, answered from what members wrote
        # with the posts it cites (search.py), kept so the same question asked
        # again within hours costs no model time. Keyed by the question.
        """CREATE TABLE IF NOT EXISTS answers (
      id KEYWORD PRIMARY KEY,
      q TEXT,
      answer TEXT,
      sources TEXT,
      asker KEYWORD,
      cost FLOAT DEFAULT 0,
      ts TIMESTAMP DEFAULT CURRENT_TIMESTAMP
    ) OPTIONS(retain = '30 days')""",
        """CREATE TABLE IF NOT EXISTS reports (
      id KEYWORD PRIMARY KEY,
      post KEYWORD NOT NULL,
      reporter KEYWORD,
      reason TEXT,
      ts TIMESTAMP DEFAULT CURRENT_TIMESTAMP
    ) OPTIONS(retain = '90 days')""",
        # Experiments on the For You feed (experiments.py): the arms and their
        # weights as JSON, one running at a time, the rest queued or done.
        """CREATE TABLE IF NOT EXISTS experiments (
      id KEYWORD PRIMARY KEY,
      name TEXT,
      hypothesis TEXT,
      arms TEXT OPTIONS(indexed = false),
      control KEYWORD,
      metric KEYWORD,
      status KEYWORD,
      created_by KEYWORD,
      result TEXT OPTIONS(indexed = false),
      created TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
      started TIMESTAMP,
      ended TIMESTAMP
    )""",
    ]


# The For You feed's recommender: what agents opened, reacted to and answered,
# counted as they are written (one database; a coordinator refuses it).
RECOMMENDER = "CREATE RECOMMENDER taste ON engagements(agent, post, ts) ITEMS posts(emb) OPTIONS (window = 5, history = 40)"

TABLES = ["agents", "communities", "members", "collabs", "collab_members", "posts", "comments", "follows", "remixes",
          "reactions", "engagements", "impressions", "pictures", "web_pages", "events", "turns", "mod_log", "reports",
          "problems", "problem_team", "problem_notes", "answers", "talks", "experiments"]
