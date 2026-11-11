package ai.chloetech.chloedb;

import java.util.ArrayList;
import java.util.Collection;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Set;
import java.util.TreeSet;

/**
 * One table of a {@link Database}: its rows by key, its columns, and search
 * over it. {@link Database#table(String)} gives one; the database's own
 * methods of the same names act on its first table.
 *
 * <p>Rows are maps of field name to value: {@code String}, {@code Long} or
 * {@code Integer}, {@code Double}, {@code Boolean}, {@code null}, a
 * {@code List} for a multi-valued field, a {@code float[]} for a vector, a
 * map of term weights for a sparse field, a map (or JSON text) for a JSON
 * field, and an {@code Instant} (or another {@code java.time} type, or ISO
 * 8601 text) for a timestamp.
 */
public final class Table {
    private final Database db;
    /** {@code null}: the first table. */
    private final String name;

    Table(Database db, String name) {
        this.db = db;
        this.name = name;
    }

    private Object call(String method, Object... kv) {
        Map<String, Object> args = Database.args(null, kv);
        args.put("table", name);
        return db.call(method, args);
    }

    private Object call(String method, Map<String, ?> options, Object... kv) {
        Map<String, Object> args = Database.args(options, kv);
        args.put("table", name);
        return db.call(method, args);
    }

    /** The table's name. */
    public String name() {
        return name != null ? name : (String) schema().get("table");
    }

    /**
     * The table's schema in its short form: {@code table}, {@code key}, the
     * table's options and {@code fields}, each a map with {@code name},
     * {@code type} and its options.
     */
    @SuppressWarnings("unchecked")
    public Map<String, Object> schema() {
        return (Map<String, Object>) call("schema");
    }

    /** Inserts a row; its key must be new. Returns 1. */
    public long insert(Map<String, ?> row) {
        return insert(Collections.singletonList(row));
    }

    /**
     * Inserts rows; their keys must be new. All or none are written: 10,000
     * rows or more are bulk-loaded into new segments in one commit, fewer go
     * through the write-ahead log. Returns the rows written.
     */
    public long insert(Iterable<? extends Map<String, ?>> rows) {
        return (Long) call("insert", "rows", rows);
    }

    /** Inserts a row, or replaces the row with its key. Returns 1. */
    public long upsert(Map<String, ?> row) {
        return upsert(Collections.singletonList(row));
    }

    /** Inserts rows or replaces the rows with their keys, in transactions of 10,000 rows. Returns the rows written. */
    public long upsert(Iterable<? extends Map<String, ?>> rows) {
        return (Long) call("upsert", "rows", rows);
    }

    /** Deletes the row with this key; returns 1, or 0 if there was none. */
    public long delete(Object key) {
        return delete(Collections.singletonList(key));
    }

    /** Deletes the rows with these keys; returns how many there were. */
    public long delete(Collection<?> keys) {
        return (Long) call("delete", "keys", keys);
    }

    /** The row with this key as a map of the columns {@code SELECT *} answers, or {@code null} if there is none. */
    public Map<String, Object> get(Object key) {
        return getMany(Collections.singletonList(key)).get(0);
    }

    /**
     * The rows with these keys, read by key without SQL: per key, in their
     * order, a map of the columns {@code SELECT *} answers, or {@code null}
     * for a key no live row has (or whose row has expired).
     */
    @SuppressWarnings("unchecked")
    public List<Map<String, Object>> getMany(Collection<?> keys) {
        return (List<Map<String, Object>>) call("getMany", "keys", keys);
    }

    /** Adds a column of {@code type} (keyword, text, int, float, bool, timestamp, json, vector...). */
    public void addColumn(String name, String type) {
        addColumn(name, type, Collections.<String, Object>emptyMap());
    }

    /** Adds a column of {@code type} with its options, as the schema's short form names them ({@code "dim"}, {@code "multi"}, ...). */
    public void addColumn(String name, String type, Map<String, ?> options) {
        Map<String, Object> field = new LinkedHashMap<>();
        field.put("name", name);
        field.put("type", type);
        field.putAll(options);
        alter(Collections.singletonMap("add", field));
    }

    /** Drops a column. */
    public void dropColumn(String name) {
        alter(Collections.singletonMap("drop", name));
    }

    /** Renames a column. */
    public void renameColumn(String from, String to) {
        List<String> names = new ArrayList<>();
        names.add(from);
        names.add(to);
        alter(Collections.singletonMap("rename", names));
    }

    private void alter(Map<String, ?> change) {
        call("alterTable", "changes", Collections.singletonList(change));
    }

    /**
     * Loads a JSON Lines, JSON, CSV or TSV file, its format told by its
     * name. Returns {@code rows} (written), {@code bulk} (whether they were
     * bulk-loaded) and {@code skipped} (fields of the file the table does
     * not have).
     */
    public Map<String, Object> importFile(String path) {
        return importFile(path, null);
    }

    /** Loads a file of {@code format}: {@code "jsonl"}, {@code "json"}, {@code "csv"} or {@code "tsv"}. */
    @SuppressWarnings("unchecked")
    public Map<String, Object> importFile(String path, String format) {
        return (Map<String, Object>) call("importFile", "path", path, "format", format);
    }

    /** The rows that best match {@code text}: 10, by text and vector as {@link SearchOptions#mode} says. */
    public Result search(String text) {
        return search(text, new SearchOptions());
    }

    /** The rows that best match {@code text}, as {@code options} say. */
    @SuppressWarnings("unchecked")
    public Result search(String text, SearchOptions options) {
        Map<String, Object> s = schema();
        List<Map<String, Object>> fields = (List<Map<String, Object>>) s.get("fields");
        List<String> texts = options.textFields;
        if (texts == null) {
            List<String> indexed = new ArrayList<>();
            List<String> generated = new ArrayList<>();
            for (Map<String, Object> f : fields) {
                if ("text".equals(f.get("type")) && !Boolean.FALSE.equals(f.get("indexed"))) {
                    indexed.add((String) f.get("name"));
                    if (f.get("from") != null) {
                        generated.add((String) f.get("name"));
                    }
                }
            }
            texts = generated.isEmpty() ? indexed : generated;
        }
        String vector = options.vectorField;
        if (vector == null) {
            Map<String, String> broken = db.embedErrors();
            for (Map<String, Object> f : fields) {
                if (f.get("embed") != null && !broken.containsKey((String) f.get("name"))) {
                    vector = (String) f.get("name");
                    break;
                }
            }
        }
        String mode = options.mode;
        if (mode.equals("auto")) {
            mode = vector != null && !texts.isEmpty() ? "hybrid" : vector != null ? "vector" : "text";
        }
        List<String> scores = new ArrayList<>();
        if (mode.equals("text") || mode.equals("hybrid")) {
            if (texts.isEmpty()) {
                throw new ChloeDBException(s.get("table") + " has no indexed text field to search");
            }
            for (String f : texts) {
                scores.add("bm25(" + quote(f) + ", :_q)");
            }
        }
        if (mode.equals("vector") || mode.equals("hybrid")) {
            if (vector == null) {
                throw new ChloeDBException(s.get("table") + " has no embedded vector field with its model open");
            }
            scores.add("similarity(" + quote(vector) + ", :_v)");
        }
        String cols = "*";
        if (options.columns != null) {
            Set<String> names = new TreeSet<>();
            for (Map<String, Object> f : fields) {
                names.add(((String) f.get("name")).toLowerCase(Locale.ROOT));
            }
            List<String> out = new ArrayList<>();
            for (String c : options.columns) {
                String t = c.trim();
                // The names of fields quoted, so that any name works; other
                // columns (score(), an expression) as written.
                out.add(names.contains(t.toLowerCase(Locale.ROOT)) ? quote(t) : t);
            }
            cols = String.join(", ", out);
        }
        String order = scores.size() == 1 ? scores.get(0) : "rrf(" + String.join(", ", scores) + ")";
        StringBuilder sql = new StringBuilder("SELECT ").append(cols).append(" FROM ").append(s.get("table"));
        if (options.where != null) {
            sql.append(" WHERE ").append(options.where);
        }
        sql.append(" ORDER BY ").append(order).append(" DESC LIMIT ").append(options.limit);
        Map<String, Object> params = new LinkedHashMap<>(options.params);
        if (order.contains("bm25")) {
            params.put("_q", text);
        }
        if (order.contains("similarity")) {
            Map<String, Object> e = new LinkedHashMap<>();
            e.put("embed", text);
            e.put("field", vector);
            params.put("_v", e);
        }
        return db.query(sql.toString(), params, options.limits);
    }

    /** A name as an SQL identifier, whatever it is. */
    static String quote(String name) {
        return '"' + name.replace("\"", "\"\"") + '"';
    }

    /** {@code text}'s vector as a query, by the model of the first embedded field whose model is open. */
    public float[] embed(String text) {
        return embed(text, null);
    }

    /** {@code text}'s vector as a query, by the model of the vector field {@code field}. */
    public float[] embed(String text, String field) {
        return (float[]) call("embed", "text", text, "field", field);
    }

    /** An image's vector (the bytes of its file), by the model of {@code field}, or of the first field with a model of images. */
    public float[] embedImage(byte[] image, String field) {
        return (float[]) call("embed", "image", image, "field", field);
    }

    /** A sound's vector (the bytes of its file), by the model of {@code field}, or of the first field with a model of sound. */
    public float[] embedAudio(byte[] audio, String field) {
        return (float[]) call("embed", "audio", audio, "field", field);
    }

    /** The vectors of {@code text}'s tokens as a query, by the model of the multi-vector field {@code field} (ColBERT), as {@code maxsim} takes them. */
    @SuppressWarnings("unchecked")
    public List<float[]> embedTokens(String text, String field) {
        return (List<float[]>) (List<?>) call("embedTokens", "text", text, "field", field);
    }

    /** The codes that can follow each of {@code prefixes} in the semantic-ID field {@code field}, with their rows. */
    public Map<String, Map<String, Long>> sidNext(String field, Collection<String> prefixes) {
        return sidNext(field, prefixes, null, Collections.<String, Object>emptyMap());
    }

    /** As {@link #sidNext(String, Collection)}, among the rows the SQL condition {@code where} matches. */
    @SuppressWarnings("unchecked")
    public Map<String, Map<String, Long>> sidNext(String field, Collection<String> prefixes, String where, Map<String, ?> params) {
        return (Map<String, Map<String, Long>>) (Map<?, ?>) call("sidNext", "field", field, "prefixes", prefixes, "where", where, "params", params);
    }

    /**
     * Trains a semantic-ID codebook on the vectors of {@code field}, saves it
     * as {@code name} under the models directory, and returns its SHA-256.
     * {@code options}: {@code models} (a directory), {@code levels} (3),
     * {@code k} (256), {@code iterations} (25), {@code sample} and
     * {@code seed} (0).
     */
    public String trainCodebook(String field, String name, Map<String, ?> options) {
        return (String) call("trainCodebook", options, "field", field, "name", name);
    }

    @Override
    public String toString() {
        return "Table(" + (name != null ? name : "the first") + ")";
    }
}
