package ai.chloetech.chloedb;

import java.util.AbstractList;
import java.util.ArrayList;
import java.util.Iterator;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * A query's answer: its {@link #columns()}, its {@link #rows()} (lists of
 * values in column order), each row's primary key in {@link #keys()} and the
 * score it was ranked by in {@link #scores()}, and what the engine did in
 * {@link #stats()}.
 *
 * <p>Iterating gives each row as a map of column to value, in column order;
 * {@link #row(int)} is one of them. Values are {@code null}, {@code Boolean},
 * {@code Long} (INT), {@code Double} (FLOAT), {@code String} (TEXT, KEYWORD,
 * and JSON as its text), {@code float[]} (VECTOR), {@code List} (a
 * multi-valued field, a multi-vector field) and {@code Instant} (TIMESTAMP).
 */
public final class Result implements Iterable<Map<String, Object>> {
    private final List<String> columns;
    private final List<List<Object>> rows;
    private final List<Object> keys;
    private final double[] scores;
    private final String stats;
    private final String request;
    private final List<String> teams;

    @SuppressWarnings("unchecked")
    Result(Map<String, Object> m) {
        this.columns = (List<String>) (List<?>) m.get("columns");
        this.rows = (List<List<Object>>) (List<?>) m.get("rows");
        this.keys = (List<Object>) m.get("keys");
        List<Object> s = (List<Object>) m.get("scores");
        this.scores = new double[s.size()];
        for (int i = 0; i < scores.length; i++) {
            scores[i] = (Double) s.get(i);
        }
        this.stats = (String) m.get("stats");
        this.request = (String) m.get("request");
        this.teams = (List<String>) (List<?>) m.get("teams");
    }

    /** The answer's column names, as the SELECT names them. */
    public List<String> columns() {
        return columns;
    }

    /** Each row's values, in column order. */
    public List<List<Object>> rows() {
        return rows;
    }

    /** Each row's primary key ({@code null} in a table without one); empty for facet and count queries. */
    public List<Object> keys() {
        return keys;
    }

    /** The score each row was ordered by (0 for unordered rows); empty for facet and count queries. */
    public double[] scores() {
        return scores.clone();
    }

    /** What the engine did (segments, candidates, time per stage), as JSON text. */
    public String stats() {
        return stats;
    }

    /** A logged query's request id ({@link Database#queryLogged}); else {@code null}. */
    public String request() {
        return request;
    }

    /** An interleaved logged query's: per row, the name of the ranking that placed it; else {@code null}. */
    public List<String> teams() {
        return teams;
    }

    /** The number of rows. */
    public int size() {
        return rows.size();
    }

    public boolean isEmpty() {
        return rows.isEmpty();
    }

    /** Row {@code i} as a map of column to value. */
    public Map<String, Object> row(int i) {
        List<Object> r = rows.get(i);
        Map<String, Object> m = new LinkedHashMap<>();
        for (int c = 0; c < columns.size(); c++) {
            m.put(columns.get(c), r.get(c));
        }
        return m;
    }

    /** One column's values. */
    public List<Object> column(String name) {
        int c = columns.indexOf(name);
        if (c < 0) {
            throw new IllegalArgumentException("no column " + name + " in " + columns);
        }
        List<Object> out = new ArrayList<>(rows.size());
        for (List<Object> r : rows) {
            out.add(r.get(c));
        }
        return out;
    }

    /** The first column of the first row ({@code null} if there is none): a count, a sum. */
    public Object scalar() {
        return rows.isEmpty() || rows.get(0).isEmpty() ? null : rows.get(0).get(0);
    }

    /** Every row as a map of column to value. */
    public List<Map<String, Object>> maps() {
        return new AbstractList<Map<String, Object>>() {
            @Override
            public Map<String, Object> get(int i) {
                return row(i);
            }

            @Override
            public int size() {
                return rows.size();
            }
        };
    }

    @Override
    public Iterator<Map<String, Object>> iterator() {
        return maps().iterator();
    }

    @Override
    public String toString() {
        return "Result(" + rows.size() + (rows.size() == 1 ? " row: " : " rows: ") + String.join(", ", columns) + ")";
    }
}
