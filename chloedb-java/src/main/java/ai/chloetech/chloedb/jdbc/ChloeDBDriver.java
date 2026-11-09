package ai.chloetech.chloedb.jdbc;

import ai.chloetech.chloedb.Database;
import java.io.UnsupportedEncodingException;
import java.net.URLDecoder;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.sql.Connection;
import java.sql.Driver;
import java.sql.DriverManager;
import java.sql.DriverPropertyInfo;
import java.sql.SQLException;
import java.sql.SQLFeatureNotSupportedException;
import java.util.HashMap;
import java.util.Map;
import java.util.Properties;
import java.util.logging.Logger;

/**
 * ChloeDB's JDBC driver: {@code jdbc:chloedb:PATH} (or {@code jdbc:strata:PATH},
 * by its old name) opens the database file (or
 * directory) at {@code PATH} in this process, as {@code jdbc:sqlite:PATH}
 * does a SQLite file. No server is involved.
 *
 * <pre>{@code
 * try (Connection c = DriverManager.getConnection("jdbc:chloedb:books.strata");
 *      PreparedStatement ps = c.prepareStatement("SELECT id, title FROM books ORDER BY bm25(title, ?) DESC LIMIT 5")) {
 *     ps.setString(1, "dragon");
 *     try (ResultSet rs = ps.executeQuery()) {
 *         while (rs.next()) System.out.println(rs.getString("id") + " " + rs.getString("title"));
 *     }
 * }
 * }</pre>
 *
 * <p>Options follow a {@code ?} in the URL ({@code jdbc:chloedb:books.strata?threads=4})
 * or come as connection properties:
 * <ul>
 *   <li>{@code schema}: the path of a schema's JSON file, with which a
 *       database that does not exist yet is created;</li>
 *   <li>{@code models}: the directory of embedding models;</li>
 *   <li>{@code threads}: the cores queries use;</li>
 *   <li>{@code readonly}: {@code true} refuses statements that write.</li>
 * </ul>
 *
 * <p>A process may have a database open once, so the connections to one
 * file share one {@link Database}, which the last of them to close closes;
 * {@code conn.unwrap(Database.class)} gives it, for what SQL does not do
 * ({@code search}, {@code importParquet}, ...). Each statement commits as it
 * runs: there are no transactions across statements, and
 * {@code setAutoCommit(false)} is refused.
 */
public final class ChloeDBDriver implements Driver {
    /** The prefix of the URLs this driver takes. */
    public static final String PREFIX = "jdbc:chloedb:";
    /** The prefix by the database's old name (it was called Strata), taken as well. */
    public static final String OLD_PREFIX = "jdbc:strata:";

    static {
        try {
            DriverManager.registerDriver(new ChloeDBDriver());
        } catch (SQLException e) {
            throw new ExceptionInInitializerError(e);
        }
    }

    @Override
    public boolean acceptsURL(String url) {
        return url != null && (url.startsWith(PREFIX) || url.startsWith(OLD_PREFIX));
    }

    @Override
    public Connection connect(String url, Properties info) throws SQLException {
        if (!acceptsURL(url)) {
            return null;
        }
        String rest = url.substring(url.startsWith(OLD_PREFIX) ? OLD_PREFIX.length() : PREFIX.length());
        Map<String, String> options = new HashMap<>();
        if (info != null) {
            for (String k : info.stringPropertyNames()) {
                options.put(k, info.getProperty(k));
            }
        }
        int q = rest.indexOf('?');
        if (q >= 0) {
            for (String kv : rest.substring(q + 1).split("&")) {
                if (kv.isEmpty()) {
                    continue;
                }
                int eq = kv.indexOf('=');
                options.put(decode(eq < 0 ? kv : kv.substring(0, eq)), eq < 0 ? "true" : decode(kv.substring(eq + 1)));
            }
            rest = rest.substring(0, q);
        }
        if (rest.startsWith("file:")) {
            rest = rest.substring("file:".length());
        }
        if (rest.isEmpty()) {
            throw new SQLException("a ChloeDB URL names a database: jdbc:chloedb:/path/to/file.strata", "08001");
        }
        for (String k : options.keySet()) {
            if (!k.equals("schema") && !k.equals("models") && !k.equals("threads") && !k.equals("readonly") && !k.equals("user") && !k.equals("password")) {
                throw new SQLException("unknown option " + k + " in " + url + ": schema, models, threads, readonly", "08001");
            }
        }
        Path path = Paths.get(rest).toAbsolutePath().normalize();
        // One file by any name (a link, another spelling) is one database.
        try {
            path = realPath(path);
        } catch (java.io.IOException e) {
            throw new SQLException("cannot read " + path + ": " + e.getMessage(), "08001", e);
        }
        Path file = path;
        Database.Options opts = new Database.Options();
        if (options.get("models") != null) {
            opts.models(options.get("models"));
        }
        if (options.get("threads") != null) {
            try {
                opts.threads(Integer.parseInt(options.get("threads")));
            } catch (IllegalArgumentException e) {
                throw new SQLException("threads: a count of cores, not " + options.get("threads"), "08001");
            }
        }
        String schema = options.get("schema");
        Shared.Entry db = Shared.open(file, () -> schema != null && !Files.exists(file) ? Database.create(file, Paths.get(schema), opts) : Database.open(file, opts));
        ChloeDBConnection c = new ChloeDBConnection(db, url);
        c.setReadOnly(Boolean.parseBoolean(options.get("readonly")));
        return c;
    }

    /**
     * The file's real path, links resolved; for a file not made yet, its
     * directory's real path and its name. A file a connection creates is
     * then named as the next connection names it once it exists, also
     * through a linked directory (on macOS the temporary directories are
     * under /var, a link to /private/var).
     */
    static Path realPath(Path path) throws java.io.IOException {
        if (Files.exists(path)) {
            return path.toRealPath();
        }
        Path parent = path.getParent();
        return parent == null ? path : realPath(parent).resolve(path.getFileName());
    }

    private static String decode(String s) {
        try {
            return URLDecoder.decode(s, "UTF-8");
        } catch (UnsupportedEncodingException e) {
            throw new AssertionError(e);
        }
    }

    @Override
    public DriverPropertyInfo[] getPropertyInfo(String url, Properties info) {
        DriverPropertyInfo schema = new DriverPropertyInfo("schema", null);
        schema.description = "The path of a schema's JSON file, to create a database that does not exist";
        DriverPropertyInfo models = new DriverPropertyInfo("models", null);
        models.description = "The directory of embedding models";
        DriverPropertyInfo threads = new DriverPropertyInfo("threads", null);
        threads.description = "The cores queries use (the machine's by default)";
        DriverPropertyInfo readonly = new DriverPropertyInfo("readonly", "false");
        readonly.description = "Refuse statements that write";
        readonly.choices = new String[] {"true", "false"};
        return new DriverPropertyInfo[] {schema, models, threads, readonly};
    }

    @Override
    public int getMajorVersion() {
        return 0;
    }

    @Override
    public int getMinorVersion() {
        return 1;
    }

    /** No: it does not pass the JDBC compliance tests (no transactions across statements, no stored procedures). */
    @Override
    public boolean jdbcCompliant() {
        return false;
    }

    @Override
    public Logger getParentLogger() throws SQLFeatureNotSupportedException {
        throw Sql.unsupported("java.util.logging");
    }
}
