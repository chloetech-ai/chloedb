package ai.chloetech.chloedb;

import java.util.ArrayList;
import java.util.Arrays;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * How {@link Table#search(String, SearchOptions)} searches: how many rows,
 * which columns, a condition, and by what (text, vector, or both).
 *
 * <pre>{@code
 * db.search("dragon", new SearchOptions().limit(5).where("year >= :y").param("y", 1950).columns("id", "year"));
 * }</pre>
 */
public final class SearchOptions {
    int limit = 10;
    List<String> columns;
    String where;
    String mode = "auto";
    List<String> textFields;
    String vectorField;
    final Map<String, Object> params = new LinkedHashMap<>();
    QueryOptions limits;

    /** The search's deadline and memory budget, as a query's ({@link QueryOptions}). */
    public SearchOptions limits(QueryOptions limits) {
        this.limits = limits;
        return this;
    }

    /** The rows to answer, 10 unless set. */
    public SearchOptions limit(int limit) {
        if (limit < 0) {
            throw new IllegalArgumentException("limit: 0 or more");
        }
        this.limit = limit;
        return this;
    }

    /** The columns to answer (field names, or expressions such as {@code score()}); every field unless set. */
    public SearchOptions columns(String... columns) {
        this.columns = Collections.unmodifiableList(new ArrayList<>(Arrays.asList(columns)));
        return this;
    }

    /** An SQL condition the rows must meet, with {@code :name} placeholders bound by {@link #param}. */
    public SearchOptions where(String condition) {
        this.where = condition == null || condition.trim().isEmpty() ? null : condition;
        return this;
    }

    /** Binds a placeholder of {@link #where}. */
    public SearchOptions param(String name, Object value) {
        params.put(name, value);
        return this;
    }

    /** Binds placeholders of {@link #where}. */
    public SearchOptions params(Map<String, ?> params) {
        this.params.putAll(params);
        return this;
    }

    /**
     * {@code "text"} ranks by BM25 over the text fields, {@code "vector"} by
     * the similarity of an embedded vector field to the text's embedding,
     * and {@code "hybrid"} fuses both by reciprocal rank (RRF).
     * {@code "auto"}, the default, is hybrid when the table has an embedded
     * field whose model is open, and text otherwise.
     */
    public SearchOptions mode(String mode) {
        if (!Arrays.asList("auto", "text", "vector", "hybrid").contains(mode)) {
            throw new IllegalArgumentException("mode is \"auto\", \"text\", \"vector\" or \"hybrid\", not " + mode);
        }
        this.mode = mode;
        return this;
    }

    /**
     * The text fields to rank by BM25. Unless set: the text fields generated
     * from others if there are any (a combined "body"), else every indexed
     * text field.
     */
    public SearchOptions textFields(String... fields) {
        this.textFields = Collections.unmodifiableList(new ArrayList<>(Arrays.asList(fields)));
        return this;
    }

    /** The embedded vector field to rank by; unless set, the first whose model is open. */
    public SearchOptions vectorField(String field) {
        this.vectorField = field;
        return this;
    }
}
