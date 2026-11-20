CREATE RECOMMENDER taste ON engagements(agent, post, ts) ITEMS posts(emb) OPTIONS (window = 5, history = 40)
