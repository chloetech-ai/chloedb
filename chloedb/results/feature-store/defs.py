from datetime import timedelta
from feast import Entity, FeatureView, Field, FileSource
from feast.types import Float64
D = "/tmp/claude-0/fs/data"
user = Entity(name="user", join_keys=["u_id"])
item = Entity(name="item", join_keys=["i_id"])
users = FeatureView(name="user_stats", entities=[user], ttl=timedelta(days=365),
    schema=[Field(name=c, dtype=Float64) for c in ["clicks_7d", "buys_30d", "avg_price", "sessions_7d", "dwell_s"]],
    source=FileSource(path=f"{D}/users_us.parquet", timestamp_field="event_timestamp"))
items = FeatureView(name="item_stats", entities=[item], ttl=timedelta(days=365),
    schema=[Field(name=c, dtype=Float64) for c in ["ctr_7d", "views_1d", "price", "stock", "rating"]],
    source=FileSource(path=f"{D}/items_us.parquet", timestamp_field="event_timestamp"))
