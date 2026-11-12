package ai.chloetech.chloedb;

import java.util.LinkedHashMap;
import java.util.Map;

/**
 * The limits of one query ({@link Database#query(String, Map, QueryOptions)},
 * {@link Database#execute(String, Map, QueryOptions)},
 * {@link SearchOptions#limits(QueryOptions)}), over the database's own
 * ({@link Database#setLimits(Long, Long)}).
 *
 * <pre>{@code
 * db.query("SELECT genres, COUNT(*) FROM books GROUP BY genres", null, new QueryOptions().timeoutMs(200));
 * db.query("SELECT * FROM books", null, new QueryOptions().memory(0));   // counts nothing
 * }</pre>
 */
public final class QueryOptions {
    private Long timeoutMs;
    private Long memory;
    private Long tag;

    /**
     * Its deadline, in ms: past it the query stops and throws a
     * {@link ChloeDBException} whose message starts {@code timeout:} (0: the
     * database's).
     */
    public QueryOptions timeoutMs(long timeoutMs) {
        this.timeoutMs = whole("timeoutMs", timeoutMs);
        return this;
    }

    /**
     * A memory budget of its own, in bytes, past which it throws a
     * {@link ChloeDBException} whose message starts {@code memory:} (0:
     * none). Unless set, the database's.
     */
    public QueryOptions memory(long memory) {
        this.memory = whole("memory", memory);
        return this;
    }

    /**
     * A tag that {@link Database#interrupt(long)} stops this query by,
     * leaving the others running (a JDBC statement's {@code cancel}).
     */
    public QueryOptions tag(long tag) {
        this.tag = whole("tag", tag);
        return this;
    }

    static long whole(String what, long n) {
        if (n < 0) {
            throw new IllegalArgumentException(what + ": 0 or more");
        }
        return n;
    }

    Map<String, Object> args() {
        Map<String, Object> m = new LinkedHashMap<>();
        if (timeoutMs != null) {
            m.put("timeoutMs", timeoutMs);
        }
        if (memory != null) {
            m.put("memory", memory);
        }
        if (tag != null) {
            m.put("tag", tag);
        }
        return m;
    }
}
