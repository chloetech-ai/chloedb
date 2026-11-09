package ai.chloetech.chloedb.jdbc;

import java.math.BigDecimal;
import java.math.BigInteger;
import java.nio.charset.StandardCharsets;
import java.sql.Date;
import java.sql.SQLDataException;
import java.sql.SQLException;
import java.sql.Time;
import java.sql.Timestamp;
import java.time.Instant;
import java.time.LocalDate;
import java.time.LocalDateTime;
import java.time.LocalTime;
import java.time.OffsetDateTime;
import java.time.ZoneOffset;
import java.time.ZonedDateTime;
import java.time.format.DateTimeParseException;
import java.util.Base64;
import java.util.List;
import java.util.Map;

/**
 * ChloeDB's values (null, Boolean, Long, Double, String, float[] vectors,
 * lists, maps, Instant times, byte[]) as the types JDBC's getters ask for.
 * Times are in UTC.
 */
final class Values {
    private Values() {}

    private static SQLException cannot(Object v, String type) {
        return new SQLDataException("cannot read a " + kind(v) + " (" + shorten(v) + ") as " + type, "22018");
    }

    private static String shorten(Object v) {
        String s = string(v);
        return s.length() > 40 ? s.substring(0, 37) + "..." : s;
    }

    static String kind(Object v) {
        if (v instanceof Long) {
            return "INT";
        }
        if (v instanceof Double) {
            return "FLOAT";
        }
        if (v instanceof String) {
            return "TEXT";
        }
        if (v instanceof Boolean) {
            return "BOOL";
        }
        if (v instanceof Instant) {
            return "TIMESTAMP";
        }
        if (v instanceof float[]) {
            return "VECTOR";
        }
        if (v instanceof List) {
            return "LIST";
        }
        if (v instanceof Map) {
            return "JSON";
        }
        if (v instanceof byte[]) {
            return "BYTES";
        }
        return v.getClass().getSimpleName();
    }

    static String string(Object v) {
        if (v == null) {
            return null;
        }
        if (v instanceof String) {
            return (String) v;
        }
        if (v instanceof float[] || v instanceof List || v instanceof Map) {
            StringBuilder b = new StringBuilder();
            json(b, v);
            return b.toString();
        }
        if (v instanceof byte[]) {
            return Base64.getEncoder().encodeToString((byte[]) v);
        }
        return v.toString();
    }

    /** A list, map or vector as JSON text. */
    private static void json(StringBuilder b, Object v) {
        if (v == null) {
            b.append("null");
        } else if (v instanceof String) {
            b.append('"');
            String s = (String) v;
            for (int i = 0; i < s.length(); i++) {
                char c = s.charAt(i);
                switch (c) {
                    case '"':
                        b.append("\\\"");
                        break;
                    case '\\':
                        b.append("\\\\");
                        break;
                    case '\n':
                        b.append("\\n");
                        break;
                    case '\r':
                        b.append("\\r");
                        break;
                    case '\t':
                        b.append("\\t");
                        break;
                    default:
                        if (c < 0x20) {
                            b.append(String.format("\\u%04x", (int) c));
                        } else {
                            b.append(c);
                        }
                }
            }
            b.append('"');
        } else if (v instanceof float[]) {
            float[] xs = (float[]) v;
            b.append('[');
            for (int i = 0; i < xs.length; i++) {
                b.append(i == 0 ? "" : ",").append(xs[i]);
            }
            b.append(']');
        } else if (v instanceof List) {
            b.append('[');
            boolean first = true;
            for (Object x : (List<?>) v) {
                b.append(first ? "" : ",");
                json(b, x);
                first = false;
            }
            b.append(']');
        } else if (v instanceof Map) {
            b.append('{');
            boolean first = true;
            for (Map.Entry<?, ?> e : ((Map<?, ?>) v).entrySet()) {
                b.append(first ? "" : ",");
                json(b, String.valueOf(e.getKey()));
                b.append(':');
                json(b, e.getValue());
                first = false;
            }
            b.append('}');
        } else if (v instanceof Instant || v instanceof byte[]) {
            json(b, string(v));
        } else {
            b.append(v);
        }
    }

    static long getLong(Object v, String type, long min, long max) throws SQLException {
        long n;
        if (v instanceof Long) {
            n = (Long) v;
        } else if (v instanceof Double) {
            double d = (Double) v;
            if (Double.isNaN(d) || d < min || d > max) {
                throw new SQLDataException(d + " is out of the range of " + type, "22003");
            }
            n = (long) d;
        } else if (v instanceof Boolean) {
            n = (Boolean) v ? 1 : 0;
        } else if (v instanceof String) {
            String s = ((String) v).trim();
            try {
                n = Long.parseLong(s);
            } catch (NumberFormatException e) {
                try {
                    return getLong(Double.parseDouble(s), type, min, max);
                } catch (NumberFormatException e2) {
                    throw cannot(v, type);
                }
            }
        } else {
            throw cannot(v, type);
        }
        if (n < min || n > max) {
            throw new SQLDataException(n + " is out of the range of " + type, "22003");
        }
        return n;
    }

    static double getDouble(Object v, String type) throws SQLException {
        if (v instanceof Number) {
            return ((Number) v).doubleValue();
        }
        if (v instanceof Boolean) {
            return (Boolean) v ? 1 : 0;
        }
        if (v instanceof String) {
            try {
                return Double.parseDouble(((String) v).trim());
            } catch (NumberFormatException e) {
                throw cannot(v, type);
            }
        }
        throw cannot(v, type);
    }

    static BigDecimal getBigDecimal(Object v) throws SQLException {
        if (v instanceof Long) {
            return BigDecimal.valueOf((Long) v);
        }
        if (v instanceof Double) {
            double d = (Double) v;
            if (Double.isNaN(d) || Double.isInfinite(d)) {
                throw new SQLDataException(d + " is not a decimal", "22003");
            }
            return BigDecimal.valueOf(d);
        }
        if (v instanceof Boolean) {
            return (Boolean) v ? BigDecimal.ONE : BigDecimal.ZERO;
        }
        if (v instanceof String) {
            try {
                return new BigDecimal(((String) v).trim());
            } catch (NumberFormatException e) {
                throw cannot(v, "a decimal");
            }
        }
        throw cannot(v, "a decimal");
    }

    static boolean getBoolean(Object v) throws SQLException {
        if (v instanceof Boolean) {
            return (Boolean) v;
        }
        if (v instanceof Number) {
            return ((Number) v).doubleValue() != 0;
        }
        if (v instanceof String) {
            String s = ((String) v).trim();
            if (s.equalsIgnoreCase("true") || s.equals("1")) {
                return true;
            }
            if (s.equalsIgnoreCase("false") || s.equals("0")) {
                return false;
            }
        }
        throw cannot(v, "a boolean");
    }

    static byte[] getBytes(Object v) throws SQLException {
        if (v instanceof byte[]) {
            return ((byte[]) v).clone();
        }
        if (v instanceof String) {
            return ((String) v).getBytes(StandardCharsets.UTF_8);
        }
        throw cannot(v, "bytes");
    }

    static Instant getInstant(Object v) throws SQLException {
        if (v instanceof Instant) {
            return (Instant) v;
        }
        if (v instanceof String) {
            String s = ((String) v).trim();
            try {
                return Instant.parse(s);
            } catch (DateTimeParseException e) {
                try {
                    return OffsetDateTime.parse(s).toInstant();
                } catch (DateTimeParseException e2) {
                    try {
                        return LocalDateTime.parse(s).toInstant(ZoneOffset.UTC);
                    } catch (DateTimeParseException e3) {
                        try {
                            return LocalDate.parse(s).atStartOfDay(ZoneOffset.UTC).toInstant();
                        } catch (DateTimeParseException e4) {
                            throw cannot(v, "a time");
                        }
                    }
                }
            }
        }
        throw cannot(v, "a time");
    }

    static Timestamp getTimestamp(Object v) throws SQLException {
        return Timestamp.from(getInstant(v));
    }

    static Date getDate(Object v) throws SQLException {
        return Date.valueOf(LocalDate.ofInstant(getInstant(v), ZoneOffset.UTC));
    }

    static Time getTime(Object v) throws SQLException {
        return Time.valueOf(LocalTime.ofInstant(getInstant(v), ZoneOffset.UTC));
    }

    static float[] getVector(Object v) throws SQLException {
        if (v instanceof float[]) {
            return ((float[]) v).clone();
        }
        if (v instanceof List) {
            List<?> xs = (List<?>) v;
            float[] out = new float[xs.size()];
            for (int i = 0; i < out.length; i++) {
                out[i] = (float) getDouble(xs.get(i), "a vector");
            }
            return out;
        }
        throw cannot(v, "a vector");
    }

    /** What {@code getObject} answers: times as {@link Timestamp}, the rest as it came. */
    static Object object(Object v) {
        return v instanceof Instant ? Timestamp.from((Instant) v) : v;
    }

    @SuppressWarnings("unchecked")
    static <T> T as(Object v, Class<T> type) throws SQLException {
        if (v == null) {
            return null;
        }
        Object out;
        if (type == String.class) {
            out = string(v);
        } else if (type == Long.class || type == long.class) {
            out = getLong(v, "a long", Long.MIN_VALUE, Long.MAX_VALUE);
        } else if (type == Integer.class || type == int.class) {
            out = (int) getLong(v, "an int", Integer.MIN_VALUE, Integer.MAX_VALUE);
        } else if (type == Short.class || type == short.class) {
            out = (short) getLong(v, "a short", Short.MIN_VALUE, Short.MAX_VALUE);
        } else if (type == Byte.class || type == byte.class) {
            out = (byte) getLong(v, "a byte", Byte.MIN_VALUE, Byte.MAX_VALUE);
        } else if (type == Double.class || type == double.class) {
            out = getDouble(v, "a double");
        } else if (type == Float.class || type == float.class) {
            out = (float) getDouble(v, "a float");
        } else if (type == Boolean.class || type == boolean.class) {
            out = getBoolean(v);
        } else if (type == BigDecimal.class) {
            out = getBigDecimal(v);
        } else if (type == BigInteger.class) {
            out = v instanceof Long ? BigInteger.valueOf((Long) v) : getBigDecimal(v).toBigInteger();
        } else if (type == Instant.class) {
            out = getInstant(v);
        } else if (type == OffsetDateTime.class) {
            out = getInstant(v).atOffset(ZoneOffset.UTC);
        } else if (type == ZonedDateTime.class) {
            out = getInstant(v).atZone(ZoneOffset.UTC);
        } else if (type == LocalDateTime.class) {
            out = LocalDateTime.ofInstant(getInstant(v), ZoneOffset.UTC);
        } else if (type == LocalDate.class) {
            out = LocalDate.ofInstant(getInstant(v), ZoneOffset.UTC);
        } else if (type == LocalTime.class) {
            out = LocalTime.ofInstant(getInstant(v), ZoneOffset.UTC);
        } else if (type == Timestamp.class) {
            out = getTimestamp(v);
        } else if (type == Date.class) {
            out = getDate(v);
        } else if (type == Time.class) {
            out = getTime(v);
        } else if (type == float[].class) {
            out = getVector(v);
        } else if (type == byte[].class) {
            out = getBytes(v);
        } else if (type.isInstance(v)) {
            out = v;
        } else if (type.isInstance(object(v))) {
            out = object(v);
        } else {
            throw cannot(v, "a " + type.getName());
        }
        return (T) out;
    }
}
