package ai.chloetech.chloedb.jdbc;

import ai.chloetech.chloedb.ChloeDBException;
import java.sql.SQLException;
import java.sql.SQLFeatureNotSupportedException;
import java.sql.SQLNonTransientConnectionException;
import java.sql.SQLSyntaxErrorException;
import java.util.Locale;

/** SQL text as JDBC gives it, and errors as JDBC throws them. */
final class Sql {
    private Sql() {}

    /** The SQL with each {@code ?} placeholder outside quotes and comments as {@code :p1}, {@code :p2}, ...; and their count. */
    static final class Rewritten {
        final String sql;
        final int params;

        Rewritten(String sql, int params) {
            this.sql = sql;
            this.params = params;
        }
    }

    static String param(int i) {
        return "p" + i;
    }

    static Rewritten placeholders(String sql) {
        StringBuilder out = new StringBuilder(sql.length() + 16);
        int n = 0;
        int i = 0;
        while (i < sql.length()) {
            char c = sql.charAt(i);
            int end = skip(sql, i);
            if (end > i) {
                out.append(sql, i, end);
                i = end;
            } else if (c == '?') {
                out.append(':').append(param(++n));
                i++;
            } else {
                out.append(c);
                i++;
            }
        }
        return new Rewritten(out.toString(), n);
    }

    /**
     * The end of the quoted string, quoted name or comment at {@code i}, or
     * {@code i} if none starts there.
     */
    private static int skip(String s, int i) {
        char c = s.charAt(i);
        if (c == '\'' || c == '"' || c == '`') {
            int j = i + 1;
            while (j < s.length()) {
                if (s.charAt(j) == c) {
                    // A doubled quote is one quote inside.
                    if (j + 1 < s.length() && s.charAt(j + 1) == c) {
                        j += 2;
                        continue;
                    }
                    return j + 1;
                }
                j++;
            }
            return s.length();
        }
        if (c == '-' && i + 1 < s.length() && s.charAt(i + 1) == '-') {
            int j = s.indexOf('\n', i);
            return j < 0 ? s.length() : j + 1;
        }
        if (c == '/' && i + 1 < s.length() && s.charAt(i + 1) == '*') {
            int j = s.indexOf("*/", i + 2);
            return j < 0 ? s.length() : j + 2;
        }
        return i;
    }

    /** Whether a statement answers rows: a SELECT, a WITH or an EXPLAIN (after any comments and parentheses). */
    static boolean answersRows(String sql) {
        int i = 0;
        while (i < sql.length()) {
            char c = sql.charAt(i);
            int end = c == '\'' || c == '"' || c == '`' ? i : skip(sql, i);
            if (end > i) {
                i = end;
            } else if (Character.isWhitespace(c) || c == '(') {
                i++;
            } else {
                break;
            }
        }
        int j = i;
        while (j < sql.length() && Character.isLetter(sql.charAt(j))) {
            j++;
        }
        String word = sql.substring(i, j).toUpperCase(Locale.ROOT);
        return word.equals("SELECT") || word.equals("WITH") || word.equals("EXPLAIN") || word.equals("VALUES");
    }

    /** A ChloeDB error as JDBC's. */
    static SQLException error(RuntimeException e) {
        String m = e.getMessage() == null ? e.toString() : e.getMessage();
        if (e instanceof ChloeDBException) {
            return m.startsWith("query error") ? new SQLSyntaxErrorException(m, "42000", e) : new SQLException(m, "HY000", e);
        }
        if (e instanceof IllegalStateException) {
            return new SQLNonTransientConnectionException(m, "08003", e);
        }
        return new SQLException(m, "22023", e);
    }

    static SQLFeatureNotSupportedException unsupported(String what) {
        return new SQLFeatureNotSupportedException("ChloeDB's JDBC driver does not support " + what, "0A000");
    }
}
