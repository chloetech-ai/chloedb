package ai.chloetech.strata.jdbc;

import ai.chloetech.strata.Result;
import java.sql.Connection;
import java.sql.ResultSet;
import java.sql.SQLException;
import java.sql.SQLWarning;
import java.sql.Statement;
import java.util.ArrayList;
import java.util.Collections;
import java.util.List;
import java.util.Map;

/** A statement run on a ChloeDB database: SQL as ChloeDB speaks it, and {@code :name} parameters through {@link StrataPreparedStatement}. */
public class StrataStatement implements Statement {
    final StrataConnection conn;
    /** The kind of result sets: forward-only, or scroll-insensitive. */
    int type = ResultSet.TYPE_FORWARD_ONLY;
    private boolean closed;
    private boolean closeOnCompletion;
    private StrataResultSet results;
    private long updateCount = -1;
    private long maxRows;
    private int fetchSize;
    private int queryTimeout;
    private final List<String> batch = new ArrayList<>();

    StrataStatement(StrataConnection conn) {
        this.conn = conn;
    }

    void check() throws SQLException {
        if (closed) {
            throw new SQLException("the statement is closed", "HY010");
        }
        if (conn.isClosed()) {
            throw new SQLException("the connection is closed", "08003");
        }
    }

    /** Runs {@code sql} with its parameters; true if it answered rows. */
    boolean run(String sql, Map<String, Object> params) throws SQLException {
        check();
        clear();
        if (Sql.answersRows(sql)) {
            results = select(sql, params);
            return true;
        }
        updateCount = write(sql, params);
        return false;
    }

    StrataResultSet select(String sql, Map<String, Object> params) throws SQLException {
        check();
        clear();
        Result r;
        try {
            r = conn.db().query(sql, params);
        } catch (RuntimeException e) {
            throw Sql.error(e);
        }
        List<List<Object>> rows = r.rows();
        if (maxRows > 0 && rows.size() > maxRows) {
            rows = rows.subList(0, (int) maxRows);
        }
        results = new StrataResultSet(this, r.columns(), rows);
        return results;
    }

    long write(String sql, Map<String, Object> params) throws SQLException {
        check();
        clear();
        if (conn.isReadOnly()) {
            throw new SQLException("the connection is read-only: it runs no statement that writes", "25006");
        }
        try {
            updateCount = conn.db().execute(sql, params);
        } catch (RuntimeException e) {
            throw Sql.error(e);
        }
        return updateCount;
    }

    /** Closes the results of the last statement run. */
    private void clear() throws SQLException {
        if (results != null) {
            StrataResultSet r = results;
            results = null;
            r.closeQuietly();
        }
        updateCount = -1;
    }

    /** Called by a result set as it closes, for {@link #closeOnCompletion()}. */
    void resultsClosed(StrataResultSet r) {
        if (r == results) {
            results = null;
        }
        if (closeOnCompletion && results == null) {
            closed = true;
        }
    }

    static int toInt(long n) {
        return n > Integer.MAX_VALUE ? Statement.SUCCESS_NO_INFO : (int) n;
    }

    @Override
    public ResultSet executeQuery(String sql) throws SQLException {
        if (!Sql.answersRows(sql)) {
            throw new SQLException("executeQuery runs a SELECT, WITH or EXPLAIN; other statements go to executeUpdate or execute", "07000");
        }
        return select(sql, null);
    }

    @Override
    public int executeUpdate(String sql) throws SQLException {
        return toInt(executeLargeUpdate(sql));
    }

    @Override
    public long executeLargeUpdate(String sql) throws SQLException {
        if (Sql.answersRows(sql)) {
            throw new SQLException("executeUpdate runs a statement that writes; a SELECT goes to executeQuery", "07000");
        }
        return write(sql, null);
    }

    @Override
    public boolean execute(String sql) throws SQLException {
        return run(sql, null);
    }

    @Override
    public void close() throws SQLException {
        if (!closed) {
            clear();
            closed = true;
        }
    }

    @Override
    public boolean isClosed() {
        return closed;
    }

    @Override
    public int getMaxFieldSize() {
        return 0;
    }

    @Override
    public void setMaxFieldSize(int max) {}

    @Override
    public int getMaxRows() {
        return maxRows > Integer.MAX_VALUE ? 0 : (int) maxRows;
    }

    @Override
    public void setMaxRows(int max) throws SQLException {
        setLargeMaxRows(max);
    }

    @Override
    public long getLargeMaxRows() {
        return maxRows;
    }

    @Override
    public void setLargeMaxRows(long max) throws SQLException {
        if (max < 0) {
            throw new SQLException("maxRows is 0 (no limit) or more, not " + max, "HY024");
        }
        maxRows = max;
    }

    @Override
    public void setEscapeProcessing(boolean enable) {}

    @Override
    public int getQueryTimeout() {
        return queryTimeout;
    }

    /** Taken and kept, but statements run to the end: ChloeDB's queries take no deadline yet. */
    @Override
    public void setQueryTimeout(int seconds) throws SQLException {
        if (seconds < 0) {
            throw new SQLException("a timeout is 0 or more seconds, not " + seconds, "HY024");
        }
        queryTimeout = seconds;
    }

    @Override
    public void cancel() throws SQLException {
        throw Sql.unsupported("cancelling a statement");
    }

    @Override
    public SQLWarning getWarnings() {
        return null;
    }

    @Override
    public void clearWarnings() {}

    @Override
    public void setCursorName(String name) throws SQLException {
        throw Sql.unsupported("named cursors");
    }

    @Override
    public ResultSet getResultSet() {
        return results;
    }

    @Override
    public int getUpdateCount() {
        return updateCount < 0 ? -1 : toInt(updateCount);
    }

    @Override
    public long getLargeUpdateCount() {
        return updateCount;
    }

    @Override
    public boolean getMoreResults() throws SQLException {
        clear();
        return false;
    }

    @Override
    public boolean getMoreResults(int current) throws SQLException {
        return getMoreResults();
    }

    @Override
    public void setFetchDirection(int direction) throws SQLException {
        if (direction != ResultSet.FETCH_FORWARD && type == ResultSet.TYPE_FORWARD_ONLY) {
            throw new SQLException("the statement's result sets are forward-only", "HY106");
        }
    }

    @Override
    public int getFetchDirection() {
        return ResultSet.FETCH_FORWARD;
    }

    /** Taken and kept: a query's rows all come back at once. */
    @Override
    public void setFetchSize(int rows) throws SQLException {
        if (rows < 0) {
            throw new SQLException("a fetch size is 0 or more, not " + rows, "HY024");
        }
        fetchSize = rows;
    }

    @Override
    public int getFetchSize() {
        return fetchSize;
    }

    @Override
    public int getResultSetConcurrency() {
        return ResultSet.CONCUR_READ_ONLY;
    }

    @Override
    public int getResultSetType() {
        return type;
    }

    @Override
    public void addBatch(String sql) throws SQLException {
        check();
        batch.add(sql);
    }

    @Override
    public void clearBatch() throws SQLException {
        check();
        batch.clear();
    }

    @Override
    public int[] executeBatch() throws SQLException {
        long[] n = executeLargeBatch();
        int[] out = new int[n.length];
        for (int i = 0; i < n.length; i++) {
            out[i] = toInt(n[i]);
        }
        return out;
    }

    /** Runs each statement in turn; one that fails stops the batch, and the ones before it stay written. */
    @Override
    public long[] executeLargeBatch() throws SQLException {
        check();
        List<String> todo = new ArrayList<>(batch);
        batch.clear();
        long[] counts = new long[todo.size()];
        for (int i = 0; i < todo.size(); i++) {
            String sql = todo.get(i);
            if (Sql.answersRows(sql)) {
                throw Batch.failed(counts, i, new SQLException("a batch runs statements that write, not a SELECT", "07000"));
            }
            try {
                counts[i] = write(sql, null);
            } catch (SQLException e) {
                throw Batch.failed(counts, i, e);
            }
        }
        return counts;
    }

    @Override
    public Connection getConnection() {
        return conn;
    }

    @Override
    public ResultSet getGeneratedKeys() throws SQLException {
        check();
        return new StrataResultSet(this, Collections.emptyList(), Collections.emptyList());
    }

    @Override
    public int executeUpdate(String sql, int autoGeneratedKeys) throws SQLException {
        return executeUpdate(sql);
    }

    @Override
    public int executeUpdate(String sql, int[] columnIndexes) throws SQLException {
        return executeUpdate(sql);
    }

    @Override
    public int executeUpdate(String sql, String[] columnNames) throws SQLException {
        return executeUpdate(sql);
    }

    @Override
    public long executeLargeUpdate(String sql, int autoGeneratedKeys) throws SQLException {
        return executeLargeUpdate(sql);
    }

    @Override
    public long executeLargeUpdate(String sql, int[] columnIndexes) throws SQLException {
        return executeLargeUpdate(sql);
    }

    @Override
    public long executeLargeUpdate(String sql, String[] columnNames) throws SQLException {
        return executeLargeUpdate(sql);
    }

    @Override
    public boolean execute(String sql, int autoGeneratedKeys) throws SQLException {
        return execute(sql);
    }

    @Override
    public boolean execute(String sql, int[] columnIndexes) throws SQLException {
        return execute(sql);
    }

    @Override
    public boolean execute(String sql, String[] columnNames) throws SQLException {
        return execute(sql);
    }

    @Override
    public int getResultSetHoldability() {
        return ResultSet.CLOSE_CURSORS_AT_COMMIT;
    }

    @Override
    public void setPoolable(boolean poolable) {}

    @Override
    public boolean isPoolable() {
        return false;
    }

    @Override
    public void closeOnCompletion() throws SQLException {
        check();
        closeOnCompletion = true;
    }

    @Override
    public boolean isCloseOnCompletion() {
        return closeOnCompletion;
    }

    @Override
    public <T> T unwrap(Class<T> iface) throws SQLException {
        if (iface.isInstance(this)) {
            return iface.cast(this);
        }
        throw new SQLException("a ChloeDB statement does not unwrap to a " + iface.getName());
    }

    @Override
    public boolean isWrapperFor(Class<?> iface) {
        return iface.isInstance(this);
    }
}
