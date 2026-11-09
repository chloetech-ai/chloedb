package ai.chloetech.chloedb.jdbc;

import java.io.ByteArrayInputStream;
import java.io.InputStream;
import java.io.Reader;
import java.io.StringReader;
import java.math.BigDecimal;
import java.math.RoundingMode;
import java.net.MalformedURLException;
import java.net.URL;
import java.nio.charset.StandardCharsets;
import java.sql.Array;
import java.sql.Blob;
import java.sql.Clob;
import java.sql.Date;
import java.sql.NClob;
import java.sql.Ref;
import java.sql.ResultSet;
import java.sql.ResultSetMetaData;
import java.sql.RowId;
import java.sql.SQLException;
import java.sql.SQLWarning;
import java.sql.SQLXML;
import java.sql.Statement;
import java.sql.Time;
import java.sql.Timestamp;
import java.util.Calendar;
import java.util.HashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;

/**
 * The rows a statement answered. They all come back at once, so the result
 * set can scroll ({@code TYPE_SCROLL_INSENSITIVE}); it is read-only.
 */
public final class ChloeDBResultSet extends ReadOnlyResultSet {
    private final ChloeDBStatement statement;
    private final List<String> columns;
    private final List<List<Object>> rows;
    private final int type;
    private Map<String, Integer> byName;
    private ChloeDBResultSetMetaData meta;
    /** The row the cursor is on: -1 before the first, rows.size() after the last. */
    private int at = -1;
    private boolean wasNull;
    private boolean closed;
    private int fetchSize;

    ChloeDBResultSet(ChloeDBStatement statement, List<String> columns, List<List<Object>> rows) {
        this.statement = statement;
        this.columns = columns;
        this.rows = rows;
        this.type = statement == null ? TYPE_FORWARD_ONLY : statement.type;
    }

    private void check() throws SQLException {
        if (closed) {
            throw new SQLException("the result set is closed", "HY010");
        }
    }

    private void scrolls() throws SQLException {
        check();
        if (type == TYPE_FORWARD_ONLY) {
            throw new SQLException("the result set is forward-only: make the statement with ResultSet.TYPE_SCROLL_INSENSITIVE to scroll", "HY106");
        }
    }

    /** The value at column {@code i} (from 1) of the current row. */
    private Object at(int i) throws SQLException {
        check();
        if (at < 0 || at >= rows.size()) {
            throw new SQLException(rows.isEmpty() ? "the result has no rows" : at < 0 ? "call next() first: the cursor is before the first row" : "the cursor is after the last row", "24000");
        }
        if (i < 1 || i > columns.size()) {
            throw new SQLException("the result has " + columns.size() + " column" + (columns.size() == 1 ? "" : "s") + " (numbered from 1), not a column " + i, "07009");
        }
        Object v = rows.get(at).get(i - 1);
        wasNull = v == null;
        return v;
    }

    void closeQuietly() {
        closed = true;
    }

    // The cursor.

    @Override
    public boolean next() throws SQLException {
        check();
        if (at < rows.size()) {
            at++;
        }
        return at < rows.size();
    }

    @Override
    public boolean previous() throws SQLException {
        scrolls();
        if (at >= 0) {
            at--;
        }
        return at >= 0;
    }

    @Override
    public boolean absolute(int row) throws SQLException {
        scrolls();
        int n = rows.size();
        at = row > 0 ? Math.min(row - 1, n) : row < 0 ? Math.max(n + row, -1) : -1;
        return at >= 0 && at < n;
    }

    @Override
    public boolean relative(int by) throws SQLException {
        scrolls();
        long to = (long) at + by;
        at = (int) Math.max(-1, Math.min(to, rows.size()));
        return at >= 0 && at < rows.size();
    }

    @Override
    public boolean first() throws SQLException {
        return absolute(1);
    }

    @Override
    public boolean last() throws SQLException {
        return absolute(-1);
    }

    @Override
    public void beforeFirst() throws SQLException {
        scrolls();
        at = -1;
    }

    @Override
    public void afterLast() throws SQLException {
        scrolls();
        at = rows.size();
    }

    @Override
    public boolean isBeforeFirst() throws SQLException {
        check();
        return at < 0 && !rows.isEmpty();
    }

    @Override
    public boolean isAfterLast() throws SQLException {
        check();
        return at >= rows.size() && !rows.isEmpty();
    }

    @Override
    public boolean isFirst() throws SQLException {
        check();
        return at == 0 && !rows.isEmpty();
    }

    @Override
    public boolean isLast() throws SQLException {
        check();
        return at == rows.size() - 1 && !rows.isEmpty();
    }

    @Override
    public int getRow() throws SQLException {
        check();
        return at >= 0 && at < rows.size() ? at + 1 : 0;
    }

    @Override
    public void close() {
        if (!closed) {
            closed = true;
            if (statement != null) {
                statement.resultsClosed(this);
            }
        }
    }

    @Override
    public boolean isClosed() {
        return closed;
    }

    @Override
    public boolean wasNull() throws SQLException {
        check();
        return wasNull;
    }

    @Override
    public int findColumn(String label) throws SQLException {
        check();
        if (byName == null) {
            byName = new HashMap<>();
            // The first of two columns with one name, as JDBC asks.
            for (int i = columns.size() - 1; i >= 0; i--) {
                byName.put(columns.get(i), i + 1);
                byName.put(columns.get(i).toLowerCase(Locale.ROOT), i + 1);
            }
        }
        Integer i = byName.get(label);
        if (i == null) {
            i = byName.get(label.toLowerCase(Locale.ROOT));
        }
        if (i == null) {
            throw new SQLException("no column " + label + " in the result: it has " + String.join(", ", columns), "42S22");
        }
        return i;
    }

    @Override
    public ResultSetMetaData getMetaData() throws SQLException {
        check();
        if (meta == null) {
            meta = new ChloeDBResultSetMetaData(columns, rows);
        }
        return meta;
    }

    @Override
    public Statement getStatement() {
        return statement;
    }

    @Override
    public SQLWarning getWarnings() {
        return null;
    }

    @Override
    public void clearWarnings() {}

    @Override
    public String getCursorName() throws SQLException {
        throw Sql.unsupported("named cursors");
    }

    @Override
    public void setFetchDirection(int direction) throws SQLException {
        check();
        if (direction != FETCH_FORWARD && type == TYPE_FORWARD_ONLY) {
            throw new SQLException("the result set is forward-only", "HY106");
        }
    }

    @Override
    public int getFetchDirection() {
        return FETCH_FORWARD;
    }

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
    public int getType() {
        return type;
    }

    @Override
    public int getConcurrency() {
        return CONCUR_READ_ONLY;
    }

    @Override
    public int getHoldability() {
        return CLOSE_CURSORS_AT_COMMIT;
    }

    // Values by column number.

    @Override
    public Object getObject(int i) throws SQLException {
        return Values.object(at(i));
    }

    @Override
    public <T> T getObject(int i, Class<T> type) throws SQLException {
        return Values.as(at(i), type);
    }

    @Override
    public Object getObject(int i, Map<String, Class<?>> map) throws SQLException {
        return getObject(i);
    }

    @Override
    public String getString(int i) throws SQLException {
        return Values.string(at(i));
    }

    @Override
    public String getNString(int i) throws SQLException {
        return getString(i);
    }

    @Override
    public boolean getBoolean(int i) throws SQLException {
        Object v = at(i);
        return v != null && Values.getBoolean(v);
    }

    @Override
    public byte getByte(int i) throws SQLException {
        Object v = at(i);
        return v == null ? 0 : (byte) Values.getLong(v, "a byte", Byte.MIN_VALUE, Byte.MAX_VALUE);
    }

    @Override
    public short getShort(int i) throws SQLException {
        Object v = at(i);
        return v == null ? 0 : (short) Values.getLong(v, "a short", Short.MIN_VALUE, Short.MAX_VALUE);
    }

    @Override
    public int getInt(int i) throws SQLException {
        Object v = at(i);
        return v == null ? 0 : (int) Values.getLong(v, "an int", Integer.MIN_VALUE, Integer.MAX_VALUE);
    }

    @Override
    public long getLong(int i) throws SQLException {
        Object v = at(i);
        return v == null ? 0 : Values.getLong(v, "a long", Long.MIN_VALUE, Long.MAX_VALUE);
    }

    @Override
    public float getFloat(int i) throws SQLException {
        Object v = at(i);
        return v == null ? 0 : (float) Values.getDouble(v, "a float");
    }

    @Override
    public double getDouble(int i) throws SQLException {
        Object v = at(i);
        return v == null ? 0 : Values.getDouble(v, "a double");
    }

    @Override
    public BigDecimal getBigDecimal(int i) throws SQLException {
        Object v = at(i);
        return v == null ? null : Values.getBigDecimal(v);
    }

    @Override
    @Deprecated
    public BigDecimal getBigDecimal(int i, int scale) throws SQLException {
        BigDecimal d = getBigDecimal(i);
        return d == null ? null : d.setScale(scale, RoundingMode.HALF_UP);
    }

    @Override
    public byte[] getBytes(int i) throws SQLException {
        Object v = at(i);
        return v == null ? null : Values.getBytes(v);
    }

    @Override
    public Date getDate(int i) throws SQLException {
        Object v = at(i);
        return v == null ? null : Values.getDate(v);
    }

    /** The calendar is ignored: times are in UTC. */
    @Override
    public Date getDate(int i, Calendar cal) throws SQLException {
        return getDate(i);
    }

    @Override
    public Time getTime(int i) throws SQLException {
        Object v = at(i);
        return v == null ? null : Values.getTime(v);
    }

    @Override
    public Time getTime(int i, Calendar cal) throws SQLException {
        return getTime(i);
    }

    @Override
    public Timestamp getTimestamp(int i) throws SQLException {
        Object v = at(i);
        return v == null ? null : Values.getTimestamp(v);
    }

    @Override
    public Timestamp getTimestamp(int i, Calendar cal) throws SQLException {
        return getTimestamp(i);
    }

    @Override
    public InputStream getAsciiStream(int i) throws SQLException {
        String s = getString(i);
        return s == null ? null : new ByteArrayInputStream(s.getBytes(StandardCharsets.US_ASCII));
    }

    @Override
    @Deprecated
    public InputStream getUnicodeStream(int i) throws SQLException {
        throw Sql.unsupported("getUnicodeStream (deprecated): use getCharacterStream");
    }

    @Override
    public InputStream getBinaryStream(int i) throws SQLException {
        byte[] b = getBytes(i);
        return b == null ? null : new ByteArrayInputStream(b);
    }

    @Override
    public Reader getCharacterStream(int i) throws SQLException {
        String s = getString(i);
        return s == null ? null : new StringReader(s);
    }

    @Override
    public Reader getNCharacterStream(int i) throws SQLException {
        return getCharacterStream(i);
    }

    /** A vector or a list as a {@link java.sql.Array}. */
    @Override
    public Array getArray(int i) throws SQLException {
        Object v = at(i);
        return v == null ? null : new ChloeDBArray(v);
    }

    @Override
    public URL getURL(int i) throws SQLException {
        String s = getString(i);
        try {
            return s == null ? null : new URL(s);
        } catch (MalformedURLException e) {
            throw new SQLException("not a URL: " + s, "22018", e);
        }
    }

    @Override
    public Ref getRef(int i) throws SQLException {
        throw Sql.unsupported("REF values");
    }

    @Override
    public Blob getBlob(int i) throws SQLException {
        throw Sql.unsupported("BLOB objects: getBytes gives the bytes");
    }

    @Override
    public Clob getClob(int i) throws SQLException {
        throw Sql.unsupported("CLOB objects: getString gives the text");
    }

    @Override
    public NClob getNClob(int i) throws SQLException {
        throw Sql.unsupported("NCLOB objects: getString gives the text");
    }

    @Override
    public SQLXML getSQLXML(int i) throws SQLException {
        throw Sql.unsupported("SQLXML");
    }

    @Override
    public RowId getRowId(int i) throws SQLException {
        throw Sql.unsupported("ROWID values: a row's key is a column");
    }

    // Values by column label.

    @Override
    public Object getObject(String label) throws SQLException {
        return getObject(findColumn(label));
    }

    @Override
    public <T> T getObject(String label, Class<T> type) throws SQLException {
        return getObject(findColumn(label), type);
    }

    @Override
    public Object getObject(String label, Map<String, Class<?>> map) throws SQLException {
        return getObject(findColumn(label));
    }

    @Override
    public String getString(String label) throws SQLException {
        return getString(findColumn(label));
    }

    @Override
    public String getNString(String label) throws SQLException {
        return getString(findColumn(label));
    }

    @Override
    public boolean getBoolean(String label) throws SQLException {
        return getBoolean(findColumn(label));
    }

    @Override
    public byte getByte(String label) throws SQLException {
        return getByte(findColumn(label));
    }

    @Override
    public short getShort(String label) throws SQLException {
        return getShort(findColumn(label));
    }

    @Override
    public int getInt(String label) throws SQLException {
        return getInt(findColumn(label));
    }

    @Override
    public long getLong(String label) throws SQLException {
        return getLong(findColumn(label));
    }

    @Override
    public float getFloat(String label) throws SQLException {
        return getFloat(findColumn(label));
    }

    @Override
    public double getDouble(String label) throws SQLException {
        return getDouble(findColumn(label));
    }

    @Override
    public BigDecimal getBigDecimal(String label) throws SQLException {
        return getBigDecimal(findColumn(label));
    }

    @Override
    @Deprecated
    public BigDecimal getBigDecimal(String label, int scale) throws SQLException {
        return getBigDecimal(findColumn(label), scale);
    }

    @Override
    public byte[] getBytes(String label) throws SQLException {
        return getBytes(findColumn(label));
    }

    @Override
    public Date getDate(String label) throws SQLException {
        return getDate(findColumn(label));
    }

    @Override
    public Date getDate(String label, Calendar cal) throws SQLException {
        return getDate(findColumn(label));
    }

    @Override
    public Time getTime(String label) throws SQLException {
        return getTime(findColumn(label));
    }

    @Override
    public Time getTime(String label, Calendar cal) throws SQLException {
        return getTime(findColumn(label));
    }

    @Override
    public Timestamp getTimestamp(String label) throws SQLException {
        return getTimestamp(findColumn(label));
    }

    @Override
    public Timestamp getTimestamp(String label, Calendar cal) throws SQLException {
        return getTimestamp(findColumn(label));
    }

    @Override
    public InputStream getAsciiStream(String label) throws SQLException {
        return getAsciiStream(findColumn(label));
    }

    @Override
    @Deprecated
    public InputStream getUnicodeStream(String label) throws SQLException {
        return getUnicodeStream(findColumn(label));
    }

    @Override
    public InputStream getBinaryStream(String label) throws SQLException {
        return getBinaryStream(findColumn(label));
    }

    @Override
    public Reader getCharacterStream(String label) throws SQLException {
        return getCharacterStream(findColumn(label));
    }

    @Override
    public Reader getNCharacterStream(String label) throws SQLException {
        return getCharacterStream(findColumn(label));
    }

    @Override
    public Array getArray(String label) throws SQLException {
        return getArray(findColumn(label));
    }

    @Override
    public URL getURL(String label) throws SQLException {
        return getURL(findColumn(label));
    }

    @Override
    public Ref getRef(String label) throws SQLException {
        return getRef(findColumn(label));
    }

    @Override
    public Blob getBlob(String label) throws SQLException {
        return getBlob(findColumn(label));
    }

    @Override
    public Clob getClob(String label) throws SQLException {
        return getClob(findColumn(label));
    }

    @Override
    public NClob getNClob(String label) throws SQLException {
        return getNClob(findColumn(label));
    }

    @Override
    public SQLXML getSQLXML(String label) throws SQLException {
        return getSQLXML(findColumn(label));
    }

    @Override
    public RowId getRowId(String label) throws SQLException {
        return getRowId(findColumn(label));
    }

    @Override
    public <T> T unwrap(Class<T> iface) throws SQLException {
        if (iface.isInstance(this)) {
            return iface.cast(this);
        }
        throw new SQLException("a ChloeDB result set does not unwrap to a " + iface.getName());
    }

    @Override
    public boolean isWrapperFor(Class<?> iface) {
        return iface.isInstance(this);
    }

    @Override
    public String toString() {
        return "ChloeDBResultSet(" + rows.size() + " rows: " + String.join(", ", columns) + ")";
    }
}
