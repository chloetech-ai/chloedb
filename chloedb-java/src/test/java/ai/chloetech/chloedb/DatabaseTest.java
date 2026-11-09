package ai.chloetech.chloedb;

import static org.junit.jupiter.api.Assertions.assertArrayEquals;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.time.Instant;
import java.time.LocalDate;
import java.time.OffsetDateTime;
import java.time.ZoneOffset;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Collections;
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.Future;
import java.util.concurrent.TimeUnit;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

/** The Python package's tests of its Database (chloedb-py/tests/test_database.py), in Java. */
class DatabaseTest {
    static final String[] WORDS = "river mountain dragon detective ship garden war letter city winter star secret".split(" ");

    /** A map of keys and values, in their order (Map.of has none, and takes no nulls). */
    static Map<String, Object> map(Object... kv) {
        Map<String, Object> m = new LinkedHashMap<>();
        for (int i = 0; i < kv.length; i += 2) {
            m.put((String) kv[i], kv[i + 1]);
        }
        return m;
    }

    static Map<String, Object> field(String name, String type, Object... options) {
        Map<String, Object> f = map("name", name, "type", type);
        f.putAll(map(options));
        return f;
    }

    static final Map<String, Object> BOOKS = map(
            "table", "books",
            "key", "id",
            "fields", Arrays.asList(
                    field("id", "keyword"),
                    field("title", "text"),
                    field("description", "text"),
                    field("genres", "keyword", "multi", true),
                    field("year", "int"),
                    field("rating", "float"),
                    field("emb", "vector", "embed", map("from", Arrays.asList("title", "description"), "model", "hash:32"))));

    static Map<String, Object> book(int i) {
        StringBuilder d = new StringBuilder();
        for (int j = 0; j < 8; j++) {
            d.append(j == 0 ? "" : " ").append(WORDS[(i + j) % 12]);
        }
        return map(
                "id", String.format("b%04d", i),
                "title", WORDS[i % 12] + " " + WORDS[(i * 7) % 12],
                "description", d.toString(),
                "genres", Arrays.asList(i % 2 == 1 ? "fantasy" : "mystery", i % 5 == 0 ? "classic" : "new"),
                "year", 1900 + i % 120,
                "rating", Math.round((1 + (i % 40) / 10.0) * 10) / 10.0);
    }

    static List<Map<String, Object>> books(int from, int to) {
        List<Map<String, Object>> out = new ArrayList<>();
        for (int i = from; i < to; i++) {
            out.add(book(i));
        }
        return out;
    }

    @TempDir
    Path dir;

    Database db;

    @BeforeEach
    void open() {
        db = Database.create(dir.resolve("books.strata"), BOOKS);
        db.insert(books(0, 200));
    }

    @AfterEach
    void close() {
        db.close();
    }

    @Test
    void createOpenAndSchema() throws Exception {
        Path path = dir.resolve("s.strata");
        Path schemaFile = dir.resolve("books.json");
        Files.write(schemaFile, ("{\"table\": \"books\", \"key\": \"id\", \"fields\": [{\"name\": \"id\", \"type\": \"keyword\"}, {\"name\": \"title\", \"type\": \"text\"}]}").getBytes(StandardCharsets.UTF_8));
        try (Database d = Database.create(path, schemaFile)) {
            assertEquals("id", d.schema().get("key"));
            @SuppressWarnings("unchecked")
            List<Map<String, Object>> fields = (List<Map<String, Object>>) d.schema().get("fields");
            assertEquals(Arrays.asList("name", "type"), new ArrayList<>(fields.get(0).keySet()).subList(0, 2));
            assertEquals(1, d.insert(map("id", "a", "title", "one")));
        }
        try (Database d = Database.open(path)) {
            assertEquals(1, d.count());
            assertEquals("books", d.table().name());
            assertTrue(d.toString().contains("s.strata"));
        }
        // JSON text is a schema too, and the schema reads back the same.
        try (Database d = Database.create(dir.resolve("t.strata"), new String(Files.readAllBytes(schemaFile), StandardCharsets.UTF_8))) {
            try (Database e = Database.open(path)) {
                assertEquals(e.schema(), d.schema());
            }
        }
        ChloeDBException missing = assertThrows(ChloeDBException.class, () -> Database.open(dir.resolve("missing.strata")));
        assertTrue(missing.getMessage().contains("does not exist"), missing.getMessage());
        ChloeDBException dim = assertThrows(ChloeDBException.class, () -> Database.create(dir.resolve("bad.strata"), map("table", "t", "fields", Collections.singletonList(field("v", "vector")))));
        assertTrue(dim.getMessage().contains("dim"), dim.getMessage());
        // One process at a time: this one already has books.strata open.
        ChloeDBException busy = assertThrows(ChloeDBException.class, () -> Database.open(dir.resolve("books.strata")));
        assertTrue(busy.getMessage().contains("busy"), busy.getMessage());
    }

    @Test
    void readOnlyFollowsTheWriter() {
        // Another process, as the workers of a server are: here a second
        // open of the file, read-only.
        try (Database r = Database.open(dir.resolve("books.strata"), new Database.Options().readOnly(true))) {
            assertEquals(200, r.count());
            db.insert(books(200, 250));
            db.optimize();
            db.delete("b0001");
            assertEquals(249, r.count());
            assertFalse(db.refresh());
            ChloeDBException refused = assertThrows(ChloeDBException.class, () -> r.insert(book(1000)));
            assertTrue(refused.getMessage().contains("read-only"), refused.getMessage());
        }
    }

    @Test
    void writes() {
        assertEquals(200, db.count());
        // Small inserts go through the log, large ones are bulk-loaded; both
        // write all rows or none.
        ChloeDBException dup = assertThrows(ChloeDBException.class, () -> db.insert(Arrays.asList(book(1000), book(1))));
        assertTrue(dup.getMessage().contains("b0001"), dup.getMessage());
        List<Map<String, Object>> many = books(1000, 12_000);
        many.add(book(3));
        assertThrows(ChloeDBException.class, () -> db.insert(many));
        assertEquals(200, db.count());
        assertEquals(11_000, db.insert(books(1000, 12_000)));
        Map<String, Object> changed = book(5);
        changed.put("title", "Changed");
        assertEquals(2, db.upsert(Arrays.asList(changed, book(90_000))));
        assertEquals("Changed", db.query("SELECT title FROM books WHERE id = 'b0005'").scalar());
        assertEquals(1, db.delete(Arrays.asList("b0005", "nope")));
        assertEquals(1, db.delete("b90000"));
        assertEquals(11_199, db.count());
        ChloeDBException unknown = assertThrows(ChloeDBException.class, () -> db.insert(map("id", "x", "colour", "red")));
        assertTrue(unknown.getMessage().contains("colour"), unknown.getMessage());
        ChloeDBException year = assertThrows(ChloeDBException.class, () -> db.insert(map("id", "x", "year", "soon")));
        assertTrue(year.getMessage().contains("year"), year.getMessage());
        IllegalArgumentException object = assertThrows(IllegalArgumentException.class, () -> db.insert(map("id", "x", "title", new Object())));
        assertTrue(object.getMessage().contains("cannot store"), object.getMessage());
    }

    @Test
    void queriesAndResults() {
        Result r = db.query("SELECT id, year FROM books WHERE year >= :y AND genres = :g ORDER BY year DESC LIMIT 3", map("y", 2000, "g", "fantasy"));
        assertEquals(Arrays.asList("id", "year"), r.columns());
        assertEquals(3, r.size());
        assertTrue((Long) r.row(0).get("year") >= (Long) r.row(1).get("year"));
        List<Object> ids = new ArrayList<>();
        for (Map<String, Object> row : r) {
            ids.add(row.get("id"));
        }
        assertEquals(ids, r.column("id"));
        assertEquals(ids, r.keys());
        assertEquals(3, r.scores().length);
        assertTrue(r.stats().startsWith("{"), r.stats());
        assertEquals(200L, db.query("SELECT COUNT(*) FROM books").scalar());
        assertEquals(Arrays.asList("Project: id", "  Limit: 2", "    Search books"), db.query("EXPLAIN SELECT id FROM books LIMIT 2").column("plan"));
        Map<String, Object> plan = db.plan("SELECT id FROM books WHERE year >= :y ORDER BY year DESC LIMIT 3", map("y", 2000));
        assertEquals(1L, plan.get("version"));
        assertEquals(1, db.execute("UPDATE books SET rating = rating + 1 WHERE id = :k", map("k", "b0001")));
        assertEquals(3, db.execute("SELECT id FROM books LIMIT 3"));
        assertEquals(Arrays.asList("fantasy", "new"), db.query("SELECT genres FROM books WHERE id = 'b0001'").scalar());
        // A vector, the text's embedding, and a stored row's vector.
        float[] v = db.embed("dragon river");
        assertEquals(32, v.length);
        List<Object> a = db.query("SELECT id FROM books ORDER BY similarity(emb, :v) DESC LIMIT 5", map("v", v)).keys();
        List<Object> b = db.query("SELECT id FROM books ORDER BY similarity(emb, :v) DESC LIMIT 5", map("v", map("embed", "dragon river"))).keys();
        assertEquals(a, b);
        assertEquals(Collections.singletonList("b0007"), db.query("SELECT id FROM books ORDER BY similarity(emb, :v) DESC LIMIT 1", map("v", map("vector_of", "b0007"))).keys());
        // A double[] is a vector too.
        double[] dv = new double[v.length];
        for (int i = 0; i < v.length; i++) {
            dv[i] = v[i];
        }
        assertEquals(a, db.query("SELECT id FROM books ORDER BY similarity(emb, :v) DESC LIMIT 5", map("v", dv)).keys());
        assertThrows(ChloeDBException.class, () -> db.query("SELECT nope FROM books"));
        IllegalArgumentException typo = assertThrows(IllegalArgumentException.class, () -> db.importSqlite("x.sqlite", map("table", "books")));
        assertTrue(typo.getMessage().contains("no option \"table\"") && typo.getMessage().contains("tables"), typo.getMessage());
    }

    @Test
    void vectorsGivenAsFloatArrays() {
        Map<String, Object> schema = map("table", "t", "key", "id", "fields", Arrays.asList(field("id", "int"), field("v", "vector", "dim", 3)));
        try (Database d = Database.create(dir.resolve("v.strata"), schema)) {
            d.insert(Arrays.asList(map("id", 1, "v", new float[] {1, 0, 0}), map("id", 2, "v", new float[] {0, 1, 0}), map("id", 3, "v", Arrays.asList(0.0, 0.0, 1.0))));
            Result r = d.query("SELECT id, v FROM t ORDER BY similarity(v, :q) DESC LIMIT 1", map("q", new float[] {0.1f, 0.9f, 0}));
            assertEquals(2L, r.scalar());
            assertArrayEquals(new float[] {0, 1, 0}, (float[]) r.row(0).get("v"));
            assertEquals(1L, d.query("SELECT id FROM t ORDER BY similarity(v, :q) DESC LIMIT 1", map("q", Arrays.asList(0.9, 0.1, 0.0))).scalar());
        }
    }

    @Test
    void search() {
        Result text = db.search("dragon", new SearchOptions().mode("text").limit(5));
        assertEquals(5, text.size());
        for (Map<String, Object> row : text) {
            assertTrue(((String) row.get("title") + row.get("description")).contains("dragon"));
        }
        assertFalse(db.search("dragon", new SearchOptions().mode("vector").limit(5)).keys().isEmpty());
        Result hybrid = db.search("dragon", new SearchOptions().limit(5).where("year >= :y").param("y", 1950).columns("id", "year"));
        assertEquals(Arrays.asList("id", "year"), hybrid.columns());
        for (Map<String, Object> row : hybrid) {
            assertTrue((Long) row.get("year") >= 1950);
        }
        assertEquals(10, db.search("winter").size());
        assertThrows(IllegalArgumentException.class, () -> new SearchOptions().mode("fuzzy"));
    }

    @Test
    void importFile() throws Exception {
        Path rows = dir.resolve("rows.jsonl");
        StringBuilder jsonl = new StringBuilder();
        for (int i = 0; i < 50; i++) {
            jsonl.append("{\"id\": \"b").append(String.format("%04d", i)).append("\", \"title\": \"t").append(i).append("\", \"publisher\": \"Acme\"}\n");
        }
        Files.write(rows, jsonl.toString().getBytes(StandardCharsets.UTF_8));
        Path csv = dir.resolve("more.csv");
        Files.write(csv, "id,title,genres,year\nb0001,\"Re, titled\",a|b,1999\nc1,New,,\n".getBytes(StandardCharsets.UTF_8));
        try (Database d = Database.create(dir.resolve("i.strata"), BOOKS)) {
            assertEquals(map("rows", 50L, "bulk", true, "skipped", Collections.singletonList("publisher")), d.importFile(rows.toString()));
            assertEquals(map("rows", 2L, "bulk", false, "skipped", Collections.emptyList()), d.importFile(csv.toString(), "csv"));
            assertEquals(51, d.count());
            assertEquals(Collections.singletonList(Arrays.asList("Re, titled", Arrays.asList("a", "b"))), d.query("SELECT title, genres FROM books WHERE id = 'b0001'").rows());
            Path bad = dir.resolve("bad.jsonl");
            Files.write(bad, "{\"id\": 1, \"year\": \"x\"}\n".getBytes(StandardCharsets.UTF_8));
            ChloeDBException e = assertThrows(ChloeDBException.class, () -> d.importFile(bad.toString()));
            assertTrue(e.getMessage().contains("line 1"), e.getMessage());
        }
    }

    @Test
    void modelsThatDoNotOpen() {
        Map<String, Object> schema = map("table", "t", "key", "id", "fields", Arrays.asList(field("id", "keyword"), field("body", "text"), field("v", "vector", "embed", map("from", "body", "model", "potion-base-8M"))));
        try (Database d = Database.create(dir.resolve("m.strata"), schema, new Database.Options().models(dir))) {
            assertTrue(d.embedErrors().containsKey("v"));
            d.insert(map("id", "a", "body", "no vector"));
            float[] v = new float[256];
            Arrays.fill(v, 0.1f);
            d.insert(map("id", "b", "body", "a vector", "v", v));
            assertEquals(2, d.count());
            // Text only: the model is not open.
            assertFalse(d.search("vector").keys().isEmpty());
            ChloeDBException e = assertThrows(ChloeDBException.class, () -> d.embed("x"));
            assertTrue(e.getMessage().contains("model"), e.getMessage());
        }
    }

    @Test
    void threadsWriteAtOnce() throws Exception {
        // Threads that write at once wait for each other and share commits:
        // none fails as busy, and no increment is lost.
        ExecutorService pool = Executors.newFixedThreadPool(8);
        List<Future<?>> work = new ArrayList<>();
        long year0 = (Long) db.query("SELECT year FROM books WHERE id = 'b0000'").scalar();
        for (int t = 0; t < 8; t++) {
            int th = t;
            work.add(pool.submit(() -> {
                for (int i = 0; i < 25; i++) {
                    assertEquals(1, db.insert(book(10_000 + th * 100 + i)));
                    assertEquals(1, db.write(Collections.singletonList(map("op", "add", "key", "b0000", "field", "year", "value", 1))));
                    assertEquals(1, db.execute("UPDATE books SET rating = rating + 1 WHERE id = 'b0001'"));
                }
                return null;
            }));
        }
        for (Future<?> f : work) {
            f.get(60, TimeUnit.SECONDS);
        }
        pool.shutdown();
        assertEquals(200 + 8 * 25, db.count());
        assertEquals(year0 + 8 * 25, db.query("SELECT year FROM books WHERE id = 'b0000'").scalar());
        assertEquals(1.1 + 8 * 25, (Double) db.query("SELECT rating FROM books WHERE id = 'b0001'").scalar(), 1e-9);
    }

    @Test
    void threadsAndClose() throws Exception {
        ExecutorService pool = Executors.newFixedThreadPool(4);
        List<Future<?>> work = new ArrayList<>();
        for (int t = 0; t < 4; t++) {
            work.add(pool.submit(() -> {
                for (int i = 0; i < 20; i++) {
                    assertEquals(3, db.search("winter", new SearchOptions().limit(3)).size());
                }
            }));
        }
        for (Future<?> f : work) {
            f.get(60, TimeUnit.SECONDS);
        }
        pool.shutdown();
        db.close();
        assertTrue(db.isClosed());
        IllegalStateException e = assertThrows(IllegalStateException.class, () -> db.query("SELECT id FROM books"));
        assertTrue(e.getMessage().contains("closed"), e.getMessage());
        // Closing again does nothing.
        db.close();
        // The file is free for another open.
        try (Database again = Database.open(dir.resolve("books.strata"))) {
            assertEquals(200, again.count());
        }
    }

    @Test
    void aDatabaseLeftOpenIsClosedWhenCollected() throws Exception {
        Path path = dir.resolve("left.strata");
        Database.create(path, BOOKS).insert(book(1));
        // Nothing holds it now: the cleaner frees its handle, which closes
        // it, once the collector finds it.
        long until = System.nanoTime() + TimeUnit.SECONDS.toNanos(30);
        while (true) {
            System.gc();
            try (Database d = Database.open(path)) {
                assertEquals(1, d.count());
                break;
            } catch (ChloeDBException busy) {
                if (System.nanoTime() > until) {
                    throw busy;
                }
                Thread.sleep(50);
            }
        }
    }

    static final Map<String, Object> TASKS = map("table", "tasks", "key", "id", "fields", Arrays.asList(field("id", "keyword"), field("done", "bool"), field("at", "timestamp"), field("n", "int")));

    @Test
    void boolAndTimestamp() {
        try (Database d = Database.create(dir.resolve("tasks.strata"), TASKS)) {
            d.insert(Arrays.asList(
                    map("id", "a", "done", true, "at", Instant.parse("2026-10-01T09:00:00Z"), "n", 1),
                    map("id", "b", "done", false, "at", LocalDate.of(2026, 10, 2), "n", 2),
                    map("id", "c", "done", null, "at", OffsetDateTime.of(2026, 10, 1, 3, 30, 0, 0, ZoneOffset.ofHours(2)), "n", 3),
                    map("id", "d", "done", "true", "at", "2026-10-03T12:00:00.25Z", "n", 4)));
            // Booleans come back as Boolean, times as Instant.
            List<List<Object>> want = Arrays.asList(
                    Arrays.asList("a", true, Instant.parse("2026-10-01T09:00:00Z")),
                    Arrays.asList("b", false, Instant.parse("2026-10-02T00:00:00Z")),
                    Arrays.asList("c", null, Instant.parse("2026-10-01T01:30:00Z")),
                    Arrays.asList("d", true, Instant.parse("2026-10-03T12:00:00.250Z")));
            assertEquals(want, d.query("SELECT id, done, at FROM tasks ORDER BY n").rows());
            assertEquals(Arrays.asList("a", "d"), d.query("SELECT id FROM tasks WHERE done = :d ORDER BY n", map("d", true)).keys());
            assertEquals(Arrays.asList("b", "d"), d.query("SELECT id FROM tasks WHERE at >= :t ORDER BY n", map("t", Instant.parse("2026-10-01T12:00:00Z"))).keys());
            assertEquals(Instant.parse("2026-10-03T12:00:00.250Z"), d.query("SELECT MAX(at) FROM tasks").scalar());
            assertEquals(1, d.execute("UPDATE tasks SET done = :d, at = :t WHERE id = 'c'", map("d", false, "t", LocalDate.of(2026, 11, 1))));
            assertEquals(map("done", false, "at", Instant.parse("2026-11-01T00:00:00Z")), d.query("SELECT done, at FROM tasks WHERE id = 'c'").row(0));
            // Before 1970 too.
            d.upsert(map("id", "e", "at", Instant.parse("1969-12-31T23:59:59.999999Z")));
            assertEquals(Instant.parse("1969-12-31T23:59:59.999999Z"), d.get("e").get("at"));
            ChloeDBException e = assertThrows(ChloeDBException.class, () -> d.insert(map("id", "x", "done", 2)));
            assertTrue(e.getMessage().contains("done"), e.getMessage());
        }
    }

    @Test
    void jsonFields() {
        Map<String, Object> schema = map("table", "items", "key", "id", "fields", Arrays.asList(field("id", "keyword"), field("attrs", "json")));
        try (Database d = Database.create(dir.resolve("items.strata"), schema)) {
            d.insert(Arrays.asList(
                    map("id", "a", "attrs", map("color", "red", "dims", map("w", 4), "certs", Arrays.asList("CE", "UL"))),
                    map("id", "b", "attrs", "{\"color\": \"blue\", \"voltage\": 5}"),
                    map("id", "c", "attrs", null)));
            assertEquals(Collections.singletonList("a"), d.query("SELECT id FROM items WHERE attrs.color = :c", map("c", "red")).keys());
            assertEquals(Arrays.asList("a", "b"), d.query("SELECT id FROM items WHERE attrs.certs = 'UL' OR attrs.voltage >= 5 ORDER BY id").keys());
            assertEquals(map("w", 4L, "attrs", "{\"certs\":[\"CE\",\"UL\"],\"color\":\"red\",\"dims\":{\"w\":4}}"), d.query("SELECT attrs.dims.w AS w, attrs FROM items WHERE id = 'a'").row(0));
        }
    }

    @Test
    void writesToAnyTable() throws Exception {
        assertEquals(0, db.execute("CREATE TABLE events (id INT PRIMARY KEY, usr KEYWORD NOT NULL, book KEYWORD REFERENCES books, ts INT) OPTIONS(shard_by = 'usr')"));
        List<String> names = new ArrayList<>();
        for (Map<String, Object> t : db.tables()) {
            names.add((String) t.get("table"));
        }
        assertEquals(Arrays.asList("books", "events"), names);
        assertEquals("books", db.schema().get("table"));
        Table events = db.table("events");
        List<Map<String, Object>> rows = new ArrayList<>();
        for (int i = 0; i < 1500; i++) {
            rows.add(map("id", i, "usr", "u" + i % 3, "book", book(i).get("id"), "ts", i));
        }
        assertEquals(1500, events.insert(rows));
        assertEquals(1, db.table("EVENTS").upsert(map("id", 1, "usr", "u1", "book", "b0007", "ts", 9)));
        assertEquals(2, events.delete(Arrays.asList(2, 3, 99999)));
        Path f = dir.resolve("more.csv");
        Files.write(f, "id,usr,ts\n5000,u9,1\n5001,u9,2\n".getBytes(StandardCharsets.UTF_8));
        assertEquals(2L, events.importFile(f.toString()).get("rows"));
        assertEquals(1500L, db.query("SELECT COUNT(*) FROM events").scalar());
        assertEquals(200, db.count());
        assertEquals("b0007", events.get(1).get("book"));
        assertEquals("events", events.name());
        // Joins across them.
        assertEquals(1L, db.query("SELECT COUNT(*) FROM events e JOIN books b ON e.book = b.id WHERE e.id = 1").scalar());
        ChloeDBException e = assertThrows(ChloeDBException.class, () -> db.table("nope").insert(map("id", 1)));
        assertTrue(e.getMessage().contains("no such table: nope"), e.getMessage());
    }

    @Test
    void columnsAddedDroppedAndRenamed() {
        Map<String, Object> schema = map("table", "items", "key", "id", "fields", Arrays.asList(field("id", "keyword"), field("title", "text"), field("n", "int")));
        Path path = dir.resolve("items.strata");
        try (Database d = Database.create(path, schema)) {
            d.insert(Arrays.asList(map("id", "a", "title", "red apple", "n", 1), map("id", "b", "title", "green pear", "n", 2)));
            d.addColumn("price", "float", map("default", 1.5));
            d.table("items").addColumn("tag", "keyword");
            d.insert(map("id", "c", "title", "ripe plum", "n", 3, "price", 3.0, "tag", "x"));
            assertEquals(Arrays.asList(Arrays.asList("a", 1.5, null), Arrays.asList("b", 1.5, null), Arrays.asList("c", 3.0, "x")), d.query("SELECT id, price, tag FROM items ORDER BY id").rows());
            d.renameColumn("title", "headline");
            assertEquals(Collections.singletonList("b"), d.query("SELECT id FROM items WHERE MATCH(headline, 'pear')").keys());
            d.dropColumn("n");
            assertThrows(ChloeDBException.class, () -> d.query("SELECT n FROM items"));
            ChloeDBException pk = assertThrows(ChloeDBException.class, () -> d.dropColumn("id"));
            assertTrue(pk.getMessage().contains("primary key"), pk.getMessage());
        }
        try (Database d = Database.open(path)) {
            List<Object> names = new ArrayList<>();
            for (Object f : (List<?>) d.schema().get("fields")) {
                names.add(((Map<?, ?>) f).get("name"));
            }
            assertEquals(Arrays.asList("id", "headline", "price", "tag"), names);
        }
    }

    @Test
    void rowsByKey() {
        Map<String, Object> got = db.get("b0007");
        assertEquals("b0007", got.get("id"));
        assertEquals(1907L, got.get("year"));
        assertFalse(got.containsKey("emb"));
        assertNull(db.get("nope"));
        List<Map<String, Object>> rows = db.getMany(Arrays.asList("b0001", "missing", "b0002"));
        assertEquals("b0001", rows.get(0).get("id"));
        assertNull(rows.get(1));
        assertEquals(db.query("SELECT * FROM books WHERE id = 'b0001'").row(0), rows.get(0));
        // A row read is the caller's: changed and written back.
        Map<String, Object> row = db.get("b0003");
        row.put("title", "Rewritten");
        assertEquals(1, db.upsert(row));
        assertEquals("Rewritten", db.get("b0003").get("title"));
        db.execute("CREATE TABLE sessions (id KEYWORD PRIMARY KEY, user KEYWORD, until TIMESTAMP) OPTIONS(expire_by = 'until')");
        db.table("sessions").insert(Arrays.asList(map("id", "s1", "user", "a", "until", "2001-01-01T00:00:00Z"), map("id", "s2", "user", "b", "until", "2999-01-01T00:00:00Z")));
        assertNull(db.table("sessions").get("s1"));
        assertEquals(1, db.expire());
        assertEquals(0, db.expire());
    }

    @Test
    void writeOperations() {
        db.execute("CREATE TABLE counters (id KEYWORD PRIMARY KEY, n INT)");
        long n = db.write(Arrays.asList(
                map("op", "add", "table", "counters", "doc", map("id", "c1", "n", 0), "field", "n", "value", 5),
                map("op", "add", "table", "counters", "key", "c1", "field", "n", "value", 2),
                map("op", "set", "key", "b0001", "field", "year", "value", 2001),
                map("op", "delete", "key", "b0002")));
        assertEquals(4, n);
        assertEquals(7L, db.table("counters").get("c1").get("n"));
        assertEquals(2001L, db.get("b0001").get("year"));
        assertNull(db.get("b0002"));
        ChloeDBException e = assertThrows(ChloeDBException.class, () -> db.write(Collections.singletonList(map("op", "explode"))));
        assertTrue(e.getMessage().contains("operation 1"), e.getMessage());
    }

    @Test
    void theRankLog() {
        Instant t0 = Instant.parse("2026-10-01T00:00:00Z");
        String sql = "SELECT id, title FROM books ORDER BY bm25(title, :q) * (1 + 0.1 * rating) DESC LIMIT 5";
        Result served = db.query("SELECT id, score() FROM books ORDER BY bm25(title, :q) * (1 + 0.1 * rating) DESC LIMIT 8", map("q", "dragon"));
        Result r = db.queryLogged(sql, map("q", "dragon"), map("context", "u1", "variant", "a", "candidates", 8, "ts", t0));
        assertEquals(Arrays.asList("id", "title"), r.columns());
        assertEquals(served.keys().subList(0, 5), r.keys());
        assertEquals(32, r.request().length());
        assertNull(r.teams());
        db.execute("UPDATE rank_log SET label = 2 WHERE request = :r AND row_key = :k", map("r", r.request(), "k", r.keys().get(1)));
        List<Map<String, Object>> records = db.exportRankLog(null);
        List<Object> keys = new ArrayList<>();
        List<Object> labels = new ArrayList<>();
        for (Map<String, Object> x : records) {
            keys.add(x.get("row_key"));
            labels.add(x.get("label"));
        }
        assertEquals(served.keys(), keys);
        assertEquals(Arrays.asList(null, 2.0, null, null, null, null, null, null), labels);
        assertEquals("u1", records.get(0).get("context"));
        assertEquals(0, db.exportRankLog(map("since", t0.plusSeconds(1))).size());
        // Two rankings interleaved, each row with the name of the one that placed it.
        Result both = db.queryLogged(sql, map("q", "dragon"), map("interleave", map("sql", "SELECT id, title FROM books ORDER BY bm25(title, :q) DESC LIMIT 5", "variants", Arrays.asList("rated", "plain"))));
        assertEquals(5, both.teams().size());
        assertTrue(new java.util.HashSet<>(Arrays.asList("rated", "plain")).containsAll(both.teams()));
    }

    @Test
    void sqliteOutAndBackIn() {
        Path sqlite = dir.resolve("books.sqlite");
        Map<String, Object> wrote = db.exportSqlite(sqlite.toString());
        assertNotNull(wrote);
        assertTrue(Files.exists(sqlite));
        Map<String, Object> plan = Database.planSqlite(sqlite.toString());
        assertNotNull(plan.get("tables"));
        try (Database back = Database.fromSqlite(sqlite.toString(), dir.resolve("back.strata").toString())) {
            assertNotNull(back.lastImport());
            assertEquals(200L, back.query("SELECT COUNT(*) FROM books").scalar());
            assertEquals(db.get("b0042").get("title"), back.get("b0042").get("title"));
        }
        try (Database d = Database.create(dir.resolve("into.strata"), BOOKS)) {
            d.importSqlite(sqlite.toString(), map("tables", "books"));
            assertEquals(200, d.count());
        }
    }

    @Test
    void parquetOutAndBackIn() {
        @SuppressWarnings("unchecked")
        List<String> features = (List<String>) Database.nativeLibrary().get("features");
        if (!features.contains("parquet")) {
            return;
        }
        Path parquet = dir.resolve("books.parquet");
        db.exportParquet(parquet.toString());
        assertTrue(Files.exists(parquet));
        try (Database back = Database.fromParquet(parquet.toString(), dir.resolve("p.strata").toString())) {
            assertEquals(200, back.count());
            assertEquals(Arrays.asList("fantasy", "new"), back.get("b0001").get("genres"));
        }
        try (Database d = Database.create(dir.resolve("pi.strata"), BOOKS)) {
            assertEquals(200L, d.importParquet(parquet.toString()).get("rows"));
        }
    }

    @Test
    void optimizeKeepsTheRows() {
        db.optimize();
        assertEquals(200, db.count());
        assertEquals(10, db.search("dragon").size());
    }

    @Test
    void theNativeLibrary() {
        Map<String, Object> lib = Database.nativeLibrary();
        assertEquals(Native.ABI, lib.get("abi"));
        assertNotNull(lib.get("version"));
        assertTrue(Native.platform().matches("[a-z]+-[a-z0-9_]+"), Native.platform());
    }

    /** Tables of Java values, as the native side read them. */
    @Test
    void valuesCrossTheBoundary() {
        Map<String, Object> given = new HashMap<>();
        given.put("value", map(
                "null", null,
                "bool", true,
                "int", 7,
                "short", (short) -3,
                "long", Long.MIN_VALUE,
                "big", new java.math.BigInteger("9007199254740993"),
                "float", 0.5f,
                "double", Math.PI,
                "string", "héllo, 世界",
                "char", 'c',
                "floats", new float[] {1.5f, -2},
                "ints", new int[] {1, 2},
                "list", Arrays.asList(1, "a", null),
                "array", new Object[] {false},
                "map", map("k", map()),
                "time", Instant.parse("2026-10-01T09:00:00.123456Z"),
                "date", LocalDate.of(2026, 10, 2),
                "bytes", new byte[] {1, 2, 3}));
        @SuppressWarnings("unchecked")
        Map<String, Object> back = (Map<String, Object>) Codec.decode(Native.call(0, "echo", Codec.encode(given)));
        assertNull(back.get("null"));
        assertEquals(true, back.get("bool"));
        assertEquals(7L, back.get("int"));
        assertEquals(-3L, back.get("short"));
        assertEquals(Long.MIN_VALUE, back.get("long"));
        assertEquals(9007199254740993L, back.get("big"));
        assertEquals(0.5, back.get("float"));
        assertEquals(Math.PI, back.get("double"));
        assertEquals("héllo, 世界", back.get("string"));
        assertEquals("c", back.get("char"));
        assertArrayEquals(new float[] {1.5f, -2}, (float[]) back.get("floats"));
        assertEquals(Arrays.asList(1L, 2L), back.get("ints"));
        assertEquals(Arrays.asList(1L, "a", null), back.get("list"));
        assertEquals(Collections.singletonList(false), back.get("array"));
        assertEquals(map("k", map()), back.get("map"));
        assertEquals(Instant.parse("2026-10-01T09:00:00.123456Z"), back.get("time"));
        assertEquals(Instant.parse("2026-10-02T00:00:00Z"), back.get("date"));
        assertArrayEquals(new byte[] {1, 2, 3}, (byte[]) back.get("bytes"));
        // The keys in the order given.
        assertEquals(new ArrayList<>(((Map<?, ?>) given.get("value")).keySet()), new ArrayList<>(back.keySet()));
        Map<String, Object> self = new HashMap<>();
        self.put("self", self);
        assertThrows(IllegalArgumentException.class, () -> Codec.encode(self));
        assertThrows(IllegalArgumentException.class, () -> Codec.encode(Collections.singletonMap(1, 2)));
        assertThrows(IllegalArgumentException.class, () -> Codec.encode(new java.math.BigInteger("99999999999999999999")));
        // Damaged bytes are refused by the native side, not read past.
        assertThrows(IllegalArgumentException.class, () -> Native.call(0, "echo", new byte[] {Codec.MAP, 1, 0, 0, 0}));
    }
}
