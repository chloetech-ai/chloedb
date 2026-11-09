package ai.chloetech.strata;

import java.math.BigDecimal;
import java.math.BigInteger;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.charset.StandardCharsets;
import java.time.Instant;
import java.time.LocalDate;
import java.time.LocalDateTime;
import java.time.OffsetDateTime;
import java.time.ZoneOffset;
import java.time.ZonedDateTime;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Date;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * The values that cross to the native library and back, in one tagged
 * binary form, little-endian (the native side is {@code src/codec.rs}, which
 * lists the tags). A call's arguments are one value and its answer another,
 * so that no JNI call is made per value.
 *
 * <p>Java values become: {@code null}; {@code Boolean}; {@code Byte},
 * {@code Short}, {@code Integer}, {@code Long} and {@code BigInteger}
 * integers; {@code Float}, {@code Double} and {@code BigDecimal} floats;
 * strings (any {@code CharSequence}, or a {@code Character}); vectors
 * ({@code float[]}, {@code double[]}); lists (any {@code Iterable}, an
 * {@code Object[]}, {@code int[]} or {@code long[]}); maps with string keys;
 * times ({@code Instant}, {@code OffsetDateTime}, {@code ZonedDateTime},
 * {@code Date}, and {@code LocalDateTime} and {@code LocalDate} taken as
 * UTC); and {@code byte[]}. Answers come back as {@code null},
 * {@code Boolean}, {@code Long}, {@code Double}, {@code String},
 * {@code float[]}, {@code List} ({@code ArrayList}), {@code Map}
 * ({@code LinkedHashMap}, in its order), {@code Instant} and {@code byte[]}:
 * new objects of the caller's, which it may change (a row read, changed and
 * written back).
 */
final class Codec {
    private Codec() {}

    static final byte NULL = 0;
    static final byte FALSE = 1;
    static final byte TRUE = 2;
    static final byte INT = 3;
    static final byte FLOAT = 4;
    static final byte STR = 5;
    static final byte VECTOR = 6;
    static final byte LIST = 7;
    static final byte MAP = 8;
    static final byte TIME = 9;
    static final byte BYTES = 10;

    /** Nesting deeper than this is refused (a map that holds itself). */
    static final int DEPTH = 128;

    static byte[] encode(Object value) {
        Out out = new Out();
        out.value(value, 0);
        return out.bytes();
    }

    static Object decode(byte[] b) {
        ByteBuffer in = ByteBuffer.wrap(b).order(ByteOrder.LITTLE_ENDIAN);
        Object v = read(in);
        if (in.hasRemaining()) {
            throw new IllegalStateException(in.remaining() + " bytes after the native library's answer");
        }
        return v;
    }

    /** Microseconds since 1970-01-01T00:00:00Z, which TIMESTAMP holds. */
    static long micros(Instant t) {
        return Math.addExact(Math.multiplyExact(t.getEpochSecond(), 1_000_000L), t.getNano() / 1_000);
    }

    static Instant instant(long micros) {
        return Instant.ofEpochSecond(Math.floorDiv(micros, 1_000_000L), Math.floorMod(micros, 1_000_000L) * 1_000L);
    }

    private static Object read(ByteBuffer in) {
        byte tag = in.get();
        switch (tag) {
            case NULL:
                return null;
            case FALSE:
                return Boolean.FALSE;
            case TRUE:
                return Boolean.TRUE;
            case INT:
                return in.getLong();
            case FLOAT:
                return in.getDouble();
            case STR:
                return string(in);
            case VECTOR: {
                float[] xs = new float[in.getInt()];
                for (int i = 0; i < xs.length; i++) {
                    xs[i] = in.getFloat();
                }
                return xs;
            }
            case LIST: {
                int n = in.getInt();
                List<Object> xs = new ArrayList<>(n);
                for (int i = 0; i < n; i++) {
                    xs.add(read(in));
                }
                return xs;
            }
            case MAP: {
                int n = in.getInt();
                Map<String, Object> m = new LinkedHashMap<>(Math.max(4, n * 4 / 3 + 1));
                for (int i = 0; i < n; i++) {
                    String k = string(in);
                    m.put(k, read(in));
                }
                return m;
            }
            case TIME:
                return instant(in.getLong());
            case BYTES: {
                byte[] b = new byte[in.getInt()];
                in.get(b);
                return b;
            }
            default:
                throw new IllegalStateException("unknown tag " + tag + " in the native library's answer");
        }
    }

    private static String string(ByteBuffer in) {
        int n = in.getInt();
        String s = new String(in.array(), in.arrayOffset() + in.position(), n, StandardCharsets.UTF_8);
        in.position(in.position() + n);
        return s;
    }

    /** A growing buffer. */
    private static final class Out {
        private byte[] b = new byte[256];
        private int n;

        byte[] bytes() {
            return Arrays.copyOf(b, n);
        }

        private void room(int more) {
            if (b.length - n < more) {
                long want = Math.max((long) b.length * 2, (long) n + more);
                if (want > Integer.MAX_VALUE - 8) {
                    throw new IllegalArgumentException("the values are more than 2 GB");
                }
                b = Arrays.copyOf(b, (int) want);
            }
        }

        private void tag(byte t) {
            room(1);
            b[n++] = t;
        }

        private void u32(int x) {
            room(4);
            b[n++] = (byte) x;
            b[n++] = (byte) (x >>> 8);
            b[n++] = (byte) (x >>> 16);
            b[n++] = (byte) (x >>> 24);
        }

        private void i64(long x) {
            room(8);
            for (int i = 0; i < 8; i++) {
                b[n++] = (byte) (x >>> (8 * i));
            }
        }

        private void str(String s) {
            byte[] u = s.getBytes(StandardCharsets.UTF_8);
            u32(u.length);
            room(u.length);
            System.arraycopy(u, 0, b, n, u.length);
            n += u.length;
        }

        private void list(int count) {
            tag(LIST);
            u32(count);
        }

        void value(Object v, int depth) {
            if (depth > DEPTH) {
                throw new IllegalArgumentException("values nest deeper than " + DEPTH + " (a map or list that holds itself?)");
            }
            if (v == null) {
                tag(NULL);
            } else if (v instanceof Boolean) {
                tag((Boolean) v ? TRUE : FALSE);
            } else if (v instanceof Long || v instanceof Integer || v instanceof Short || v instanceof Byte) {
                tag(INT);
                i64(((Number) v).longValue());
            } else if (v instanceof BigInteger) {
                tag(INT);
                try {
                    i64(((BigInteger) v).longValueExact());
                } catch (ArithmeticException e) {
                    throw new IllegalArgumentException(v + " does not fit an INT (64 bits)");
                }
            } else if (v instanceof Double || v instanceof Float || v instanceof BigDecimal) {
                tag(FLOAT);
                i64(Double.doubleToRawLongBits(((Number) v).doubleValue()));
            } else if (v instanceof CharSequence || v instanceof Character) {
                tag(STR);
                str(v.toString());
            } else if (v instanceof float[]) {
                float[] xs = (float[]) v;
                tag(VECTOR);
                u32(xs.length);
                room(xs.length * 4);
                for (float x : xs) {
                    int bits = Float.floatToRawIntBits(x);
                    b[n++] = (byte) bits;
                    b[n++] = (byte) (bits >>> 8);
                    b[n++] = (byte) (bits >>> 16);
                    b[n++] = (byte) (bits >>> 24);
                }
            } else if (v instanceof double[]) {
                double[] xs = (double[]) v;
                float[] fs = new float[xs.length];
                for (int i = 0; i < xs.length; i++) {
                    fs[i] = (float) xs[i];
                }
                value(fs, depth);
            } else if (v instanceof int[]) {
                int[] xs = (int[]) v;
                list(xs.length);
                for (int x : xs) {
                    tag(INT);
                    i64(x);
                }
            } else if (v instanceof long[]) {
                long[] xs = (long[]) v;
                list(xs.length);
                for (long x : xs) {
                    tag(INT);
                    i64(x);
                }
            } else if (v instanceof byte[]) {
                byte[] xs = (byte[]) v;
                tag(BYTES);
                u32(xs.length);
                room(xs.length);
                System.arraycopy(xs, 0, b, n, xs.length);
                n += xs.length;
            } else if (v instanceof Object[]) {
                Object[] xs = (Object[]) v;
                list(xs.length);
                for (Object x : xs) {
                    value(x, depth + 1);
                }
            } else if (v instanceof Map) {
                Map<?, ?> m = (Map<?, ?>) v;
                tag(MAP);
                int at = n;
                u32(0);
                int count = 0;
                for (Map.Entry<?, ?> e : m.entrySet()) {
                    if (!(e.getKey() instanceof CharSequence)) {
                        throw new IllegalArgumentException("a map's keys are strings, not " + (e.getKey() == null ? "null" : e.getKey().getClass().getName()));
                    }
                    str(e.getKey().toString());
                    value(e.getValue(), depth + 1);
                    count++;
                }
                patch(at, count);
            } else if (v instanceof Iterable) {
                tag(LIST);
                int at = n;
                u32(0);
                int count = 0;
                for (Object x : (Iterable<?>) v) {
                    value(x, depth + 1);
                    count++;
                }
                patch(at, count);
            } else if (v instanceof Instant) {
                tag(TIME);
                i64(micros((Instant) v));
            } else if (v instanceof OffsetDateTime) {
                value(((OffsetDateTime) v).toInstant(), depth);
            } else if (v instanceof ZonedDateTime) {
                value(((ZonedDateTime) v).toInstant(), depth);
            } else if (v instanceof LocalDateTime) {
                value(((LocalDateTime) v).toInstant(ZoneOffset.UTC), depth);
            } else if (v instanceof LocalDate) {
                value(((LocalDate) v).atStartOfDay(ZoneOffset.UTC).toInstant(), depth);
            } else if (v instanceof Date) {
                value(((Date) v).toInstant(), depth);
            } else {
                throw new IllegalArgumentException("cannot store a " + v.getClass().getName() + ": values are null, Boolean, numbers, strings, float[] vectors, lists, maps, times (Instant and the java.time types) and byte[]");
            }
        }

        /** Writes a count where {@link #u32} left room for one. */
        private void patch(int at, int count) {
            b[at] = (byte) count;
            b[at + 1] = (byte) (count >>> 8);
            b[at + 2] = (byte) (count >>> 16);
            b[at + 3] = (byte) (count >>> 24);
        }
    }
}
