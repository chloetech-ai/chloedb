package ai.chloetech.strata.jdbc;

import java.sql.ResultSetMetaData;
import java.sql.SQLException;
import java.sql.Timestamp;
import java.sql.Types;
import java.time.Instant;
import java.util.List;
import java.util.Map;

/**
 * The columns of a result. A column's type is what its values are (a
 * SELECT's columns can be any expression): INT is BIGINT, FLOAT is DOUBLE,
 * TEXT and KEYWORD are VARCHAR, a vector is an ARRAY of REAL, a list is an
 * ARRAY, JSON is OTHER; a column of nulls only, or of values of different
 * types, is OTHER too.
 */
final class StrataResultSetMetaData implements ResultSetMetaData {
    /** A column's type as JDBC names it. */
    static final class Kind {
        final int sqlType;
        final String name;
        final String className;
        final int precision;
        final boolean signed;

        Kind(int sqlType, String name, String className, int precision, boolean signed) {
            this.sqlType = sqlType;
            this.name = name;
            this.className = className;
            this.precision = precision;
            this.signed = signed;
        }

        Kind precision(int p) {
            return new Kind(sqlType, name, className, p, signed);
        }
    }

    static final Kind INT = new Kind(Types.BIGINT, "INT", Long.class.getName(), 19, true);
    static final Kind FLOAT = new Kind(Types.DOUBLE, "FLOAT", Double.class.getName(), 17, true);
    static final Kind TEXT = new Kind(Types.VARCHAR, "TEXT", String.class.getName(), 0, false);
    static final Kind BOOL = new Kind(Types.BOOLEAN, "BOOL", Boolean.class.getName(), 1, false);
    static final Kind TIMESTAMP = new Kind(Types.TIMESTAMP, "TIMESTAMP", Timestamp.class.getName(), 26, false);
    static final Kind VECTOR = new Kind(Types.ARRAY, "VECTOR", float[].class.getName(), 0, false);
    static final Kind LIST = new Kind(Types.ARRAY, "LIST", List.class.getName(), 0, false);
    static final Kind JSON = new Kind(Types.OTHER, "JSON", Map.class.getName(), 0, false);
    static final Kind BYTES = new Kind(Types.VARBINARY, "BYTES", byte[].class.getName(), 0, false);
    static final Kind ANY = new Kind(Types.OTHER, "ANY", Object.class.getName(), 0, false);

    static Kind of(Object v) {
        if (v instanceof Long) {
            return INT;
        }
        if (v instanceof Double) {
            return FLOAT;
        }
        if (v instanceof String) {
            return TEXT;
        }
        if (v instanceof Boolean) {
            return BOOL;
        }
        if (v instanceof Instant || v instanceof Timestamp) {
            return TIMESTAMP;
        }
        if (v instanceof float[]) {
            return VECTOR;
        }
        if (v instanceof List) {
            return LIST;
        }
        if (v instanceof Map) {
            return JSON;
        }
        if (v instanceof byte[]) {
            return BYTES;
        }
        return ANY;
    }

    /** The type of column {@code c} (from 0) of {@code rows}: an INT column with a FLOAT in it is FLOAT. */
    static Kind infer(List<List<Object>> rows, int c) {
        Kind k = null;
        int longest = 0;
        for (List<Object> row : rows) {
            Object v = row.get(c);
            if (v == null) {
                continue;
            }
            Kind kv = of(v);
            if (k == null || k == kv) {
                k = kv;
            } else if ((k == INT && kv == FLOAT) || (k == FLOAT && kv == INT)) {
                k = FLOAT;
            } else {
                return ANY;
            }
            if (kv == TEXT) {
                longest = Math.max(longest, ((String) v).length());
            } else if (kv == VECTOR) {
                longest = Math.max(longest, ((float[]) v).length);
            }
        }
        if (k == null) {
            return ANY;
        }
        return k == TEXT || k == VECTOR ? k.precision(longest) : k;
    }

    private final List<String> columns;
    private final Kind[] kinds;

    StrataResultSetMetaData(List<String> columns, List<List<Object>> rows) {
        this.columns = columns;
        this.kinds = new Kind[columns.size()];
        for (int c = 0; c < kinds.length; c++) {
            kinds[c] = infer(rows, c);
        }
    }

    private Kind kind(int i) throws SQLException {
        if (i < 1 || i > kinds.length) {
            throw new SQLException("the result has " + kinds.length + " columns (numbered from 1), not a column " + i, "07009");
        }
        return kinds[i - 1];
    }

    @Override
    public int getColumnCount() {
        return columns.size();
    }

    @Override
    public boolean isAutoIncrement(int i) throws SQLException {
        kind(i);
        return false;
    }

    @Override
    public boolean isCaseSensitive(int i) throws SQLException {
        return kind(i) == TEXT;
    }

    @Override
    public boolean isSearchable(int i) throws SQLException {
        kind(i);
        return true;
    }

    @Override
    public boolean isCurrency(int i) throws SQLException {
        kind(i);
        return false;
    }

    @Override
    public int isNullable(int i) throws SQLException {
        kind(i);
        return columnNullableUnknown;
    }

    @Override
    public boolean isSigned(int i) throws SQLException {
        return kind(i).signed;
    }

    @Override
    public int getColumnDisplaySize(int i) throws SQLException {
        Kind k = kind(i);
        if (k == INT) {
            return 20;
        }
        if (k == FLOAT) {
            return 24;
        }
        if (k == BOOL) {
            return 5;
        }
        if (k == TIMESTAMP) {
            return 27;
        }
        return Math.max(k.precision, 1);
    }

    @Override
    public String getColumnLabel(int i) throws SQLException {
        kind(i);
        return columns.get(i - 1);
    }

    @Override
    public String getColumnName(int i) throws SQLException {
        return getColumnLabel(i);
    }

    @Override
    public String getSchemaName(int i) throws SQLException {
        kind(i);
        return "";
    }

    @Override
    public int getPrecision(int i) throws SQLException {
        return kind(i).precision;
    }

    @Override
    public int getScale(int i) throws SQLException {
        kind(i);
        return 0;
    }

    /** "": a column of a SELECT can be any expression, of any tables. */
    @Override
    public String getTableName(int i) throws SQLException {
        kind(i);
        return "";
    }

    @Override
    public String getCatalogName(int i) throws SQLException {
        kind(i);
        return "";
    }

    @Override
    public int getColumnType(int i) throws SQLException {
        return kind(i).sqlType;
    }

    @Override
    public String getColumnTypeName(int i) throws SQLException {
        return kind(i).name;
    }

    @Override
    public boolean isReadOnly(int i) throws SQLException {
        kind(i);
        return true;
    }

    @Override
    public boolean isWritable(int i) throws SQLException {
        kind(i);
        return false;
    }

    @Override
    public boolean isDefinitelyWritable(int i) throws SQLException {
        kind(i);
        return false;
    }

    @Override
    public String getColumnClassName(int i) throws SQLException {
        return kind(i).className;
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
