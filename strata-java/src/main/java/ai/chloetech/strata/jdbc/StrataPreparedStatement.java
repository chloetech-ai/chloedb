package ai.chloetech.strata.jdbc;

import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.Reader;
import java.math.BigDecimal;
import java.net.URL;
import java.nio.charset.StandardCharsets;
import java.sql.Array;
import java.sql.Blob;
import java.sql.Clob;
import java.sql.Date;
import java.sql.NClob;
import java.sql.ParameterMetaData;
import java.sql.PreparedStatement;
import java.sql.Ref;
import java.sql.ResultSet;
import java.sql.ResultSetMetaData;
import java.sql.RowId;
import java.sql.SQLException;
import java.sql.SQLType;
import java.sql.SQLXML;
import java.sql.Time;
import java.sql.Timestamp;
import java.sql.Types;
import java.util.ArrayList;
import java.util.Calendar;
import java.util.HashMap;
import java.util.List;
import java.util.Map;

/**
 * A statement with {@code ?} parameters, numbered from 1. A {@code float[]}
 * given to {@link #setObject(int, Object)} is a vector; a {@code List} or an
 * array is a list; a {@code Map} is a JSON object.
 */
public final class StrataPreparedStatement extends StrataStatement implements PreparedStatement {
    private final String sql;
    private final Sql.Rewritten rewritten;
    private final Object[] values;
    private final boolean[] set;
    private final List<Object[]> batch = new ArrayList<>();

    StrataPreparedStatement(StrataConnection conn, String sql) {
        super(conn);
        this.sql = sql;
        this.rewritten = Sql.placeholders(sql);
        this.values = new Object[rewritten.params];
        this.set = new boolean[rewritten.params];
    }

    /** The SQL as it was given. */
    String sql() {
        return sql;
    }

    private Map<String, Object> params(Object[] vs) {
        Map<String, Object> m = new HashMap<>();
        for (int i = 0; i < vs.length; i++) {
            m.put(Sql.param(i + 1), vs[i]);
        }
        return m;
    }

    private Object[] bound() throws SQLException {
        for (int i = 0; i < set.length; i++) {
            if (!set[i]) {
                throw new SQLException("parameter " + (i + 1) + " of " + set.length + " has no value: set it with setString, setLong, setObject... (setNull for NULL)", "07001");
            }
        }
        return values.clone();
    }

    private void put(int i, Object v) throws SQLException {
        check();
        if (i < 1 || i > values.length) {
            throw new SQLException("the statement has " + values.length + " parameter" + (values.length == 1 ? "" : "s") + " (numbered from 1), not a parameter " + i, "07009");
        }
        values[i - 1] = v;
        set[i - 1] = true;
    }

    /** A value as the bindings take it. */
    static Object value(Object x) throws SQLException {
        if (x instanceof Timestamp) {
            return ((Timestamp) x).toInstant();
        }
        if (x instanceof Date) {
            return ((Date) x).toLocalDate();
        }
        if (x instanceof Time) {
            throw Sql.unsupported("times of day (java.sql.Time): give a Timestamp, or a String");
        }
        if (x instanceof java.util.Date) {
            return ((java.util.Date) x).toInstant();
        }
        if (x instanceof Array) {
            Object[] xs = (Object[]) ((Array) x).getArray();
            Object[] out = new Object[xs.length];
            for (int i = 0; i < xs.length; i++) {
                out[i] = value(xs[i]);
            }
            return out;
        }
        if (x instanceof Blob) {
            Blob b = (Blob) x;
            return b.getBytes(1, (int) b.length());
        }
        if (x instanceof Clob) {
            Clob c = (Clob) x;
            return c.getSubString(1, (int) c.length());
        }
        if (x instanceof URL) {
            return x.toString();
        }
        return x;
    }

    @Override
    public ResultSet executeQuery() throws SQLException {
        if (!Sql.answersRows(sql)) {
            throw new SQLException("executeQuery runs a SELECT, WITH or EXPLAIN; other statements go to executeUpdate or execute", "07000");
        }
        return select(rewritten.sql, params(bound()));
    }

    @Override
    public int executeUpdate() throws SQLException {
        return toInt(executeLargeUpdate());
    }

    @Override
    public long executeLargeUpdate() throws SQLException {
        if (Sql.answersRows(sql)) {
            throw new SQLException("executeUpdate runs a statement that writes; a SELECT goes to executeQuery", "07000");
        }
        return write(rewritten.sql, params(bound()));
    }

    @Override
    public boolean execute() throws SQLException {
        return run(rewritten.sql, params(bound()));
    }

    @Override
    public void addBatch() throws SQLException {
        check();
        batch.add(bound());
    }

    @Override
    public void clearBatch() throws SQLException {
        check();
        batch.clear();
    }

    /** Runs the statement once for each set of parameters added; a run that fails stops the batch, and the ones before it stay written. */
    @Override
    public long[] executeLargeBatch() throws SQLException {
        check();
        if (Sql.answersRows(sql)) {
            batch.clear();
            throw Batch.failed(new long[0], 0, new SQLException("a batch runs statements that write, not a SELECT", "07000"));
        }
        List<Object[]> todo = new ArrayList<>(batch);
        batch.clear();
        long[] counts = new long[todo.size()];
        for (int i = 0; i < todo.size(); i++) {
            try {
                counts[i] = write(rewritten.sql, params(todo.get(i)));
            } catch (SQLException e) {
                throw Batch.failed(counts, i, e);
            }
        }
        return counts;
    }

    // A prepared statement runs its own SQL.

    private static SQLException ownSql() {
        return new SQLException("a PreparedStatement runs the SQL it was prepared with: call executeQuery(), executeUpdate() or execute() without SQL", "HY000");
    }

    @Override
    public ResultSet executeQuery(String sql) throws SQLException {
        throw ownSql();
    }

    @Override
    public int executeUpdate(String sql) throws SQLException {
        throw ownSql();
    }

    @Override
    public long executeLargeUpdate(String sql) throws SQLException {
        throw ownSql();
    }

    @Override
    public boolean execute(String sql) throws SQLException {
        throw ownSql();
    }

    @Override
    public void addBatch(String sql) throws SQLException {
        throw ownSql();
    }

    @Override
    public void clearParameters() throws SQLException {
        check();
        java.util.Arrays.fill(values, null);
        java.util.Arrays.fill(set, false);
    }

    @Override
    public void setNull(int i, int sqlType) throws SQLException {
        put(i, null);
    }

    @Override
    public void setNull(int i, int sqlType, String typeName) throws SQLException {
        put(i, null);
    }

    @Override
    public void setBoolean(int i, boolean x) throws SQLException {
        put(i, x);
    }

    @Override
    public void setByte(int i, byte x) throws SQLException {
        put(i, (long) x);
    }

    @Override
    public void setShort(int i, short x) throws SQLException {
        put(i, (long) x);
    }

    @Override
    public void setInt(int i, int x) throws SQLException {
        put(i, (long) x);
    }

    @Override
    public void setLong(int i, long x) throws SQLException {
        put(i, x);
    }

    @Override
    public void setFloat(int i, float x) throws SQLException {
        put(i, (double) x);
    }

    @Override
    public void setDouble(int i, double x) throws SQLException {
        put(i, x);
    }

    @Override
    public void setBigDecimal(int i, BigDecimal x) throws SQLException {
        put(i, x);
    }

    @Override
    public void setString(int i, String x) throws SQLException {
        put(i, x);
    }

    @Override
    public void setNString(int i, String x) throws SQLException {
        put(i, x);
    }

    @Override
    public void setBytes(int i, byte[] x) throws SQLException {
        put(i, x);
    }

    @Override
    public void setDate(int i, Date x) throws SQLException {
        put(i, x == null ? null : x.toLocalDate());
    }

    /** The calendar is ignored: a date is stored as its day, at midnight UTC. */
    @Override
    public void setDate(int i, Date x, Calendar cal) throws SQLException {
        setDate(i, x);
    }

    @Override
    public void setTime(int i, Time x) throws SQLException {
        throw Sql.unsupported("times of day (java.sql.Time): give a Timestamp, or a String");
    }

    @Override
    public void setTime(int i, Time x, Calendar cal) throws SQLException {
        setTime(i, x);
    }

    @Override
    public void setTimestamp(int i, Timestamp x) throws SQLException {
        put(i, x == null ? null : x.toInstant());
    }

    /** The calendar is ignored: a Timestamp is an instant, and is stored as one. */
    @Override
    public void setTimestamp(int i, Timestamp x, Calendar cal) throws SQLException {
        setTimestamp(i, x);
    }

    @Override
    public void setObject(int i, Object x) throws SQLException {
        put(i, value(x));
    }

    @Override
    public void setObject(int i, Object x, int targetSqlType) throws SQLException {
        setObject(i, x);
    }

    @Override
    public void setObject(int i, Object x, int targetSqlType, int scaleOrLength) throws SQLException {
        setObject(i, x);
    }

    @Override
    public void setObject(int i, Object x, SQLType targetSqlType) throws SQLException {
        setObject(i, x);
    }

    @Override
    public void setObject(int i, Object x, SQLType targetSqlType, int scaleOrLength) throws SQLException {
        setObject(i, x);
    }

    @Override
    public void setArray(int i, Array x) throws SQLException {
        put(i, value(x));
    }

    @Override
    public void setBlob(int i, Blob x) throws SQLException {
        put(i, value(x));
    }

    @Override
    public void setClob(int i, Clob x) throws SQLException {
        put(i, value(x));
    }

    @Override
    public void setURL(int i, URL x) throws SQLException {
        put(i, value(x));
    }

    // Streams are read to the end, into bytes or text.

    private static byte[] bytes(InputStream in, long length) throws SQLException {
        if (in == null) {
            return null;
        }
        try {
            ByteArrayOutputStream out = new ByteArrayOutputStream();
            byte[] buf = new byte[8192];
            long left = length < 0 ? Long.MAX_VALUE : length;
            int n;
            while (left > 0 && (n = in.read(buf, 0, (int) Math.min(buf.length, left))) > 0) {
                out.write(buf, 0, n);
                left -= n;
            }
            return out.toByteArray();
        } catch (IOException e) {
            throw new SQLException("reading the stream: " + e.getMessage(), "22000", e);
        }
    }

    private static String text(Reader in, long length) throws SQLException {
        if (in == null) {
            return null;
        }
        try {
            StringBuilder out = new StringBuilder();
            char[] buf = new char[8192];
            long left = length < 0 ? Long.MAX_VALUE : length;
            int n;
            while (left > 0 && (n = in.read(buf, 0, (int) Math.min(buf.length, left))) > 0) {
                out.append(buf, 0, n);
                left -= n;
            }
            return out.toString();
        } catch (IOException e) {
            throw new SQLException("reading the stream: " + e.getMessage(), "22000", e);
        }
    }

    private static String ascii(InputStream in, long length) throws SQLException {
        byte[] b = bytes(in, length);
        return b == null ? null : new String(b, StandardCharsets.US_ASCII);
    }

    @Override
    public void setAsciiStream(int i, InputStream x, int length) throws SQLException {
        put(i, ascii(x, length));
    }

    @Override
    public void setAsciiStream(int i, InputStream x, long length) throws SQLException {
        put(i, ascii(x, length));
    }

    @Override
    public void setAsciiStream(int i, InputStream x) throws SQLException {
        put(i, ascii(x, -1));
    }

    @Override
    @Deprecated
    public void setUnicodeStream(int i, InputStream x, int length) throws SQLException {
        throw Sql.unsupported("setUnicodeStream (deprecated): use setCharacterStream");
    }

    @Override
    public void setBinaryStream(int i, InputStream x, int length) throws SQLException {
        put(i, bytes(x, length));
    }

    @Override
    public void setBinaryStream(int i, InputStream x, long length) throws SQLException {
        put(i, bytes(x, length));
    }

    @Override
    public void setBinaryStream(int i, InputStream x) throws SQLException {
        put(i, bytes(x, -1));
    }

    @Override
    public void setCharacterStream(int i, Reader x, int length) throws SQLException {
        put(i, text(x, length));
    }

    @Override
    public void setCharacterStream(int i, Reader x, long length) throws SQLException {
        put(i, text(x, length));
    }

    @Override
    public void setCharacterStream(int i, Reader x) throws SQLException {
        put(i, text(x, -1));
    }

    @Override
    public void setNCharacterStream(int i, Reader x, long length) throws SQLException {
        put(i, text(x, length));
    }

    @Override
    public void setNCharacterStream(int i, Reader x) throws SQLException {
        put(i, text(x, -1));
    }

    @Override
    public void setClob(int i, Reader x, long length) throws SQLException {
        put(i, text(x, length));
    }

    @Override
    public void setClob(int i, Reader x) throws SQLException {
        put(i, text(x, -1));
    }

    @Override
    public void setNClob(int i, Reader x, long length) throws SQLException {
        put(i, text(x, length));
    }

    @Override
    public void setNClob(int i, Reader x) throws SQLException {
        put(i, text(x, -1));
    }

    @Override
    public void setNClob(int i, NClob x) throws SQLException {
        put(i, value(x));
    }

    @Override
    public void setBlob(int i, InputStream x, long length) throws SQLException {
        put(i, bytes(x, length));
    }

    @Override
    public void setBlob(int i, InputStream x) throws SQLException {
        put(i, bytes(x, -1));
    }

    @Override
    public void setRef(int i, Ref x) throws SQLException {
        throw Sql.unsupported("REF values");
    }

    @Override
    public void setRowId(int i, RowId x) throws SQLException {
        throw Sql.unsupported("ROWID values: a row's key is a column");
    }

    @Override
    public void setSQLXML(int i, SQLXML x) throws SQLException {
        throw Sql.unsupported("SQLXML");
    }

    /** {@code null}: the columns a statement answers are known once it runs. */
    @Override
    public ResultSetMetaData getMetaData() throws SQLException {
        check();
        return null;
    }

    @Override
    public ParameterMetaData getParameterMetaData() throws SQLException {
        check();
        return new Parameters(values.length);
    }

    /** The parameters' count; Strata takes any value for any parameter. */
    static final class Parameters implements ParameterMetaData {
        private final int n;

        Parameters(int n) {
            this.n = n;
        }

        private void in(int i) throws SQLException {
            if (i < 1 || i > n) {
                throw new SQLException("the statement has " + n + " parameters, not a parameter " + i, "07009");
            }
        }

        @Override
        public int getParameterCount() {
            return n;
        }

        @Override
        public int isNullable(int i) throws SQLException {
            in(i);
            return parameterNullableUnknown;
        }

        @Override
        public boolean isSigned(int i) throws SQLException {
            in(i);
            return true;
        }

        @Override
        public int getPrecision(int i) throws SQLException {
            in(i);
            return 0;
        }

        @Override
        public int getScale(int i) throws SQLException {
            in(i);
            return 0;
        }

        @Override
        public int getParameterType(int i) throws SQLException {
            in(i);
            return Types.OTHER;
        }

        @Override
        public String getParameterTypeName(int i) throws SQLException {
            in(i);
            return "ANY";
        }

        @Override
        public String getParameterClassName(int i) throws SQLException {
            in(i);
            return Object.class.getName();
        }

        @Override
        public int getParameterMode(int i) throws SQLException {
            in(i);
            return parameterModeIn;
        }

        @Override
        public <T> T unwrap(Class<T> iface) throws SQLException {
            if (iface.isInstance(this)) {
                return iface.cast(this);
            }
            throw new SQLException("does not unwrap to a " + iface.getName());
        }

        @Override
        public boolean isWrapperFor(Class<?> iface) {
            return iface.isInstance(this);
        }
    }
}
