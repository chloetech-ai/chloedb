package ai.chloetech.strata.jdbc;

import static org.junit.jupiter.api.Assertions.assertArrayEquals;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertInstanceOf;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertSame;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import ai.chloetech.strata.Database;
import ai.chloetech.strata.Result;
import java.math.BigDecimal;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.sql.Array;
import java.sql.BatchUpdateException;
import java.sql.Connection;
import java.sql.DatabaseMetaData;
import java.sql.DriverManager;
import java.sql.PreparedStatement;
import java.sql.ResultSet;
import java.sql.ResultSetMetaData;
import java.sql.SQLException;
import java.sql.SQLFeatureNotSupportedException;
import java.sql.SQLSyntaxErrorException;
import java.sql.Statement;
import java.sql.Timestamp;
import java.sql.Types;
import java.time.Instant;
import java.time.LocalDate;
import java.time.OffsetDateTime;
import java.time.ZoneOffset;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;
import java.util.Properties;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class JdbcTest {
    static final String SCHEMA = "{\"table\": \"books\", \"key\": \"id\", \"fields\": ["
            + "{\"name\": \"id\", \"type\": \"keyword\"},"
            + "{\"name\": \"title\", \"type\": \"text\"},"
            + "{\"name\": \"year\", \"type\": \"int\", \"not_null\": true, \"default\": 2000},"
            + "{\"name\": \"rating\", \"type\": \"float\"},"
            + "{\"name\": \"done\", \"type\": \"bool\"},"
            + "{\"name\": \"at\", \"type\": \"timestamp\"},"
            + "{\"name\": \"tags\", \"type\": \"keyword\", \"multi\": true},"
            + "{\"name\": \"v\", \"type\": \"vector\", \"dim\": 3}]}";

    @TempDir
    Path dir;

    Path schema;
    String url;
    Connection conn;

    @BeforeEach
    void open() throws Exception {
        schema = dir.resolve("books.json");
        Files.write(schema, SCHEMA.getBytes(StandardCharsets.UTF_8));
        url = "jdbc:strata:" + dir.resolve("books.strata");
        conn = DriverManager.getConnection(url + "?schema=" + schema);
        try (PreparedStatement ps = conn.prepareStatement("INSERT INTO books (id, title, year, rating, done, at, v) VALUES (?, ?, ?, ?, ?, ?, ?)")) {
            for (int i = 0; i < 20; i++) {
                ps.setString(1, String.format("b%02d", i));
                ps.setString(2, (i % 2 == 0 ? "dragon " : "river ") + "book " + i);
                ps.setInt(3, 1990 + i);
                ps.setDouble(4, i / 4.0);
                ps.setBoolean(5, i % 3 == 0);
                ps.setTimestamp(6, Timestamp.from(Instant.parse("2026-10-01T00:00:00Z").plusSeconds(3600L * i)));
                ps.setObject(7, new float[] {i, 1, 0});
                ps.addBatch();
            }
            assertArrayEquals(ones(20), ps.executeBatch());
        }
    }

    @AfterEach
    void close() throws Exception {
        conn.close();
    }

    static int[] ones(int n) {
        int[] xs = new int[n];
        Arrays.fill(xs, 1);
        return xs;
    }

    @Test
    void theDriverIsFoundByItsUrl() throws Exception {
        assertInstanceOf(StrataDriver.class, DriverManager.getDriver("jdbc:strata:any.strata"));
        assertFalse(new StrataDriver().acceptsURL("jdbc:sqlite:any.db"));
        assertNull(new StrataDriver().connect("jdbc:sqlite:any.db", new Properties()));
        assertEquals("Strata", conn.getMetaData().getDatabaseProductName());
        assertEquals(url + "?schema=" + schema, conn.getMetaData().getURL());
    }

    @Test
    void connectOpensWhatCreateMade() throws Exception {
        conn.close();
        // Without a schema the file must exist.
        SQLException missing = assertThrows(SQLException.class, () -> DriverManager.getConnection("jdbc:strata:" + dir.resolve("none.strata")));
        assertTrue(missing.getMessage().contains("does not exist"), missing.getMessage());
        SQLException option = assertThrows(SQLException.class, () -> DriverManager.getConnection(url + "?thread=2"));
        assertEquals("08001", option.getSQLState());
        assertTrue(option.getMessage().contains("thread"), option.getMessage());
        Properties p = new Properties();
        p.setProperty("threads", "2");
        conn = DriverManager.getConnection("jdbc:strata:file:" + dir.resolve("books.strata"), p);
        try (Statement s = conn.createStatement(); ResultSet rs = s.executeQuery("SELECT COUNT(*) FROM books")) {
            assertTrue(rs.next());
            assertEquals(20, rs.getInt(1));
            assertFalse(rs.next());
        }
        assertThrows(SQLException.class, () -> DriverManager.getConnection(url + "?threads=many"));
    }

    @Test
    void parametersAndValues() throws Exception {
        try (PreparedStatement ps = conn.prepareStatement("SELECT id, title, year, rating, done, at, tags, v FROM books WHERE year >= ? AND title LIKE ? ORDER BY year LIMIT ?")) {
            assertEquals(3, ps.getParameterMetaData().getParameterCount());
            ps.setLong(1, 2000);
            ps.setString(2, "dragon%");
            ps.setInt(3, 2);
            try (ResultSet rs = ps.executeQuery()) {
                assertTrue(rs.next());
                assertEquals("b10", rs.getString("ID"));
                assertEquals("b10", rs.getString(1));
                assertEquals("dragon book 10", rs.getString("title"));
                assertEquals(2000, rs.getInt("year"));
                assertEquals(2000L, rs.getObject("year"));
                assertEquals(2.5, rs.getDouble("rating"));
                assertEquals(new BigDecimal("2.5"), rs.getBigDecimal("rating"));
                assertEquals(2, rs.getInt("rating"));
                assertEquals("2.5", rs.getString("rating"));
                assertFalse(rs.getBoolean("done"));
                assertFalse(rs.wasNull());
                Instant at = Instant.parse("2026-10-01T10:00:00Z");
                assertEquals(Timestamp.from(at), rs.getTimestamp("at"));
                assertEquals(Timestamp.from(at), rs.getObject("at"));
                assertEquals(at, rs.getObject("at", Instant.class));
                assertEquals(at.atOffset(ZoneOffset.UTC), rs.getObject("at", OffsetDateTime.class));
                assertEquals(LocalDate.of(2026, 10, 1), rs.getDate("at").toLocalDate());
                assertEquals("2026-10-01T10:00:00Z", rs.getString("at"));
                assertNull(rs.getObject("tags"));
                assertTrue(rs.wasNull());
                assertEquals(0, rs.getInt("tags"));
                assertArrayEquals(new float[] {10, 1, 0}, (float[]) rs.getObject("v"));
                assertArrayEquals(new float[] {10, 1, 0}, rs.getObject("v", float[].class));
                assertEquals("[10.0,1.0,0.0]", rs.getString("v"));
                Array v = rs.getArray("v");
                assertEquals(Types.REAL, v.getBaseType());
                assertArrayEquals(new Float[] {10f, 1f, 0f}, (Float[]) v.getArray());
                assertArrayEquals(new Float[] {1f}, (Float[]) v.getArray(2, 1));
                assertTrue(rs.next());
                assertEquals("b12", rs.getString(1));
                assertFalse(rs.next());
                SQLException after = assertThrows(SQLException.class, () -> rs.getString(1));
                assertEquals("24000", after.getSQLState());
                assertThrows(SQLException.class, () -> rs.getString("nope"));
            }
            // A parameter left unset is an error, not a NULL.
            ps.clearParameters();
            ps.setLong(1, 2000);
            SQLException unset = assertThrows(SQLException.class, ps::executeQuery);
            assertEquals("07001", unset.getSQLState());
            assertThrows(SQLException.class, () -> ps.setString(4, "x"));
            // A number where an int goes, text that is a number.
            ps.setString(2, "%");
            ps.setObject(3, 1);
            try (ResultSet rs = ps.executeQuery()) {
                assertTrue(rs.next());
                assertEquals("b10", rs.getString(1));
                assertThrows(SQLException.class, () -> rs.getInt("title"));
                assertThrows(SQLException.class, () -> rs.getTimestamp("year"));
            }
        }
        // A ? in a string or a comment is not a parameter.
        try (PreparedStatement ps = conn.prepareStatement("SELECT id, 'what?' AS q FROM books -- any?\n WHERE id = ? /* or? */")) {
            assertEquals(1, ps.getParameterMetaData().getParameterCount());
            ps.setString(1, "b03");
            try (ResultSet rs = ps.executeQuery()) {
                assertTrue(rs.next());
                assertEquals("what?", rs.getString("q"));
            }
        }
        // Vectors as parameters, and NULLs.
        try (PreparedStatement ps = conn.prepareStatement("SELECT id FROM books ORDER BY similarity(v, ?) DESC LIMIT 1")) {
            ps.setObject(1, new float[] {-1, 1, 0});
            try (ResultSet rs = ps.executeQuery()) {
                assertTrue(rs.next());
                assertEquals("b00", rs.getString(1));
            }
        }
        try (PreparedStatement ps = conn.prepareStatement("UPDATE books SET rating = ?, done = ?, at = ? WHERE id = ?")) {
            ps.setNull(1, Types.DOUBLE);
            ps.setObject(2, true);
            ps.setDate(3, java.sql.Date.valueOf(LocalDate.of(2026, 11, 3)));
            ps.setString(4, "b01");
            assertEquals(1, ps.executeUpdate());
        }
        try (Statement s = conn.createStatement(); ResultSet rs = s.executeQuery("SELECT rating, done, at FROM books WHERE id = 'b01'")) {
            assertTrue(rs.next());
            assertEquals(0.0, rs.getDouble(1));
            assertTrue(rs.wasNull());
            assertNull(rs.getBigDecimal(1));
            assertTrue(rs.getBoolean(2));
            assertEquals(Instant.parse("2026-11-03T00:00:00Z"), rs.getTimestamp(3).toInstant());
        }
    }

    @Test
    void theColumnsOfAResult() throws Exception {
        try (Statement s = conn.createStatement(); ResultSet rs = s.executeQuery("SELECT id, year, rating, done, at, v, year * 2 AS twice, tags FROM books")) {
            ResultSetMetaData m = rs.getMetaData();
            assertEquals(8, m.getColumnCount());
            assertEquals("twice", m.getColumnLabel(7));
            int[] types = {Types.VARCHAR, Types.BIGINT, Types.DOUBLE, Types.BOOLEAN, Types.TIMESTAMP, Types.ARRAY, Types.BIGINT, Types.OTHER};
            String[] names = {"TEXT", "INT", "FLOAT", "BOOL", "TIMESTAMP", "VECTOR", "INT", "ANY"};
            for (int i = 0; i < types.length; i++) {
                assertEquals(types[i], m.getColumnType(i + 1), m.getColumnName(i + 1));
                assertEquals(names[i], m.getColumnTypeName(i + 1));
            }
            assertEquals(Timestamp.class.getName(), m.getColumnClassName(5));
            assertEquals(3, m.getPrecision(1));
            assertThrows(SQLException.class, () -> m.getColumnType(9));
            int n = 0;
            while (rs.next()) {
                n++;
            }
            assertEquals(20, n);
        }
    }

    @Test
    void statementsThatWrite() throws Exception {
        try (Statement s = conn.createStatement()) {
            assertFalse(s.execute("CREATE TABLE notes (id INT PRIMARY KEY, book KEYWORD REFERENCES books, body TEXT)"));
            assertEquals(0, s.getUpdateCount());
            assertEquals(2, s.executeUpdate("INSERT INTO notes (id, book, body) VALUES (1, 'b01', 'one'), (2, 'b02', 'two')"));
            assertTrue(s.execute("SELECT body FROM notes ORDER BY id"));
            assertEquals(-1, s.getUpdateCount());
            try (ResultSet rs = s.getResultSet()) {
                assertTrue(rs.next());
                assertEquals("one", rs.getString(1));
            }
            assertFalse(s.getMoreResults());
            assertEquals(-1, s.getUpdateCount());
            assertEquals(1, s.executeLargeUpdate("DELETE FROM notes WHERE id = 2"));
            // executeQuery runs what answers rows, executeUpdate what writes.
            assertThrows(SQLException.class, () -> s.executeQuery("DELETE FROM notes"));
            assertThrows(SQLException.class, () -> s.executeUpdate("SELECT * FROM notes"));
            s.addBatch("INSERT INTO notes (id, body) VALUES (10, 'a')");
            s.addBatch("UPDATE notes SET body = 'b' WHERE id = 10");
            s.addBatch("UPDATE notes SET body = 'c' WHERE id = 99");
            assertArrayEquals(new int[] {1, 1, 0}, s.executeBatch());
            s.addBatch("INSERT INTO notes (id, body) VALUES (11, 'a')");
            s.addBatch("INSERT INTO nowhere (id) VALUES (1)");
            s.addBatch("INSERT INTO notes (id, body) VALUES (12, 'a')");
            BatchUpdateException e = assertThrows(BatchUpdateException.class, s::executeBatch);
            assertArrayEquals(new long[] {1}, e.getLargeUpdateCounts());
            try (ResultSet rs = s.executeQuery("SELECT COUNT(*) FROM notes")) {
                rs.next();
                assertEquals(3, rs.getLong(1));
            }
            s.setMaxRows(2);
            try (ResultSet rs = s.executeQuery("SELECT id FROM books")) {
                assertTrue(rs.next() && rs.next());
                assertFalse(rs.next());
            }
        }
    }

    @Test
    void connectionsShareTheFile() throws Exception {
        Connection other = DriverManager.getConnection(url);
        try (Statement s = other.createStatement()) {
            s.executeUpdate("INSERT INTO books (id, title) VALUES ('new', 'seen by both')");
        }
        try (Statement s = conn.createStatement(); ResultSet rs = s.executeQuery("SELECT title, year FROM books WHERE id = 'new'")) {
            assertTrue(rs.next());
            assertEquals("seen by both", rs.getString(1));
            assertEquals(2000, rs.getInt(2), "the default");
        }
        assertSame(conn.unwrap(Database.class), other.unwrap(Database.class));
        // A link to the file is the same database.
        Path link = Files.createSymbolicLink(dir.resolve("link.strata"), dir.resolve("books.strata"));
        try (Connection linked = DriverManager.getConnection("jdbc:strata:" + link)) {
            assertSame(conn.unwrap(Database.class), linked.unwrap(Database.class));
        }
        other.close();
        assertTrue(other.isClosed());
        assertThrows(SQLException.class, other::createStatement);
        // The other connection still has it open.
        assertFalse(conn.unwrap(Database.class).isClosed());
        Database db = conn.unwrap(Database.class);
        conn.close();
        assertTrue(db.isClosed());
        // Closed by the last connection, so the process may open it again.
        try (Database again = Database.open(dir.resolve("books.strata"))) {
            assertEquals(21, again.count());
        }
        conn = DriverManager.getConnection(url);
    }

    @Test
    void theDatabaseBehindTheConnection() throws Exception {
        assertTrue(conn.isWrapperFor(Database.class));
        Database db = conn.unwrap(Database.class);
        Result r = db.query("SELECT id FROM books WHERE id = :id", java.util.Collections.singletonMap("id", "b05"));
        assertEquals("b05", r.scalar());
        assertEquals(20, db.count());
        assertThrows(SQLException.class, () -> conn.unwrap(String.class));
    }

    @Test
    void readOnlyConnections() throws Exception {
        try (Connection ro = DriverManager.getConnection(url + "?readonly=true"); Statement s = ro.createStatement()) {
            assertTrue(ro.isReadOnly());
            assertTrue(ro.getMetaData().isReadOnly());
            SQLException e = assertThrows(SQLException.class, () -> s.executeUpdate("DELETE FROM books"));
            assertEquals("25006", e.getSQLState());
            try (ResultSet rs = s.executeQuery("SELECT COUNT(*) FROM books")) {
                rs.next();
                assertEquals(20, rs.getInt(1));
            }
        }
        assertFalse(conn.isReadOnly());
    }

    @Test
    void errorsAreSqlExceptions() throws Exception {
        try (Statement s = conn.createStatement()) {
            assertThrows(SQLSyntaxErrorException.class, () -> s.executeQuery("SELECT FROM WHERE"));
            SQLException e = assertThrows(SQLException.class, () -> s.executeQuery("SELECT nope FROM books"));
            assertTrue(e.getMessage().contains("nope"), e.getMessage());
        }
        assertTrue(conn.getAutoCommit());
        conn.setAutoCommit(true);
        assertThrows(SQLFeatureNotSupportedException.class, () -> conn.setAutoCommit(false));
        assertThrows(SQLException.class, conn::commit);
        assertThrows(SQLException.class, conn::rollback);
        assertThrows(SQLFeatureNotSupportedException.class, () -> conn.prepareCall("CALL x()"));
        assertThrows(SQLFeatureNotSupportedException.class, () -> conn.createStatement(ResultSet.TYPE_FORWARD_ONLY, ResultSet.CONCUR_UPDATABLE));
        try (PreparedStatement ps = conn.prepareStatement("SELECT id FROM books")) {
            assertThrows(SQLException.class, () -> ps.executeQuery("SELECT 1"));
            try (ResultSet rs = ps.executeQuery()) {
                rs.next();
                assertThrows(SQLFeatureNotSupportedException.class, () -> rs.updateString(1, "x"));
                assertThrows(SQLException.class, rs::previous, "forward-only");
            }
        }
        Statement s = conn.createStatement();
        s.close();
        assertThrows(SQLException.class, () -> s.executeQuery("SELECT 1"));
    }

    @Test
    void resultsScrollWhenAsked() throws Exception {
        try (Statement s = conn.createStatement(ResultSet.TYPE_SCROLL_INSENSITIVE, ResultSet.CONCUR_READ_ONLY);
                ResultSet rs = s.executeQuery("SELECT id FROM books ORDER BY id")) {
            assertEquals(ResultSet.TYPE_SCROLL_INSENSITIVE, rs.getType());
            assertTrue(rs.isBeforeFirst());
            assertTrue(rs.last());
            assertEquals("b19", rs.getString(1));
            assertEquals(20, rs.getRow());
            assertTrue(rs.absolute(3));
            assertEquals("b02", rs.getString(1));
            assertTrue(rs.previous());
            assertEquals("b01", rs.getString(1));
            assertTrue(rs.relative(-1));
            assertTrue(rs.isFirst());
            assertTrue(rs.absolute(-2));
            assertEquals("b18", rs.getString(1));
            assertFalse(rs.relative(5));
            assertTrue(rs.isAfterLast());
            rs.beforeFirst();
            assertTrue(rs.next());
            assertEquals("b00", rs.getString(1));
        }
        try (Statement s = conn.createStatement()) {
            s.closeOnCompletion();
            ResultSet rs = s.executeQuery("SELECT id FROM books");
            rs.close();
            assertTrue(s.isClosed());
        }
    }

    @Test
    void metadataDescribesTheTables() throws Exception {
        try (Statement s = conn.createStatement()) {
            s.execute("CREATE TABLE notes (id INT PRIMARY KEY, book KEYWORD REFERENCES books, body TEXT NOT NULL)");
        }
        DatabaseMetaData m = conn.getMetaData();
        assertEquals(Arrays.asList("books", "notes"), strings(m.getTables(null, null, "%", null), "TABLE_NAME"));
        assertEquals(Arrays.asList("notes"), strings(m.getTables(null, null, "NOT_S", new String[] {"TABLE"}), "TABLE_NAME"));
        assertEquals(Arrays.asList(), strings(m.getTables(null, null, "%", new String[] {"VIEW"}), "TABLE_NAME"));
        assertEquals(Arrays.asList(), strings(m.getTables(null, "other", "%", null), "TABLE_NAME"));
        assertEquals(Arrays.asList("TABLE"), strings(m.getTableTypes(), "TABLE_TYPE"));

        try (ResultSet rs = m.getColumns(null, null, "books", "%")) {
            List<String> names = new ArrayList<>();
            while (rs.next()) {
                names.add(rs.getString("COLUMN_NAME") + " " + rs.getString("TYPE_NAME") + " " + rs.getInt("DATA_TYPE") + " " + rs.getString("IS_NULLABLE") + " " + rs.getInt("ORDINAL_POSITION"));
                if (rs.getString("COLUMN_NAME").equals("year")) {
                    assertEquals("2000", rs.getString("COLUMN_DEF"));
                    assertEquals(DatabaseMetaData.columnNoNulls, rs.getInt("NULLABLE"));
                }
                if (rs.getString("COLUMN_NAME").equals("v")) {
                    assertEquals(3, rs.getInt("COLUMN_SIZE"));
                }
            }
            assertEquals(Arrays.asList(
                    "id KEYWORD 12 NO 1", "title TEXT 12 YES 2", "year INT -5 NO 3", "rating FLOAT 8 YES 4", "done BOOL 16 YES 5",
                    "at TIMESTAMP 93 YES 6", "tags KEYWORD[] 2003 YES 7", "v VECTOR(3) 2003 YES 8"), names);
        }
        assertEquals(Arrays.asList("body"), strings(m.getColumns(null, null, "notes", "b_dy"), "COLUMN_NAME"));
        assertEquals(Arrays.asList("NO"), strings(m.getColumns(null, null, "notes", "body"), "IS_NULLABLE"));
        assertEquals(Arrays.asList("id"), strings(m.getPrimaryKeys(null, null, "notes"), "COLUMN_NAME"));
        assertEquals(Arrays.asList("books"), strings(m.getImportedKeys(null, null, "notes"), "PKTABLE_NAME"));
        assertEquals(Arrays.asList("book"), strings(m.getExportedKeys(null, null, "books"), "FKCOLUMN_NAME"));
        assertEquals(Arrays.asList("id"), strings(m.getCrossReference(null, null, "books", null, null, "notes"), "PKCOLUMN_NAME"));
        assertEquals(Arrays.asList(), strings(m.getImportedKeys(null, null, "books"), "PKTABLE_NAME"));
        assertEquals(Arrays.asList("id"), strings(m.getIndexInfo(null, null, "books", true, false), "COLUMN_NAME"));
        assertEquals(Arrays.asList("id"), strings(m.getBestRowIdentifier(null, null, "books", DatabaseMetaData.bestRowSession, false), "COLUMN_NAME"));
        assertTrue(strings(m.getTypeInfo(), "TYPE_NAME").containsAll(Arrays.asList("TEXT", "KEYWORD", "INT", "FLOAT", "BOOL", "TIMESTAMP", "VECTOR")));
        assertEquals(Arrays.asList(), strings(m.getSchemas(), "TABLE_SCHEM"));
        assertEquals(Arrays.asList(), strings(m.getProcedures(null, null, "%"), "PROCEDURE_NAME"));
        assertFalse(m.supportsTransactions());
        assertTrue(m.supportsBatchUpdates());
        assertTrue(m.supportsResultSetType(ResultSet.TYPE_SCROLL_INSENSITIVE));
        assertEquals("\"", m.getIdentifierQuoteString());
        assertEquals(Connection.TRANSACTION_NONE, m.getDefaultTransactionIsolation());
        assertTrue(m.getDatabaseProductVersion().matches("\\d+\\.\\d+\\.\\d+.*"));
    }

    static List<String> strings(ResultSet rs, String column) throws SQLException {
        List<String> out = new ArrayList<>();
        try (ResultSet r = rs) {
            while (r.next()) {
                out.add(r.getString(column));
            }
        }
        return out;
    }

    @Test
    void placeholdersAndStatementKinds() {
        Sql.Rewritten r = Sql.placeholders("SELECT ?, '?', \"a?\", `b?` -- ?\n, ? /* ? */ FROM t WHERE x = 'it''s?' AND y = ?");
        assertEquals("SELECT :p1, '?', \"a?\", `b?` -- ?\n, :p2 /* ? */ FROM t WHERE x = 'it''s?' AND y = :p3", r.sql);
        assertEquals(3, r.params);
        assertTrue(Sql.answersRows("  select 1"));
        assertTrue(Sql.answersRows("-- note\n(SELECT 1) UNION (SELECT 2)"));
        assertTrue(Sql.answersRows("/* c */ WITH x AS (SELECT 1) SELECT * FROM x"));
        assertTrue(Sql.answersRows("EXPLAIN SELECT 1"));
        assertFalse(Sql.answersRows("INSERT INTO t VALUES (1)"));
        assertFalse(Sql.answersRows("update t set x = 1"));
        assertFalse(Sql.answersRows(""));
        assertTrue(StrataDatabaseMetaData.like("b%", "Books"));
        assertTrue(StrataDatabaseMetaData.like("bo_ks", "books"));
        assertFalse(StrataDatabaseMetaData.like("bo\\_ks", "books"));
        assertTrue(StrataDatabaseMetaData.like("bo\\_ks", "bo_ks"));
        assertTrue(StrataDatabaseMetaData.like(null, "x"));
        assertFalse(StrataDatabaseMetaData.like("a.c", "abc"));
    }
}
