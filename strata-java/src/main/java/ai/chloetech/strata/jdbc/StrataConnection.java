package ai.chloetech.strata.jdbc;

import ai.chloetech.strata.Database;
import java.sql.Array;
import java.sql.Blob;
import java.sql.CallableStatement;
import java.sql.Clob;
import java.sql.Connection;
import java.sql.DatabaseMetaData;
import java.sql.NClob;
import java.sql.PreparedStatement;
import java.sql.ResultSet;
import java.sql.SQLClientInfoException;
import java.sql.SQLException;
import java.sql.SQLWarning;
import java.sql.SQLXML;
import java.sql.Savepoint;
import java.sql.Statement;
import java.sql.Struct;
import java.util.Collections;
import java.util.Map;
import java.util.Properties;
import java.util.concurrent.Executor;

/**
 * A connection to a ChloeDB database in this process. Each statement
 * commits as it runs (auto-commit, which cannot be turned off).
 */
public final class StrataConnection implements Connection {
    private final Shared.Entry shared;
    private final String url;
    private volatile boolean closed;
    private boolean readOnly;
    private final Properties clientInfo = new Properties();

    StrataConnection(Shared.Entry shared, String url) {
        this.shared = shared;
        this.url = url;
    }

    Database db() throws SQLException {
        check();
        return shared.db;
    }

    String url() {
        return url;
    }

    private void check() throws SQLException {
        if (closed) {
            throw new SQLException("the connection is closed", "08003");
        }
    }

    @Override
    public StrataStatement createStatement() throws SQLException {
        check();
        return new StrataStatement(this);
    }

    @Override
    public StrataPreparedStatement prepareStatement(String sql) throws SQLException {
        check();
        return new StrataPreparedStatement(this, sql);
    }

    @Override
    public CallableStatement prepareCall(String sql) throws SQLException {
        throw Sql.unsupported("stored procedures");
    }

    @Override
    public String nativeSQL(String sql) {
        return sql;
    }

    @Override
    public void setAutoCommit(boolean autoCommit) throws SQLException {
        check();
        if (!autoCommit) {
            throw Sql.unsupported("transactions across statements: each statement commits as it runs");
        }
    }

    @Override
    public boolean getAutoCommit() throws SQLException {
        check();
        return true;
    }

    @Override
    public void commit() throws SQLException {
        check();
        throw new SQLException("each statement commits as it runs (auto-commit): there is nothing to commit", "25000");
    }

    @Override
    public void rollback() throws SQLException {
        check();
        throw new SQLException("each statement commits as it runs (auto-commit): there is nothing to roll back", "25000");
    }

    /** Lets go of the database; the last connection to a file closes it. */
    @Override
    public void close() {
        if (!closed) {
            closed = true;
            shared.release();
        }
    }

    @Override
    public boolean isClosed() {
        return closed;
    }

    @Override
    public DatabaseMetaData getMetaData() throws SQLException {
        check();
        return new StrataDatabaseMetaData(this);
    }

    /** {@code true} refuses statements that write, through this connection. */
    @Override
    public void setReadOnly(boolean readOnly) throws SQLException {
        check();
        this.readOnly = readOnly;
    }

    @Override
    public boolean isReadOnly() throws SQLException {
        check();
        return readOnly;
    }

    @Override
    public void setCatalog(String catalog) {}

    @Override
    public String getCatalog() {
        return null;
    }

    /**
     * Taken and ignored, for pools that set one: there are no transactions
     * across statements, and each statement reads one snapshot and writes
     * atomically.
     */
    @Override
    public void setTransactionIsolation(int level) throws SQLException {
        check();
    }

    @Override
    public int getTransactionIsolation() throws SQLException {
        check();
        return TRANSACTION_NONE;
    }

    @Override
    public SQLWarning getWarnings() {
        return null;
    }

    @Override
    public void clearWarnings() {}

    @Override
    public Statement createStatement(int resultSetType, int resultSetConcurrency) throws SQLException {
        return kinds(createStatement(), resultSetType, resultSetConcurrency);
    }

    @Override
    public PreparedStatement prepareStatement(String sql, int resultSetType, int resultSetConcurrency) throws SQLException {
        return kinds(prepareStatement(sql), resultSetType, resultSetConcurrency);
    }

    @Override
    public CallableStatement prepareCall(String sql, int resultSetType, int resultSetConcurrency) throws SQLException {
        throw Sql.unsupported("stored procedures");
    }

    /**
     * Result sets are read-only, and forward-only or scroll-insensitive (a
     * query's rows all come back at once, so they scroll for free).
     */
    private static <S extends StrataStatement> S kinds(S s, int type, int concurrency) throws SQLException {
        if (type != ResultSet.TYPE_FORWARD_ONLY && type != ResultSet.TYPE_SCROLL_INSENSITIVE) {
            throw Sql.unsupported("TYPE_SCROLL_SENSITIVE result sets: they are TYPE_FORWARD_ONLY or TYPE_SCROLL_INSENSITIVE");
        }
        if (concurrency != ResultSet.CONCUR_READ_ONLY) {
            throw Sql.unsupported("updatable result sets: write rows with INSERT, UPDATE or DELETE");
        }
        s.type = type;
        return s;
    }

    @Override
    public Map<String, Class<?>> getTypeMap() {
        return Collections.emptyMap();
    }

    @Override
    public void setTypeMap(Map<String, Class<?>> map) throws SQLException {
        if (!map.isEmpty()) {
            throw Sql.unsupported("user-defined types");
        }
    }

    @Override
    public void setHoldability(int holdability) {}

    @Override
    public int getHoldability() {
        return ResultSet.CLOSE_CURSORS_AT_COMMIT;
    }

    @Override
    public Savepoint setSavepoint() throws SQLException {
        throw Sql.unsupported("savepoints");
    }

    @Override
    public Savepoint setSavepoint(String name) throws SQLException {
        throw Sql.unsupported("savepoints");
    }

    @Override
    public void rollback(Savepoint savepoint) throws SQLException {
        throw Sql.unsupported("savepoints");
    }

    @Override
    public void releaseSavepoint(Savepoint savepoint) throws SQLException {
        throw Sql.unsupported("savepoints");
    }

    @Override
    public Statement createStatement(int resultSetType, int resultSetConcurrency, int resultSetHoldability) throws SQLException {
        return kinds(createStatement(), resultSetType, resultSetConcurrency);
    }

    @Override
    public PreparedStatement prepareStatement(String sql, int resultSetType, int resultSetConcurrency, int resultSetHoldability) throws SQLException {
        return kinds(prepareStatement(sql), resultSetType, resultSetConcurrency);
    }

    @Override
    public CallableStatement prepareCall(String sql, int resultSetType, int resultSetConcurrency, int resultSetHoldability) throws SQLException {
        throw Sql.unsupported("stored procedures");
    }

    /** Statements give no generated keys: a row's key is the one it was written with. */
    @Override
    public PreparedStatement prepareStatement(String sql, int autoGeneratedKeys) throws SQLException {
        return prepareStatement(sql);
    }

    @Override
    public PreparedStatement prepareStatement(String sql, int[] columnIndexes) throws SQLException {
        return prepareStatement(sql);
    }

    @Override
    public PreparedStatement prepareStatement(String sql, String[] columnNames) throws SQLException {
        return prepareStatement(sql);
    }

    @Override
    public Clob createClob() throws SQLException {
        throw Sql.unsupported("CLOBs");
    }

    @Override
    public Blob createBlob() throws SQLException {
        throw Sql.unsupported("BLOBs");
    }

    @Override
    public NClob createNClob() throws SQLException {
        throw Sql.unsupported("NCLOBs");
    }

    @Override
    public SQLXML createSQLXML() throws SQLException {
        throw Sql.unsupported("SQLXML");
    }

    @Override
    public boolean isValid(int timeout) {
        return !closed;
    }

    @Override
    public void setClientInfo(String name, String value) {
        clientInfo.setProperty(name, value);
    }

    @Override
    public void setClientInfo(Properties properties) throws SQLClientInfoException {
        clientInfo.clear();
        clientInfo.putAll(properties);
    }

    @Override
    public String getClientInfo(String name) {
        return clientInfo.getProperty(name);
    }

    @Override
    public Properties getClientInfo() {
        Properties p = new Properties();
        p.putAll(clientInfo);
        return p;
    }

    @Override
    public Array createArrayOf(String typeName, Object[] elements) throws SQLException {
        throw Sql.unsupported("java.sql.Array: give a List, or a float[] for a vector, to setObject");
    }

    @Override
    public Struct createStruct(String typeName, Object[] attributes) throws SQLException {
        throw Sql.unsupported("structs");
    }

    @Override
    public void setSchema(String schema) {}

    @Override
    public String getSchema() {
        return null;
    }

    @Override
    public void abort(Executor executor) {
        close();
    }

    @Override
    public void setNetworkTimeout(Executor executor, int milliseconds) {}

    @Override
    public int getNetworkTimeout() {
        return 0;
    }

    /** {@code unwrap(Database.class)} gives the database, for what SQL does not do. */
    @Override
    public <T> T unwrap(Class<T> iface) throws SQLException {
        if (iface.isInstance(shared.db)) {
            return iface.cast(db());
        }
        if (iface.isInstance(this)) {
            return iface.cast(this);
        }
        throw new SQLException("a StrataConnection unwraps to a Database, not a " + iface.getName());
    }

    @Override
    public boolean isWrapperFor(Class<?> iface) {
        return iface.isInstance(shared.db) || iface.isInstance(this);
    }

    @Override
    public String toString() {
        return "StrataConnection(" + url + ")";
    }
}
