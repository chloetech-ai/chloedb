package ai.chloetech.chloedb.jdbc;

import java.sql.Array;
import java.sql.ResultSet;
import java.sql.SQLException;
import java.sql.Types;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;
import java.util.Map;

/** A vector (its elements are {@code Float}s) or a list, as JDBC's {@link Array}. */
final class ChloeDBArray implements Array {
    private final Object[] items;
    private final boolean vector;
    private boolean freed;

    ChloeDBArray(Object v) throws SQLException {
        if (v instanceof float[]) {
            float[] xs = (float[]) v;
            items = new Object[xs.length];
            for (int i = 0; i < xs.length; i++) {
                items[i] = xs[i];
            }
            vector = true;
        } else if (v instanceof List) {
            List<?> xs = (List<?>) v;
            items = new Object[xs.size()];
            for (int i = 0; i < items.length; i++) {
                items[i] = Values.object(xs.get(i));
            }
            vector = false;
        } else {
            throw new SQLException("a " + Values.kind(v) + " is not an array: vectors and lists are", "22018");
        }
    }

    private void check() throws SQLException {
        if (freed) {
            throw new SQLException("the array was freed", "HY010");
        }
    }

    /** The elements' type: REAL for a vector, else what the elements are (OTHER if they differ). */
    @Override
    public int getBaseType() throws SQLException {
        check();
        return vector ? Types.REAL : elements().sqlType;
    }

    private ChloeDBResultSetMetaData.Kind elements() {
        List<List<Object>> column = new ArrayList<>();
        for (Object x : items) {
            column.add(Arrays.asList(x));
        }
        return ChloeDBResultSetMetaData.infer(column, 0);
    }

    @Override
    public String getBaseTypeName() throws SQLException {
        check();
        return vector ? "REAL" : elements().name;
    }

    @Override
    public Object getArray() throws SQLException {
        check();
        return vector ? Arrays.copyOf(items, items.length, Float[].class) : items.clone();
    }

    @Override
    public Object getArray(Map<String, Class<?>> map) throws SQLException {
        return getArray();
    }

    /** The {@code count} elements from {@code index} (from 1). */
    @Override
    public Object getArray(long index, int count) throws SQLException {
        check();
        if (index < 1 || count < 0 || index - 1 + count > items.length) {
            throw new SQLException("the array has " + items.length + " elements: " + count + " from " + index + " is outside it", "2202E");
        }
        int from = (int) index - 1;
        return vector ? Arrays.copyOfRange(items, from, from + count, Float[].class) : Arrays.copyOfRange(items, from, from + count);
    }

    @Override
    public Object getArray(long index, int count, Map<String, Class<?>> map) throws SQLException {
        return getArray(index, count);
    }

    @Override
    public ResultSet getResultSet() throws SQLException {
        throw Sql.unsupported("an array's elements as a result set: getArray() gives them");
    }

    @Override
    public ResultSet getResultSet(Map<String, Class<?>> map) throws SQLException {
        return getResultSet();
    }

    @Override
    public ResultSet getResultSet(long index, int count) throws SQLException {
        return getResultSet();
    }

    @Override
    public ResultSet getResultSet(long index, int count, Map<String, Class<?>> map) throws SQLException {
        return getResultSet();
    }

    @Override
    public void free() {
        freed = true;
    }

    @Override
    public String toString() {
        return Arrays.toString(items);
    }
}
