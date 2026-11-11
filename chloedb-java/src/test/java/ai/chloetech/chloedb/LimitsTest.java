package ai.chloetech.chloedb;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.nio.file.Path;
import java.sql.Connection;
import java.sql.DriverManager;
import java.sql.SQLException;
import java.sql.SQLTimeoutException;
import java.sql.Statement;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.Future;
import java.util.concurrent.TimeUnit;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

/** A database's limits on its queries: memory budgets, deadlines, interrupting (chloedb-py/tests/test_limits.py, in Java). */
class LimitsTest {
    static Map<String, Object> map(Object... kv) {
        Map<String, Object> m = new LinkedHashMap<>();
        for (int i = 0; i < kv.length; i += 2) {
            m.put((String) kv[i], kv[i + 1]);
        }
        return m;
    }

    static final Map<String, Object> SCHEMA = map(
            "table", "t",
            "key", "id",
            "fields", Arrays.asList(
                    map("name", "id", "type", "int"),
                    map("name", "a", "type", "int"),
                    map("name", "c", "type", "int"),
                    map("name", "note", "type", "text")));

    /** A group per row: a few MiB. */
    static final String HEAVY = "SELECT a, COUNT(*) FROM t GROUP BY a LIMIT 5";

    /**
     * Every row meets the 20,000 rows of its {@code c} and none passes the
     * rest of the condition: seconds of comparisons, on DataFusion or the
     * operators, holding next to nothing.
     */
    static final String SLOW = "SELECT x.id, y.id FROM t x JOIN t y ON x.c = y.c AND x.a < y.a - 1000000";

    @TempDir
    Path dir;

    Database made(int n, Database.Options options) {
        Database db = Database.create(dir.resolve("t.strata"), SCHEMA, options);
        List<Map<String, Object>> rows = new ArrayList<>();
        for (int i = 0; i < n; i++) {
            rows.add(map("id", (long) i, "a", (long) i * 7919 % n, "c", (long) i % 3, "note", String.format("%08d ", i).repeat(20)));
        }
        db.insert(rows);
        return db;
    }

    @Test
    void queriesShareTheProcessBudgetAndAQueryPastItsOwnFails() {
        try (Database db = made(50_000, new Database.Options())) {
            assertNotNull(db.memory());
            assertTrue(db.memory() >= 1 << 20);
            assertNull(db.timeoutMs());
            assertEquals(5, db.query(HEAVY).size());
            ChloeDBException e = assertThrows(ChloeDBException.class, () -> db.query(HEAVY, null, new QueryOptions().memory(256 << 10)));
            assertTrue(e.getMessage().startsWith("memory:") && e.getMessage().contains("this query may hold (its memory option)"), e.getMessage());
            assertThrows(ChloeDBException.class, () -> db.execute(HEAVY, null, new QueryOptions().memory(256 << 10)));
            assertThrows(ChloeDBException.class, () -> db.table().search("00000001", new SearchOptions().limits(new QueryOptions().memory(1))));
            assertEquals(5, db.query(HEAVY, null, new QueryOptions().memory(0)).size());
            assertThrows(IllegalArgumentException.class, () -> new QueryOptions().memory(-1));
        }
    }

    @Test
    void aDatabaseBudgetItsQueriesShare() {
        try (Database db = made(50_000, new Database.Options().memory(256 << 10).timeoutMs(60_000))) {
            assertEquals(Long.valueOf(256L << 10), db.memory());
            assertEquals(Long.valueOf(60_000L), db.timeoutMs());
            ChloeDBException e = assertThrows(ChloeDBException.class, () -> db.query(HEAVY));
            assertTrue(e.getMessage().contains("this database's queries may hold"), e.getMessage());
            assertEquals(5, db.query(HEAVY, null, new QueryOptions().memory(0)).size());
            db.setLimits(null, 0L);
            assertNull(db.memory());
            assertNull(db.timeoutMs());
            assertEquals(5, db.query(HEAVY).size());
        }
    }

    @Test
    void aDeadlineStopsASlowQuery() {
        try (Database db = made(60_000, new Database.Options().memory(0))) {
            long t0 = System.nanoTime();
            ChloeDBException e = assertThrows(ChloeDBException.class, () -> db.query(SLOW, null, new QueryOptions().timeoutMs(200)));
            assertTrue(e.getMessage().startsWith("timeout:") && e.getMessage().contains("timeoutMs=200"), e.getMessage());
            assertTrue(System.nanoTime() - t0 < TimeUnit.SECONDS.toNanos(10));
            db.setLimits(200L, 0L);
            e = assertThrows(ChloeDBException.class, () -> db.execute(SLOW, null));
            assertTrue(e.getMessage().contains("timeoutMs=200"), e.getMessage());
            assertEquals(1, db.query("SELECT COUNT(*) FROM t").size());
        }
    }

    @Test
    void interruptStopsTheQueriesRunning() throws Exception {
        try (Database db = made(60_000, new Database.Options().memory(0))) {
            ExecutorService pool = Executors.newFixedThreadPool(2);
            try {
                Future<Throwable> tagged = pool.submit(() -> failure(() -> db.query(SLOW, null, new QueryOptions().tag(7))));
                Future<Throwable> other = pool.submit(() -> failure(() -> db.query(SLOW)));
                int n = 0;
                long t0 = System.nanoTime();
                // The tagged one alone, then the rest.
                while (n < 1 && System.nanoTime() - t0 < TimeUnit.SECONDS.toNanos(10)) {
                    Thread.sleep(50);
                    n += db.interrupt(7);
                }
                Throwable first = tagged.get(10, TimeUnit.SECONDS);
                assertTrue(first instanceof ChloeDBException && first.getMessage().contains("interrupt"), String.valueOf(first));
                while (n < 2 && System.nanoTime() - t0 < TimeUnit.SECONDS.toNanos(10)) {
                    Thread.sleep(50);
                    n += db.interrupt();
                }
                assertEquals(2, n);
                Throwable second = other.get(10, TimeUnit.SECONDS);
                assertTrue(second instanceof ChloeDBException && second.getMessage().startsWith("timeout:"), String.valueOf(second));
            } finally {
                pool.shutdownNow();
            }
            assertEquals(0, db.interrupt());
            assertEquals(1, db.query("SELECT COUNT(*) FROM t").size());
        }
    }

    @Test
    void aJdbcStatementsTimeoutAndCancel() throws Exception {
        made(60_000, new Database.Options()).close();
        try (Connection conn = DriverManager.getConnection("jdbc:chloedb:" + dir.resolve("t.strata")); Statement s = conn.createStatement()) {
            s.setQueryTimeout(1);
            long t0 = System.nanoTime();
            SQLTimeoutException late = assertThrows(SQLTimeoutException.class, () -> s.executeQuery(SLOW));
            assertEquals("HYT00", late.getSQLState());
            assertTrue(System.nanoTime() - t0 < TimeUnit.SECONDS.toNanos(10));
            s.setQueryTimeout(0);
            ExecutorService pool = Executors.newSingleThreadExecutor();
            try {
                Future<Throwable> q = pool.submit(() -> failure(() -> s.executeQuery(SLOW)));
                // Until it has started and stopped: a cancel stops a query
                // running, not one to come.
                long t1 = System.nanoTime();
                while (!q.isDone() && System.nanoTime() - t1 < TimeUnit.SECONDS.toNanos(10)) {
                    Thread.sleep(50);
                    s.cancel();
                }
                Throwable e = q.get(10, TimeUnit.SECONDS);
                assertTrue(e instanceof SQLException && "HY008".equals(((SQLException) e).getSQLState()), String.valueOf(e));
            } finally {
                pool.shutdownNow();
            }
        }
    }

    interface Body {
        Object run() throws Exception;
    }

    static Throwable failure(Body b) {
        try {
            b.run();
            return null;
        } catch (Throwable t) {
            return t;
        }
    }
}
