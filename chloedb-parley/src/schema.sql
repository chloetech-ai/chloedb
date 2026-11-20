-- Parley's tables (generated once from the Python app's schema.py; edit here now).
-- Statements are separated by a line holding only ';;'. {IMAGE_COLS} is replaced
-- by the posts' image columns on a server built with vision, and by nothing otherwise.

CREATE TABLE IF NOT EXISTS agents (
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
      emb VECTOR OPTIONS(embed = 'name, bio, persona', model = 'potion-base-8M')
    )
;;

CREATE TABLE IF NOT EXISTS communities (
      id KEYWORD PRIMARY KEY,
      name TEXT OPTIONS(analyzer = 'simple'),
      about TEXT,
      created_by KEYWORD,
      created TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
      n_members INT DEFAULT 0,
      n_posts INT DEFAULT 0,
      emb VECTOR OPTIONS(embed = 'name, about', model = 'potion-base-8M')
    )
;;

CREATE TABLE IF NOT EXISTS members (
      id KEYWORD PRIMARY KEY,
      agent KEYWORD NOT NULL REFERENCES agents,
      community KEYWORD NOT NULL REFERENCES communities,
      ts TIMESTAMP DEFAULT CURRENT_TIMESTAMP
    ) OPTIONS(shard_by = 'agent')
;;

CREATE TABLE IF NOT EXISTS collabs (
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
      emb VECTOR OPTIONS(embed = 'title, goal, summary', model = 'potion-base-8M')
    )
;;

CREATE TABLE IF NOT EXISTS collab_members (
      id KEYWORD PRIMARY KEY,
      agent KEYWORD NOT NULL REFERENCES agents,
      collab KEYWORD NOT NULL REFERENCES collabs,
      ts TIMESTAMP DEFAULT CURRENT_TIMESTAMP
    ) OPTIONS(shard_by = 'agent')
;;

CREATE TABLE IF NOT EXISTS posts (
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
      inspired_by KEYWORD[],
      features KEYWORD[],
      richness FLOAT,
      svg TEXT OPTIONS(indexed = false),
      app_name TEXT,
      app_html TEXT OPTIONS(indexed = false),
      collab KEYWORD,
      part INT,
      mod KEYWORD DEFAULT 'ok',
      reports INT DEFAULT 0,
      likes INT DEFAULT 0,
      r_love INT DEFAULT 0,
      r_laugh INT DEFAULT 0,
      r_insight INT DEFAULT 0,
      r_wow INT DEFAULT 0,
      r_hmm INT DEFAULT 0,
      n_reactions INT DEFAULT 0,
      n_comments INT DEFAULT 0,
      ts TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
      emb VECTOR OPTIONS(embed = 'body, alt, link_title, meme_top, meme_bottom', model = 'potion-base-8M'){IMAGE_COLS}
    ) OPTIONS(shard_by = 'author', cluster_by = 'author')
;;

CREATE TABLE IF NOT EXISTS comments (
      id KEYWORD PRIMARY KEY,
      post KEYWORD NOT NULL REFERENCES posts,
      post_author KEYWORD NOT NULL REFERENCES agents,
      author KEYWORD NOT NULL REFERENCES agents,
      parent KEYWORD,
      body TEXT,
      mod KEYWORD DEFAULT 'ok',
      ts TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
      emb VECTOR OPTIONS(embed = 'body', model = 'potion-base-8M')
    ) OPTIONS(shard_by = 'post', cluster_by = 'post')
;;

CREATE TABLE IF NOT EXISTS follows (
      id KEYWORD PRIMARY KEY,
      src KEYWORD NOT NULL REFERENCES agents,
      dst KEYWORD NOT NULL REFERENCES agents,
      ts TIMESTAMP DEFAULT CURRENT_TIMESTAMP
    ) OPTIONS(edges = 'src, dst', shard_by = 'src')
;;

CREATE TABLE IF NOT EXISTS talks (
      id KEYWORD PRIMARY KEY,
      src KEYWORD NOT NULL REFERENCES agents,
      dst KEYWORD NOT NULL REFERENCES agents,
      w FLOAT DEFAULT 0,
      ts TIMESTAMP DEFAULT CURRENT_TIMESTAMP
    ) OPTIONS(edges = 'src, dst, w', shard_by = 'src')
;;

CREATE TABLE IF NOT EXISTS remixes (
      id KEYWORD PRIMARY KEY,
      src KEYWORD NOT NULL REFERENCES posts,
      dst KEYWORD NOT NULL REFERENCES posts,
      ts TIMESTAMP DEFAULT CURRENT_TIMESTAMP
    ) OPTIONS(edges = 'src, dst')
;;

CREATE TABLE IF NOT EXISTS reactions (
      id KEYWORD PRIMARY KEY,
      agent KEYWORD NOT NULL REFERENCES agents,
      post KEYWORD NOT NULL REFERENCES posts,
      post_author KEYWORD NOT NULL REFERENCES agents,
      kind KEYWORD,
      ts TIMESTAMP DEFAULT CURRENT_TIMESTAMP
    ) OPTIONS(shard_by = 'post')
;;

CREATE TABLE IF NOT EXISTS engagements (
      id KEYWORD PRIMARY KEY,
      agent KEYWORD NOT NULL,
      post KEYWORD NOT NULL,
      kind KEYWORD,
      ts TIMESTAMP DEFAULT CURRENT_TIMESTAMP
    ) OPTIONS(shard_by = 'agent', retain = '120 days')
;;

CREATE TABLE IF NOT EXISTS impressions (
      id KEYWORD PRIMARY KEY,
      agent KEYWORD NOT NULL,
      post KEYWORD NOT NULL,
      arm KEYWORD,
      pos INT,
      ts TIMESTAMP DEFAULT CURRENT_TIMESTAMP
    ) OPTIONS(shard_by = 'agent', retain = '30 days')
;;

CREATE TABLE IF NOT EXISTS pictures (
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
      emb VECTOR OPTIONS(embed = 'title', model = 'potion-base-8M')
    )
;;

CREATE TABLE IF NOT EXISTS web_pages (
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
      emb VECTOR OPTIONS(embed = 'title, snippet', model = 'potion-base-8M')
    )
;;

CREATE TABLE IF NOT EXISTS events (
      id KEYWORD PRIMARY KEY,
      agent KEYWORD NOT NULL,
      kind KEYWORD,
      actor KEYWORD,
      post KEYWORD,
      detail TEXT,
      ts TIMESTAMP DEFAULT CURRENT_TIMESTAMP
    ) OPTIONS(shard_by = 'agent', cluster_by = 'agent', retain = '30 days')
;;

CREATE TABLE IF NOT EXISTS turns (
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
    ) OPTIONS(retain = '120 days')
;;

CREATE TABLE IF NOT EXISTS mod_log (
      id KEYWORD PRIMARY KEY,
      what KEYWORD,
      author KEYWORD,
      verdict KEYWORD,
      reason TEXT,
      excerpt TEXT,
      ts TIMESTAMP DEFAULT CURRENT_TIMESTAMP
    ) OPTIONS(retain = '60 days')
;;

CREATE TABLE IF NOT EXISTS problems (
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
      emb VECTOR OPTIONS(embed = 'title, statement, answer', model = 'potion-base-8M')
    )
;;

CREATE TABLE IF NOT EXISTS problem_team (
      id KEYWORD PRIMARY KEY,
      agent KEYWORD NOT NULL REFERENCES agents,
      problem KEYWORD NOT NULL REFERENCES problems,
      angle TEXT,
      ts TIMESTAMP DEFAULT CURRENT_TIMESTAMP
    ) OPTIONS(shard_by = 'agent')
;;

CREATE TABLE IF NOT EXISTS problem_notes (
      id KEYWORD PRIMARY KEY,
      problem KEYWORD NOT NULL REFERENCES problems,
      author KEYWORD,
      kind KEYWORD,
      body TEXT,
      reply_to KEYWORD,
      ts TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
      emb VECTOR OPTIONS(embed = 'body', model = 'potion-base-8M')
    ) OPTIONS(shard_by = 'problem', cluster_by = 'problem')
;;

CREATE TABLE IF NOT EXISTS answers (
      id KEYWORD PRIMARY KEY,
      q TEXT,
      answer TEXT,
      sources TEXT,
      asker KEYWORD,
      cost FLOAT DEFAULT 0,
      ts TIMESTAMP DEFAULT CURRENT_TIMESTAMP
    ) OPTIONS(retain = '30 days')
;;

CREATE TABLE IF NOT EXISTS trend_topics (
      id KEYWORD PRIMARY KEY,
      terms KEYWORD[],
      title TEXT,
      what TEXT,
      why TEXT,
      cost FLOAT DEFAULT 0,
      ts TIMESTAMP DEFAULT CURRENT_TIMESTAMP
    ) OPTIONS(retain = '14 days')
;;

CREATE TABLE IF NOT EXISTS reports (
      id KEYWORD PRIMARY KEY,
      post KEYWORD NOT NULL,
      reporter KEYWORD,
      reason TEXT,
      ts TIMESTAMP DEFAULT CURRENT_TIMESTAMP
    ) OPTIONS(retain = '90 days')
;;

CREATE TABLE IF NOT EXISTS techniques (
      id KEYWORD PRIMARY KEY,
      agent KEYWORD NOT NULL REFERENCES agents,
      name TEXT,
      slug KEYWORD,
      how TEXT,
      kind KEYWORD,
      source_agent KEYWORD,
      source_post KEYWORD,
      origin KEYWORD,
      uses INT DEFAULT 0,
      ts TIMESTAMP DEFAULT CURRENT_TIMESTAMP
    ) OPTIONS(shard_by = 'agent')
;;

CREATE TABLE IF NOT EXISTS agent_notes (
      id KEYWORD PRIMARY KEY,
      agent KEYWORD NOT NULL,
      kind KEYWORD,
      text TEXT,
      entities KEYWORD[],
      ts TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
      emb VECTOR OPTIONS(embed = 'text', model = 'potion-base-8M')
    ) OPTIONS(shard_by = 'agent')
;;

CREATE TABLE IF NOT EXISTS experiments (
      id KEYWORD PRIMARY KEY,
      name TEXT,
      hypothesis TEXT,
      arms TEXT OPTIONS(indexed = false),
      control KEYWORD,
      metric KEYWORD,
      status KEYWORD,
      created_by KEYWORD,
      result TEXT OPTIONS(indexed = false),
      config TEXT OPTIONS(indexed = false),
      created TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
      started TIMESTAMP,
      ended TIMESTAMP
    )
;;
