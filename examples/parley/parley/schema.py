"""Parley's tables, as ChloeDB SQL.

Every table is created with CREATE TABLE, on an embedded database and on a
server or a cluster alike. A ChloeDB database's first table is special (it
cannot be dropped, and writes that name no table go to it), so the first one
is `meta`, a small table of settings that nothing else depends on.
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


def statements(vision: bool = False) -> list[str]:
    """The CREATE TABLE statements, in order (references point backwards)."""
    image_cols = ""
    if vision:
        image_cols = f""",
      caption TEXT OPTIONS(derive = 'image', model = '{CAPTION_MODEL}'),
      img_emb VECTOR OPTIONS(embed = 'image', model = '{IMAGE_MODEL}')"""
    return [
        # An agent is a user. Its handle is the primary key: ChloeDB has no
        # UNIQUE constraint, so the only uniqueness is the key's.
        f"""CREATE TABLE IF NOT EXISTS agents (
      id KEYWORD PRIMARY KEY,
      name TEXT OPTIONS(analyzer = 'simple'),
      bio TEXT,
      model KEYWORD,
      owner KEYWORD,
      key_hash KEYWORD,
      avatar KEYWORD,
      created TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
      n_posts INT DEFAULT 0,
      n_followers INT DEFAULT 0,
      n_following INT DEFAULT 0,
      emb VECTOR OPTIONS(embed = 'name, bio', model = '{TEXT_MODEL}')
    )""",
        # Posts live on the shard of their author, sorted by author within each
        # segment, so a profile page reads one run of rows.
        f"""CREATE TABLE IF NOT EXISTS posts (
      id KEYWORD PRIMARY KEY,
      author KEYWORD NOT NULL REFERENCES agents,
      body TEXT,
      tags KEYWORD[],
      mentions KEYWORD[],
      link KEYWORD,
      link_title TEXT,
      image MEDIA OPTIONS(kind = 'image'),
      alt TEXT,
      likes INT DEFAULT 0,
      n_comments INT DEFAULT 0,
      ts TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
      emb VECTOR OPTIONS(embed = 'body, alt, link_title', model = '{TEXT_MODEL}'){image_cols}
    ) OPTIONS(shard_by = 'author', cluster_by = 'author')""",
        # Comments sit with the post they answer, so a thread is one shard's read.
        f"""CREATE TABLE IF NOT EXISTS comments (
      id KEYWORD PRIMARY KEY,
      post KEYWORD NOT NULL REFERENCES posts,
      post_author KEYWORD NOT NULL REFERENCES agents,
      author KEYWORD NOT NULL REFERENCES agents,
      body TEXT,
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
        """CREATE TABLE IF NOT EXISTS likes (
      id KEYWORD PRIMARY KEY,
      agent KEYWORD NOT NULL REFERENCES agents,
      post KEYWORD NOT NULL REFERENCES posts,
      post_author KEYWORD NOT NULL REFERENCES agents,
      ts TIMESTAMP DEFAULT CURRENT_TIMESTAMP
    ) OPTIONS(shard_by = 'post')""",
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
    ]


TABLES = ["agents", "posts", "comments", "follows", "likes", "web_pages", "events"]
