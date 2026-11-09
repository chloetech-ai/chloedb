package ai.chloetech.strata;

import java.lang.ref.Cleaner;
import java.lang.ref.Reference;
import java.nio.file.Path;
import java.util.Collection;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * A ChloeDB database opened in this process: a file (or a directory of
 * segment files) read and written by ChloeDB's native library, as
 * sqlite-jdbc opens a SQLite file. No server is involved.
 *
 * <pre>{@code
 * Map<String, Object> schema = Map.of("table", "books", "key", "id", "fields", List.of(
 *     Map.of("name", "id", "type", "keyword"),
 *     Map.of("name", "title", "type", "text"),
 *     Map.of("name", "year", "type", "int")));
 * try (Database db = Database.create("books.strata", schema)) {
 *     db.upsert(Map.of("id", "b1", "title", "The river and the dragon", "year", 1999));
 *     for (Map<String, Object> row : db.search("dragon")) {
 *         System.out.println(row.get("id") + " " + row.get("title"));
 *     }
 *     db.query("SELECT year, COUNT(*) FROM books GROUP BY year").rows();
 * }
 * }</pre>
 *
 * <p>Only one process may have a database open to write it at a time
 * (another gets a {@link StrataException}: "busy"); other processes may open
 * it to read ({@link Options#readOnly}) and see each of its commits, or
 * share it through {@code chloedb-server}. Within the process a {@code Database} is safe to
 * use from many threads at once: queries run in parallel, and calls do not
 * hold the JVM. Close it when done ({@link #close()}, or try-with-resources):
 * one left open is closed when it is garbage-collected.
 *
 * <p>The methods that act on a table ({@link #insert}, {@link #search},
 * {@link #schema()}, ...) act on the first; {@link #table(String)} gives
 * another. Query parameters ({@code :name} in the SQL) are a map of name to
 * value: as a row's values, and also {@code Map.of("embed", "some text")},
 * the text's vector by an embedded field's model, and
 * {@code Map.of("vector_of", key)}, a stored row's vector.
 */
public final class Database implements AutoCloseable {
    private static final Cleaner CLEANER = Cleaner.create();

    private final long handle;
    private final String path;
    private final Map<String, String> embedErrors;
    private final Map<String, Object> lastImport;
    private final Table first;

    /** How a database is created or opened. */
    public static final class Options {
        private boolean directory;
        private String models;
        private Integer threads;
        private boolean readOnly;

        /**
         * Keeps a new database as a directory of segment files instead of
         * one file (only a create reads it: a database's layout is its own).
         */
        public Options directory(boolean directory) {
            this.directory = directory;
            return this;
        }

        /**
         * Where embedding models are, by name: a directory of model
         * directories. Unless set, {@code STRATA_MODELS} or ChloeDB's
         * default.
         */
        public Options models(String dir) {
            this.models = dir;
            return this;
        }

        public Options models(Path dir) {
            return models(dir.toString());
        }

        /** The cores queries and index builds use; unless set, the machine's (its cgroup's quota on Linux). */
        public Options threads(int threads) {
            if (threads < 1) {
                throw new IllegalArgumentException("threads: at least 1");
            }
            this.threads = threads;
            return this;
        }

        /**
         * Opens the database to read it while another process writes it: it
         * sees each of the writer's commits, and every write fails (only an
         * open reads it).
         */
        public Options readOnly(boolean readOnly) {
            this.readOnly = readOnly;
            return this;
        }

        Map<String, Object> args() {
            Map<String, Object> m = new LinkedHashMap<>();
            m.put("directory", directory);
            m.put("models", models);
            m.put("threads", threads);
            return m;
        }
    }

    @SuppressWarnings("unchecked")
    private Database(Object opened, String path) {
        Map<String, Object> m = (Map<String, Object>) opened;
        this.handle = (Long) m.get("handle");
        this.path = path;
        this.embedErrors = (Map<String, String>) (Map<?, ?>) m.get("embedErrors");
        this.lastImport = (Map<String, Object>) m.get("report");
        this.first = new Table(this, null);
        CLEANER.register(this, new Free(handle));
    }

    /** Frees the native handle once the database is unreachable (and so in no call). */
    private static final class Free implements Runnable {
        private final long handle;

        Free(long handle) {
            this.handle = handle;
        }

        @Override
        public void run() {
            Native.free(handle);
        }
    }

    /** Arguments by name: {@code options}, then the pairs of {@code kv}. */
    static Map<String, Object> args(Map<String, ?> options, Object... kv) {
        Map<String, Object> m = new LinkedHashMap<>();
        if (options != null) {
            m.putAll(options);
        }
        for (int i = 0; i < kv.length; i += 2) {
            m.put((String) kv[i], kv[i + 1]);
        }
        return m;
    }

    Object call(String method, Map<String, Object> args) {
        try {
            return Codec.decode(Native.call(handle, method, Codec.encode(args)));
        } finally {
            // Until the call returns, the cleaner must not free the handle.
            Reference.reachabilityFence(this);
        }
    }

    private Object call(String method, Object... kv) {
        return call(method, args(null, kv));
    }

    private static Object calls(String method, Map<String, Object> args) {
        return Codec.decode(Native.call(0, method, Codec.encode(args)));
    }

    // Opening

    /**
     * Creates a database at {@code path} with {@code schema}: a map (the
     * short form, or the long one), JSON text, or the path of a JSON file
     * (a {@code String} or a {@code Path}).
     */
    public static Database create(String path, Object schema) {
        return create(path, schema, new Options());
    }

    public static Database create(String path, Object schema, Options options) {
        Map<String, Object> a = options.args();
        a.put("path", path);
        a.put("schema", schema instanceof Path ? schema.toString() : schema);
        return new Database(calls("create", a), path);
    }

    public static Database create(Path path, Object schema) {
        return create(path.toString(), schema);
    }

    public static Database create(Path path, Object schema, Options options) {
        return create(path.toString(), schema, options);
    }

    /** Opens the database at {@code path}. */
    public static Database open(String path) {
        return open(path, new Options());
    }

    public static Database open(String path, Options options) {
        Map<String, Object> a = options.args();
        a.remove("directory");
        a.put("path", path);
        a.put("readOnly", options.readOnly);
        return new Database(calls("open", a), path);
    }

    public static Database open(Path path) {
        return open(path.toString());
    }

    public static Database open(Path path, Options options) {
        return open(path.toString(), options);
    }

    /**
     * Creates a database at {@code path} from the tables of the SQLite file
     * {@code sqlitePath}; {@link #lastImport()} says what it did.
     * {@code options}: {@code tables} (a name or a list), {@code schemas}
     * (a schema, a list of them or a plan from {@link #planSqlite}),
     * {@code blobsBase64}, and {@code directory}, {@code models} and
     * {@code threads} as {@link Options} has them.
     */
    public static Database fromSqlite(String sqlitePath, String path, Map<String, ?> options) {
        return new Database(calls("fromSqlite", args(options, "sqlitePath", sqlitePath, "path", path)), path);
    }

    public static Database fromSqlite(String sqlitePath, String path) {
        return fromSqlite(sqlitePath, path, null);
    }

    /** What an import of the SQLite file {@code sqlitePath} into a new database would do; {@code options} as {@link #fromSqlite} takes them. */
    @SuppressWarnings("unchecked")
    public static Map<String, Object> planSqlite(String sqlitePath, Map<String, ?> options) {
        return (Map<String, Object>) calls("planSqlite", args(options, "sqlitePath", sqlitePath));
    }

    public static Map<String, Object> planSqlite(String sqlitePath) {
        return planSqlite(sqlitePath, null);
    }

    /**
     * Creates a database at {@code path} whose first table is the one the
     * Parquet file or directory {@code parquetPath} (or {@code s3://} URL)
     * makes, and loads it; {@link #lastImport()} says what it did.
     * {@code options}: {@code table}, {@code schema}, {@code key},
     * {@code blobsBase64}, and {@code directory}, {@code models} and
     * {@code threads}.
     */
    public static Database fromParquet(String parquetPath, String path, Map<String, ?> options) {
        return new Database(calls("fromParquet", args(options, "parquetPath", parquetPath, "path", path)), path);
    }

    public static Database fromParquet(String parquetPath, String path) {
        return fromParquet(parquetPath, path, null);
    }

    /** What an import of the Parquet file or directory {@code parquetPath} into a new database would do. */
    @SuppressWarnings("unchecked")
    public static Map<String, Object> planParquet(String parquetPath, Map<String, ?> options) {
        return (Map<String, Object>) calls("planParquet", args(options, "parquetPath", parquetPath));
    }

    public static Map<String, Object> planParquet(String parquetPath) {
        return planParquet(parquetPath, null);
    }

    /**
     * Copies the database at {@code path}, written by a version of ChloeDB
     * from before field ids (which no longer opens), into this version's
     * format in its place, keeping the old one as
     * {@code <path>.before-field-ids}. Returns {@code null} when it needs no
     * upgrade, else {@code old} (the old database's path), {@code tables}
     * and {@code rows} (copied).
     */
    @SuppressWarnings("unchecked")
    public static Map<String, Object> upgrade(String path) {
        return (Map<String, Object>) calls("upgrade", args(null, "path", path));
    }

    /** The native library's {@code version} and the optional {@code features} it was built with ({@code parquet}, {@code s3}, {@code bert}, ...). */
    @SuppressWarnings("unchecked")
    public static Map<String, Object> nativeLibrary() {
        return (Map<String, Object>) calls("version", args(null));
    }

    // The database

    /** The first table. */
    public Table table() {
        return first;
    }

    /** The table named {@code name} (any case). */
    public Table table(String name) {
        return new Table(this, name);
    }

    /** Every table's schema in its short form, the first first. */
    @SuppressWarnings("unchecked")
    public List<Map<String, Object>> tables() {
        return (List<Map<String, Object>>) call("tables");
    }

    /** Embedded fields whose model did not open, with the reason: rows written here must bring those fields' vectors. */
    public Map<String, String> embedErrors() {
        return embedErrors;
    }

    /** What the SQLite or Parquet import that made this database did ({@link #fromSqlite}, {@link #fromParquet}); else {@code null}. */
    public Map<String, Object> lastImport() {
        return lastImport;
    }

    /** The first table's live rows. */
    public long count() {
        return (Long) call("count");
    }

    /** Runs a SELECT (or an EXPLAIN of a statement). */
    public Result query(String sql) {
        return query(sql, null);
    }

    /** Runs a SELECT with its {@code :name} parameters. */
    @SuppressWarnings("unchecked")
    public Result query(String sql, Map<String, ?> params) {
        return new Result((Map<String, Object>) call("query", "sql", sql, "params", params));
    }

    /**
     * Runs any statement (INSERT, UPDATE, DELETE, CREATE TABLE, ALTER
     * TABLE...); returns the rows it wrote, or, for a SELECT, the rows it
     * answered.
     */
    public long execute(String sql) {
        return execute(sql, null);
    }

    /** {@link #execute(String)} with {@code :name} parameters. */
    @SuppressWarnings("unchecked")
    public long execute(String sql, Map<String, ?> params) {
        Map<String, Object> r = (Map<String, Object>) call("sql", "sql", sql, "params", params);
        Object written = r.get("written");
        return written != null ? (Long) written : ((List<?>) r.get("rows")).size();
    }

    /** The plan of a SELECT, INSERT, UPDATE or DELETE, without running it. */
    public Map<String, Object> plan(String sql) {
        return plan(sql, null);
    }

    @SuppressWarnings("unchecked")
    public Map<String, Object> plan(String sql, Map<String, ?> params) {
        return (Map<String, Object>) call("plan", "sql", sql, "params", params);
    }

    /**
     * Runs a SELECT ranked by an ORDER BY and logs what it served to a log
     * table, which the first query logged to it makes ({@code log}: the
     * options chloedb-server's {@code "log"} takes: {@code table},
     * {@code request}, {@code context}, {@code variant}, {@code candidates},
     * {@code ts} (an {@code Instant} or ISO 8601 text) and
     * {@code interleave}, a map of a second SELECT's {@code sql} and the
     * {@code variants}' names). {@link Result#request()} is the
     * request's id, which labels its rows later.
     */
    @SuppressWarnings("unchecked")
    public Result queryLogged(String sql, Map<String, ?> params, Map<String, ?> log) {
        return new Result((Map<String, Object>) call("queryLogged", "sql", sql, "params", params, "log", log));
    }

    /** The records of a log table ({@code options} as chloedb-server's {@code /v1/rank_log/export} takes them). */
    @SuppressWarnings("unchecked")
    public List<Map<String, Object>> exportRankLog(Map<String, ?> options) {
        return (List<Map<String, Object>>) call("exportRankLog", "options", options);
    }

    /**
     * Applies write operations as chloedb-server's {@code /v1/write} takes
     * them, in one transaction: maps with {@code op} ({@code insert},
     * {@code upsert}, {@code delete}, {@code set}, {@code add}), and
     * {@code doc}, {@code key}, {@code field}, {@code value} and
     * {@code table} as it needs. Returns the rows written.
     */
    public long write(List<? extends Map<String, ?>> ops) {
        return (Long) call("write", "ops", ops);
    }

    /** Deletes the rows whose {@code expire_by} time has passed, in every table that has one; returns how many. */
    public long expire() {
        return (Long) call("expire");
    }

    /**
     * Imports the tables of the SQLite file {@code path}. {@code options}:
     * {@code tables}, {@code schemas}, {@code blobsBase64}. Returns what it
     * did.
     */
    @SuppressWarnings("unchecked")
    public Map<String, Object> importSqlite(String path, Map<String, ?> options) {
        return (Map<String, Object>) call("importSqlite", args(options, "path", path));
    }

    public Map<String, Object> importSqlite(String path) {
        return importSqlite(path, null);
    }

    /**
     * Writes the tables to a new SQLite file. {@code options}:
     * {@code tables}, {@code fts} (full-text tables, true),
     * {@code indexes} (true), {@code schemas} (ChloeDB's schemas, true).
     * Returns what it wrote.
     */
    @SuppressWarnings("unchecked")
    public Map<String, Object> exportSqlite(String path, Map<String, ?> options) {
        return (Map<String, Object>) call("exportSqlite", args(options, "path", path));
    }

    public Map<String, Object> exportSqlite(String path) {
        return exportSqlite(path, null);
    }

    /**
     * Imports the Parquet file or directory {@code path} (or {@code s3://}
     * URL) into one table. {@code options}: {@code table}, {@code schema},
     * {@code key}, {@code blobsBase64}. Returns what it did.
     */
    @SuppressWarnings("unchecked")
    public Map<String, Object> importParquet(String path, Map<String, ?> options) {
        return (Map<String, Object>) call("importParquet", args(options, "path", path));
    }

    public Map<String, Object> importParquet(String path) {
        return importParquet(path, null);
    }

    /**
     * Writes the tables to Parquet: a {@code .parquet} file for one table,
     * else a directory of them; {@code path} may be an {@code s3://} URL.
     * {@code options}: {@code tables}, {@code schemas} (true). Returns what
     * it wrote.
     */
    @SuppressWarnings("unchecked")
    public Map<String, Object> exportParquet(String path, Map<String, ?> options) {
        return (Map<String, Object>) call("exportParquet", args(options, "path", path));
    }

    public Map<String, Object> exportParquet(String path) {
        return exportParquet(path, null);
    }

    /**
     * Readies a table that is done changing for queries: writes the rows
     * still in memory into segments, merges its segments into as few as it
     * can (one, up to 2,000,000 rows; a query visits each), drops deleted
     * rows, and returns the space this frees to the file system. Segments
     * merge in the background anyway; this is for after a load.
     */
    public void optimize() {
        call("optimize");
    }

    /**
     * Opened {@link Options#readOnly}: reads what the writer committed since
     * the last query, now (each query does so by itself); whether anything
     * changed. False for the writer.
     */
    public boolean refresh() {
        return (Boolean) call("refresh");
    }

    /**
     * Merges small segments, returns the space merges freed to the file
     * system when it is an eighth of the file or more, and closes the
     * database. Calls after it throw
     * {@link IllegalStateException}; closing again does nothing.
     */
    @Override
    public void close() {
        call("close");
    }

    public boolean isClosed() {
        return (Boolean) call("closed");
    }

    // The first table's

    /** The first table's schema; see {@link Table#schema()}. */
    public Map<String, Object> schema() {
        return first.schema();
    }

    /** See {@link Table#insert(Map)}. */
    public long insert(Map<String, ?> row) {
        return first.insert(row);
    }

    /** See {@link Table#insert(Iterable)}. */
    public long insert(Iterable<? extends Map<String, ?>> rows) {
        return first.insert(rows);
    }

    /** See {@link Table#upsert(Map)}. */
    public long upsert(Map<String, ?> row) {
        return first.upsert(row);
    }

    /** See {@link Table#upsert(Iterable)}. */
    public long upsert(Iterable<? extends Map<String, ?>> rows) {
        return first.upsert(rows);
    }

    /** See {@link Table#delete(Object)}. */
    public long delete(Object key) {
        return first.delete(key);
    }

    /** See {@link Table#delete(Collection)}. */
    public long delete(Collection<?> keys) {
        return first.delete(keys);
    }

    /** See {@link Table#get(Object)}. */
    public Map<String, Object> get(Object key) {
        return first.get(key);
    }

    /** See {@link Table#getMany(Collection)}. */
    public List<Map<String, Object>> getMany(Collection<?> keys) {
        return first.getMany(keys);
    }

    /** See {@link Table#addColumn(String, String)}. */
    public void addColumn(String name, String type) {
        first.addColumn(name, type);
    }

    /** See {@link Table#addColumn(String, String, Map)}. */
    public void addColumn(String name, String type, Map<String, ?> options) {
        first.addColumn(name, type, options);
    }

    /** See {@link Table#dropColumn(String)}. */
    public void dropColumn(String name) {
        first.dropColumn(name);
    }

    /** See {@link Table#renameColumn(String, String)}. */
    public void renameColumn(String from, String to) {
        first.renameColumn(from, to);
    }

    /** See {@link Table#importFile(String)}. */
    public Map<String, Object> importFile(String path) {
        return first.importFile(path);
    }

    /** See {@link Table#importFile(String, String)}. */
    public Map<String, Object> importFile(String path, String format) {
        return first.importFile(path, format);
    }

    /** See {@link Table#search(String)}. */
    public Result search(String text) {
        return first.search(text);
    }

    /** See {@link Table#search(String, SearchOptions)}. */
    public Result search(String text, SearchOptions options) {
        return first.search(text, options);
    }

    /** See {@link Table#embed(String)}. */
    public float[] embed(String text) {
        return first.embed(text);
    }

    /** See {@link Table#embed(String, String)}. */
    public float[] embed(String text, String field) {
        return first.embed(text, field);
    }

    /** See {@link Table#embedTokens(String, String)}. */
    public List<float[]> embedTokens(String text, String field) {
        return first.embedTokens(text, field);
    }

    @Override
    public String toString() {
        return "Database(" + path + ")";
    }
}
