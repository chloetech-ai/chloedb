package ai.chloetech.chloedb.jdbc;

import ai.chloetech.chloedb.Database;
import java.sql.Connection;
import java.sql.ResultSet;
import java.sql.RowIdLifetime;
import java.sql.SQLException;
import java.sql.Types;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Comparator;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.regex.Pattern;

/**
 * What a ChloeDB database holds and what its SQL does, for tools that ask
 * (DBeaver, IntelliJ, Spark, ...). Tables have no catalog or schema; table
 * patterns match names regardless of case, as ChloeDB's SQL does.
 */
final class ChloeDBDatabaseMetaData implements java.sql.DatabaseMetaData {
    private final ChloeDBConnection conn;

    ChloeDBDatabaseMetaData(ChloeDBConnection conn) {
        this.conn = conn;
    }

    // The result sets metadata answers.

    private static ResultSet rows(String[] columns, List<List<Object>> rows) {
        return new ChloeDBResultSet(null, Arrays.asList(columns), rows);
    }

    private static ResultSet none(String... columns) {
        return rows(columns, new ArrayList<>());
    }

    private static List<Object> row(Object... values) {
        Object[] vs = values.clone();
        for (int i = 0; i < vs.length; i++) {
            if (vs[i] instanceof Integer || vs[i] instanceof Short) {
                vs[i] = ((Number) vs[i]).longValue();
            }
        }
        return Arrays.asList(vs);
    }

    /** Whether {@code name} matches a JDBC pattern ({@code %} any text, {@code _} one character, {@code \} escapes); null matches all. */
    static boolean like(String pattern, String name) {
        if (pattern == null) {
            return true;
        }
        StringBuilder re = new StringBuilder();
        for (int i = 0; i < pattern.length(); i++) {
            char c = pattern.charAt(i);
            if (c == '\\' && i + 1 < pattern.length()) {
                re.append(Pattern.quote(String.valueOf(pattern.charAt(++i))));
            } else if (c == '%') {
                re.append(".*");
            } else if (c == '_') {
                re.append('.');
            } else {
                re.append(Pattern.quote(String.valueOf(c)));
            }
        }
        return Pattern.compile(re.toString(), Pattern.CASE_INSENSITIVE | Pattern.UNICODE_CASE | Pattern.DOTALL).matcher(name).matches();
    }

    /** Tables have no catalog or schema: a pattern for one matches only if it matches anything. */
    private static boolean noSchema(String catalog, String schemaPattern) {
        return (catalog == null || catalog.isEmpty()) && (schemaPattern == null || schemaPattern.isEmpty() || like(schemaPattern, ""));
    }

    private List<Map<String, Object>> tables(String catalog, String schemaPattern, String tablePattern) throws SQLException {
        List<Map<String, Object>> out = new ArrayList<>();
        if (!noSchema(catalog, schemaPattern)) {
            return out;
        }
        try {
            for (Map<String, Object> t : conn.db().tables()) {
                if (like(tablePattern, (String) t.get("table"))) {
                    out.add(t);
                }
            }
        } catch (RuntimeException e) {
            throw Sql.error(e);
        }
        out.sort(Comparator.comparing(t -> ((String) t.get("table")).toLowerCase(Locale.ROOT)));
        return out;
    }

    @SuppressWarnings("unchecked")
    private static List<Map<String, Object>> fields(Map<String, Object> table) {
        return (List<Map<String, Object>>) table.get("fields");
    }

    /** A field's type as JDBC names it: its SQL type, and its SQL name ({@code KEYWORD[]} for a list). */
    static Object[] type(Map<String, Object> f) {
        String t = String.valueOf(f.get("type")).toUpperCase(Locale.ROOT);
        boolean multi = Boolean.TRUE.equals(f.get("multi"));
        int sql;
        switch (t) {
            case "INT":
                sql = Types.BIGINT;
                break;
            case "FLOAT":
                sql = Types.DOUBLE;
                break;
            case "BOOL":
                sql = Types.BOOLEAN;
                break;
            case "TIMESTAMP":
                sql = Types.TIMESTAMP;
                break;
            case "VECTOR":
                sql = Types.ARRAY;
                break;
            default:
                // TEXT, KEYWORD, MEDIA, and JSON, SPARSE and EDGES, which read back as JSON text.
                sql = Types.VARCHAR;
        }
        Object dim = f.get("dim");
        String name = t.equals("VECTOR") && dim != null ? "VECTOR(" + dim + ")" : t;
        return new Object[] {multi ? Types.ARRAY : sql, multi ? name + "[]" : name};
    }

    private static Object size(Map<String, Object> f, int sql) {
        if (sql == Types.BIGINT) {
            return 19;
        }
        if (sql == Types.DOUBLE) {
            return 17;
        }
        if (sql == Types.BOOLEAN) {
            return 1;
        }
        if (sql == Types.TIMESTAMP) {
            return 26;
        }
        if (sql == Types.ARRAY && "vector".equals(f.get("type")) && !Boolean.TRUE.equals(f.get("multi"))) {
            return f.get("dim");
        }
        return null;
    }

    /** A default as SQL text. */
    private static String literal(Object d) {
        if (d == null) {
            return null;
        }
        if (d instanceof String) {
            return "'" + ((String) d).replace("'", "''") + "'";
        }
        if (d instanceof Boolean) {
            return ((Boolean) d) ? "TRUE" : "FALSE";
        }
        return Values.string(d);
    }

    private static boolean generated(Map<String, Object> f) {
        return f.containsKey("from") || f.containsKey("embed") || f.containsKey("derive");
    }

    private static String remarks(Map<String, Object> f) {
        Object e = f.get("embed");
        if (e instanceof Map) {
            Map<?, ?> m = (Map<?, ?>) e;
            return "embedded from " + Values.string(m.get("from")) + " by " + m.get("model");
        }
        Object from = f.get("from");
        return from == null ? null : "made from " + Values.string(from);
    }

    @Override
    public ResultSet getTables(String catalog, String schemaPattern, String tableNamePattern, String[] types) throws SQLException {
        String[] columns = {"TABLE_CAT", "TABLE_SCHEM", "TABLE_NAME", "TABLE_TYPE", "REMARKS", "TYPE_CAT", "TYPE_SCHEM", "TYPE_NAME", "SELF_REFERENCING_COL_NAME", "REF_GENERATION"};
        List<List<Object>> out = new ArrayList<>();
        boolean tablesToo = types == null;
        for (String t : types == null ? new String[0] : types) {
            tablesToo |= "TABLE".equalsIgnoreCase(t);
        }
        if (tablesToo) {
            for (Map<String, Object> t : tables(catalog, schemaPattern, tableNamePattern)) {
                out.add(row(null, null, t.get("table"), "TABLE", null, null, null, null, null, null));
            }
        }
        return rows(columns, out);
    }

    @Override
    public ResultSet getColumns(String catalog, String schemaPattern, String tableNamePattern, String columnNamePattern) throws SQLException {
        String[] columns = {"TABLE_CAT", "TABLE_SCHEM", "TABLE_NAME", "COLUMN_NAME", "DATA_TYPE", "TYPE_NAME", "COLUMN_SIZE", "BUFFER_LENGTH", "DECIMAL_DIGITS", "NUM_PREC_RADIX", "NULLABLE", "REMARKS", "COLUMN_DEF", "SQL_DATA_TYPE", "SQL_DATETIME_SUB", "CHAR_OCTET_LENGTH", "ORDINAL_POSITION", "IS_NULLABLE", "SCOPE_CATALOG", "SCOPE_SCHEMA", "SCOPE_TABLE", "SOURCE_DATA_TYPE", "IS_AUTOINCREMENT", "IS_GENERATEDCOLUMN"};
        List<List<Object>> out = new ArrayList<>();
        for (Map<String, Object> t : tables(catalog, schemaPattern, tableNamePattern)) {
            Object key = t.get("key");
            int position = 0;
            for (Map<String, Object> f : fields(t)) {
                position++;
                String name = (String) f.get("name");
                if (!like(columnNamePattern, name)) {
                    continue;
                }
                Object[] type = type(f);
                int sql = (Integer) type[0];
                boolean notNull = name.equals(key) || Boolean.TRUE.equals(f.get("not_null"));
                out.add(row(null, null, t.get("table"), name, sql, type[1], size(f, sql), null,
                        sql == Types.BIGINT ? 0 : null, sql == Types.BIGINT || sql == Types.DOUBLE ? 10 : null,
                        notNull ? columnNoNulls : columnNullable, remarks(f), literal(f.get("default")), null, null, null,
                        position, notNull ? "NO" : "YES", null, null, null, null, "NO", generated(f) ? "YES" : "NO"));
            }
        }
        return rows(columns, out);
    }

    @Override
    public ResultSet getPrimaryKeys(String catalog, String schema, String table) throws SQLException {
        String[] columns = {"TABLE_CAT", "TABLE_SCHEM", "TABLE_NAME", "COLUMN_NAME", "KEY_SEQ", "PK_NAME"};
        List<List<Object>> out = new ArrayList<>();
        for (Map<String, Object> t : tables(catalog, schema, exactly(table))) {
            if (t.get("key") != null) {
                out.add(row(null, null, t.get("table"), t.get("key"), 1, t.get("table") + "_key"));
            }
        }
        out.sort(Comparator.comparing(r -> String.valueOf(r.get(3))));
        return rows(columns, out);
    }

    /** A name, as a pattern that matches only it. */
    private static String exactly(String name) {
        return name == null ? null : name.replace("\\", "\\\\").replace("%", "\\%").replace("_", "\\_");
    }

    private static final String[] KEYS = {"PKTABLE_CAT", "PKTABLE_SCHEM", "PKTABLE_NAME", "PKCOLUMN_NAME", "FKTABLE_CAT", "FKTABLE_SCHEM", "FKTABLE_NAME", "FKCOLUMN_NAME", "KEY_SEQ", "UPDATE_RULE", "DELETE_RULE", "FK_NAME", "PK_NAME", "DEFERRABILITY"};

    /**
     * The references from columns of tables matching {@code foreign} to the
     * keys of tables matching {@code parent}. A reference names the table
     * whose key a column holds; ChloeDB does not check that the row exists.
     */
    private ResultSet references(String parentCatalog, String parentSchema, String parent, String foreignCatalog, String foreignSchema, String foreign, boolean byForeign) throws SQLException {
        List<Map<String, Object>> all = tables(null, null, null);
        List<List<Object>> out = new ArrayList<>();
        boolean parents = noSchema(parentCatalog, parentSchema);
        boolean foreigns = noSchema(foreignCatalog, foreignSchema);
        for (Map<String, Object> ft : all) {
            if (!foreigns || (foreign != null && !ft.get("table").toString().equalsIgnoreCase(foreign))) {
                continue;
            }
            for (Map<String, Object> f : fields(ft)) {
                Object ref = f.get("references");
                if (!(ref instanceof String)) {
                    continue;
                }
                for (Map<String, Object> pt : all) {
                    String name = (String) pt.get("table");
                    if (!parents || !name.equalsIgnoreCase((String) ref) || (parent != null && !name.equalsIgnoreCase(parent))) {
                        continue;
                    }
                    out.add(row(null, null, name, pt.get("key"), null, null, ft.get("table"), f.get("name"), 1,
                            importedKeyNoAction, importedKeyNoAction, ft.get("table") + "_" + f.get("name") + "_references", name + "_key", importedKeyNotDeferrable));
                }
            }
        }
        // Imported keys sort by the parent table; exported keys and cross references by the foreign one.
        int by = byForeign ? 6 : 2;
        out.sort(Comparator.comparing((List<Object> r) -> String.valueOf(r.get(by)).toLowerCase(Locale.ROOT)).thenComparing(r -> String.valueOf(r.get(by + 1))));
        return rows(KEYS, out);
    }

    @Override
    public ResultSet getImportedKeys(String catalog, String schema, String table) throws SQLException {
        return references(null, null, null, catalog, schema, table, false);
    }

    @Override
    public ResultSet getExportedKeys(String catalog, String schema, String table) throws SQLException {
        return references(catalog, schema, table, null, null, null, true);
    }

    @Override
    public ResultSet getCrossReference(String parentCatalog, String parentSchema, String parentTable, String foreignCatalog, String foreignSchema, String foreignTable) throws SQLException {
        return references(parentCatalog, parentSchema, parentTable, foreignCatalog, foreignSchema, foreignTable, true);
    }

    /** Each column is indexed by its type; the key is the one unique index. */
    @Override
    public ResultSet getIndexInfo(String catalog, String schema, String table, boolean unique, boolean approximate) throws SQLException {
        String[] columns = {"TABLE_CAT", "TABLE_SCHEM", "TABLE_NAME", "NON_UNIQUE", "INDEX_QUALIFIER", "INDEX_NAME", "TYPE", "ORDINAL_POSITION", "COLUMN_NAME", "ASC_OR_DESC", "CARDINALITY", "PAGES", "FILTER_CONDITION"};
        List<List<Object>> out = new ArrayList<>();
        for (Map<String, Object> t : tables(catalog, schema, exactly(table))) {
            if (t.get("key") != null) {
                out.add(row(null, null, t.get("table"), false, null, t.get("table") + "_key", tableIndexOther, 1, t.get("key"), null, null, null, null));
            }
        }
        return rows(columns, out);
    }

    @Override
    public ResultSet getBestRowIdentifier(String catalog, String schema, String table, int scope, boolean nullable) throws SQLException {
        String[] columns = {"SCOPE", "COLUMN_NAME", "DATA_TYPE", "TYPE_NAME", "COLUMN_SIZE", "BUFFER_LENGTH", "DECIMAL_DIGITS", "PSEUDO_COLUMN"};
        List<List<Object>> out = new ArrayList<>();
        for (Map<String, Object> t : tables(catalog, schema, exactly(table))) {
            for (Map<String, Object> f : fields(t)) {
                if (f.get("name").equals(t.get("key"))) {
                    Object[] type = type(f);
                    out.add(row(bestRowSession, f.get("name"), type[0], type[1], size(f, (Integer) type[0]), null, null, bestRowNotPseudo));
                }
            }
        }
        return rows(columns, out);
    }

    @Override
    public ResultSet getTableTypes() {
        List<List<Object>> out = new ArrayList<>();
        out.add(row("TABLE"));
        return rows(new String[] {"TABLE_TYPE"}, out);
    }

    @Override
    public ResultSet getTypeInfo() {
        String[] columns = {"TYPE_NAME", "DATA_TYPE", "PRECISION", "LITERAL_PREFIX", "LITERAL_SUFFIX", "CREATE_PARAMS", "NULLABLE", "CASE_SENSITIVE", "SEARCHABLE", "UNSIGNED_ATTRIBUTE", "FIXED_PREC_SCALE", "AUTO_INCREMENT", "LOCAL_TYPE_NAME", "MINIMUM_SCALE", "MAXIMUM_SCALE", "SQL_DATA_TYPE", "SQL_DATETIME_SUB", "NUM_PREC_RADIX"};
        Object[][] types = {
            {"BOOL", Types.BOOLEAN, 1, null, null, null, typeSearchable, null},
            {"INT", Types.BIGINT, 19, null, null, null, typePredBasic, 10},
            {"FLOAT", Types.DOUBLE, 17, null, null, null, typePredBasic, 10},
            {"TEXT", Types.VARCHAR, null, "'", "'", null, typeSearchable, null},
            {"KEYWORD", Types.VARCHAR, null, "'", "'", null, typeSearchable, null},
            {"MEDIA", Types.VARCHAR, null, "'", "'", "kind", typePredBasic, null},
            {"JSON", Types.VARCHAR, null, "'", "'", null, typePredBasic, null},
            {"SPARSE", Types.VARCHAR, null, "'", "'", null, typePredNone, null},
            {"EDGES", Types.VARCHAR, null, "'", "'", null, typePredBasic, null},
            {"TIMESTAMP", Types.TIMESTAMP, 26, "'", "'", null, typePredBasic, null},
            {"VECTOR", Types.ARRAY, null, null, null, "dimensions", typePredNone, null},
        };
        List<List<Object>> out = new ArrayList<>();
        for (Object[] t : types) {
            out.add(row(t[0], t[1], t[2], t[3], t[4], t[5], typeNullable, t[1].equals(Types.VARCHAR), t[6], t[7] == null ? null : false, false, false, t[0], 0, 0, null, null, t[7]));
        }
        out.sort(Comparator.comparing(r -> (Long) r.get(1)));
        return rows(columns, out);
    }

    @Override
    public ResultSet getSchemas() {
        return none("TABLE_SCHEM", "TABLE_CATALOG");
    }

    @Override
    public ResultSet getSchemas(String catalog, String schemaPattern) {
        return getSchemas();
    }

    @Override
    public ResultSet getCatalogs() {
        return none("TABLE_CAT");
    }

    @Override
    public ResultSet getClientInfoProperties() {
        return none("NAME", "MAX_LEN", "DEFAULT_VALUE", "DESCRIPTION");
    }

    @Override
    public ResultSet getProcedures(String catalog, String schemaPattern, String procedureNamePattern) {
        return none("PROCEDURE_CAT", "PROCEDURE_SCHEM", "PROCEDURE_NAME", "reserved1", "reserved2", "reserved3", "REMARKS", "PROCEDURE_TYPE", "SPECIFIC_NAME");
    }

    @Override
    public ResultSet getProcedureColumns(String catalog, String schemaPattern, String procedureNamePattern, String columnNamePattern) {
        return none("PROCEDURE_CAT", "PROCEDURE_SCHEM", "PROCEDURE_NAME", "COLUMN_NAME", "COLUMN_TYPE", "DATA_TYPE", "TYPE_NAME", "PRECISION", "LENGTH", "SCALE", "RADIX", "NULLABLE", "REMARKS", "COLUMN_DEF", "SQL_DATA_TYPE", "SQL_DATETIME_SUB", "CHAR_OCTET_LENGTH", "ORDINAL_POSITION", "IS_NULLABLE", "SPECIFIC_NAME");
    }

    @Override
    public ResultSet getFunctions(String catalog, String schemaPattern, String functionNamePattern) {
        return none("FUNCTION_CAT", "FUNCTION_SCHEM", "FUNCTION_NAME", "REMARKS", "FUNCTION_TYPE", "SPECIFIC_NAME");
    }

    @Override
    public ResultSet getFunctionColumns(String catalog, String schemaPattern, String functionNamePattern, String columnNamePattern) {
        return none("FUNCTION_CAT", "FUNCTION_SCHEM", "FUNCTION_NAME", "COLUMN_NAME", "COLUMN_TYPE", "DATA_TYPE", "TYPE_NAME", "PRECISION", "LENGTH", "SCALE", "RADIX", "NULLABLE", "REMARKS", "CHAR_OCTET_LENGTH", "ORDINAL_POSITION", "IS_NULLABLE", "SPECIFIC_NAME");
    }

    @Override
    public ResultSet getUDTs(String catalog, String schemaPattern, String typeNamePattern, int[] types) {
        return none("TYPE_CAT", "TYPE_SCHEM", "TYPE_NAME", "CLASS_NAME", "DATA_TYPE", "REMARKS", "BASE_TYPE");
    }

    @Override
    public ResultSet getSuperTypes(String catalog, String schemaPattern, String typeNamePattern) {
        return none("TYPE_CAT", "TYPE_SCHEM", "TYPE_NAME", "SUPERTYPE_CAT", "SUPERTYPE_SCHEM", "SUPERTYPE_NAME");
    }

    @Override
    public ResultSet getSuperTables(String catalog, String schemaPattern, String tableNamePattern) {
        return none("TABLE_CAT", "TABLE_SCHEM", "TABLE_NAME", "SUPERTABLE_NAME");
    }

    @Override
    public ResultSet getAttributes(String catalog, String schemaPattern, String typeNamePattern, String attributeNamePattern) {
        return none("TYPE_CAT", "TYPE_SCHEM", "TYPE_NAME", "ATTR_NAME", "DATA_TYPE", "ATTR_TYPE_NAME", "ATTR_SIZE", "DECIMAL_DIGITS", "NUM_PREC_RADIX", "NULLABLE", "REMARKS", "ATTR_DEF", "SQL_DATA_TYPE", "SQL_DATETIME_SUB", "CHAR_OCTET_LENGTH", "ORDINAL_POSITION", "IS_NULLABLE", "SCOPE_CATALOG", "SCOPE_SCHEMA", "SCOPE_TABLE", "SOURCE_DATA_TYPE");
    }

    @Override
    public ResultSet getColumnPrivileges(String catalog, String schema, String table, String columnNamePattern) {
        return none("TABLE_CAT", "TABLE_SCHEM", "TABLE_NAME", "COLUMN_NAME", "GRANTOR", "GRANTEE", "PRIVILEGE", "IS_GRANTABLE");
    }

    @Override
    public ResultSet getTablePrivileges(String catalog, String schemaPattern, String tableNamePattern) {
        return none("TABLE_CAT", "TABLE_SCHEM", "TABLE_NAME", "GRANTOR", "GRANTEE", "PRIVILEGE", "IS_GRANTABLE");
    }

    @Override
    public ResultSet getVersionColumns(String catalog, String schema, String table) {
        return none("SCOPE", "COLUMN_NAME", "DATA_TYPE", "TYPE_NAME", "COLUMN_SIZE", "BUFFER_LENGTH", "DECIMAL_DIGITS", "PSEUDO_COLUMN");
    }

    @Override
    public ResultSet getPseudoColumns(String catalog, String schemaPattern, String tableNamePattern, String columnNamePattern) {
        return none("TABLE_CAT", "TABLE_SCHEM", "TABLE_NAME", "COLUMN_NAME", "DATA_TYPE", "COLUMN_SIZE", "DECIMAL_DIGITS", "NUM_PREC_RADIX", "COLUMN_USAGE", "REMARKS", "CHAR_OCTET_LENGTH", "IS_NULLABLE");
    }

    // About the database and its SQL.

    private static String version() {
        Object v = Database.nativeLibrary().get("version");
        return v == null ? "0.0.0" : v.toString();
    }

    private static int part(int i) {
        String[] parts = version().split("[.-]");
        try {
            return i < parts.length ? Integer.parseInt(parts[i]) : 0;
        } catch (NumberFormatException e) {
            return 0;
        }
    }

    @Override
    public Connection getConnection() {
        return conn;
    }

    @Override
    public String getURL() {
        return conn.url();
    }

    @Override
    public String getUserName() {
        return null;
    }

    @Override
    public boolean isReadOnly() throws SQLException {
        return conn.isReadOnly();
    }

    @Override
    public String getDatabaseProductName() {
        return "ChloeDB";
    }

    @Override
    public String getDatabaseProductVersion() {
        return version();
    }

    @Override
    public int getDatabaseMajorVersion() {
        return part(0);
    }

    @Override
    public int getDatabaseMinorVersion() {
        return part(1);
    }

    @Override
    public String getDriverName() {
        return "ChloeDB JDBC";
    }

    @Override
    public String getDriverVersion() {
        return version();
    }

    @Override
    public int getDriverMajorVersion() {
        return part(0);
    }

    @Override
    public int getDriverMinorVersion() {
        return part(1);
    }

    @Override
    public int getJDBCMajorVersion() {
        return 4;
    }

    @Override
    public int getJDBCMinorVersion() {
        return 3;
    }

    @Override
    public String getIdentifierQuoteString() {
        return "\"";
    }

    @Override
    public String getSQLKeywords() {
        return "KEYWORD,MEDIA,SPARSE,EDGES,VECTOR,OPTIONS";
    }

    @Override
    public String getNumericFunctions() {
        return "abs,round,ln,log,exp,sqrt,pow,least,greatest,sigmoid,clamp,dot";
    }

    @Override
    public String getStringFunctions() {
        return "lower,upper,length";
    }

    @Override
    public String getSystemFunctions() {
        return "coalesce";
    }

    @Override
    public String getTimeDateFunctions() {
        return "";
    }

    @Override
    public String getSearchStringEscape() {
        return "\\";
    }

    @Override
    public String getExtraNameCharacters() {
        return "";
    }

    @Override
    public String getSchemaTerm() {
        return "schema";
    }

    @Override
    public String getProcedureTerm() {
        return "procedure";
    }

    @Override
    public String getCatalogTerm() {
        return "catalog";
    }

    @Override
    public String getCatalogSeparator() {
        return ".";
    }

    @Override
    public RowIdLifetime getRowIdLifetime() {
        return RowIdLifetime.ROWID_UNSUPPORTED;
    }

    @Override
    public long getMaxLogicalLobSize() {
        return 0;
    }

    /** None: each statement commits as it runs. */
    @Override
    public int getDefaultTransactionIsolation() {
        return Connection.TRANSACTION_NONE;
    }

    @Override
    public boolean supportsTransactionIsolationLevel(int level) {
        return level == Connection.TRANSACTION_NONE;
    }

    @Override
    public boolean supportsResultSetType(int type) {
        return type == ResultSet.TYPE_FORWARD_ONLY || type == ResultSet.TYPE_SCROLL_INSENSITIVE;
    }

    @Override
    public boolean supportsResultSetConcurrency(int type, int concurrency) {
        return supportsResultSetType(type) && concurrency == ResultSet.CONCUR_READ_ONLY;
    }

    @Override
    public boolean supportsResultSetHoldability(int holdability) {
        return holdability == ResultSet.CLOSE_CURSORS_AT_COMMIT;
    }

    @Override
    public int getResultSetHoldability() {
        return ResultSet.CLOSE_CURSORS_AT_COMMIT;
    }

    @Override
    public int getSQLStateType() {
        return sqlStateSQL;
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

    // The rest are flags: true where ChloeDB's SQL does it (checked against it), else false.

    @Override
    public boolean allProceduresAreCallable() {
        return false;
    }

    @Override
    public boolean allTablesAreSelectable() {
        return true;
    }

    @Override
    public boolean autoCommitFailureClosesAllResultSets() {
        return false;
    }

    @Override
    public boolean dataDefinitionCausesTransactionCommit() {
        return false;
    }

    @Override
    public boolean dataDefinitionIgnoredInTransactions() {
        return false;
    }

    @Override
    public boolean deletesAreDetected(int type) {
        return false;
    }

    @Override
    public boolean doesMaxRowSizeIncludeBlobs() {
        return false;
    }

    @Override
    public boolean generatedKeyAlwaysReturned() {
        return false;
    }

    @Override
    public boolean insertsAreDetected(int type) {
        return false;
    }

    @Override
    public boolean isCatalogAtStart() {
        return false;
    }

    @Override
    public boolean locatorsUpdateCopy() {
        return false;
    }

    @Override
    public boolean nullPlusNonNullIsNull() {
        return true;
    }

    @Override
    public boolean nullsAreSortedAtEnd() {
        return false;
    }

    @Override
    public boolean nullsAreSortedAtStart() {
        return false;
    }

    @Override
    public boolean nullsAreSortedHigh() {
        return false;
    }

    @Override
    public boolean nullsAreSortedLow() {
        return false;
    }

    @Override
    public boolean othersDeletesAreVisible(int type) {
        return false;
    }

    @Override
    public boolean othersInsertsAreVisible(int type) {
        return false;
    }

    @Override
    public boolean othersUpdatesAreVisible(int type) {
        return false;
    }

    @Override
    public boolean ownDeletesAreVisible(int type) {
        return false;
    }

    @Override
    public boolean ownInsertsAreVisible(int type) {
        return false;
    }

    @Override
    public boolean ownUpdatesAreVisible(int type) {
        return false;
    }

    @Override
    public boolean storesLowerCaseIdentifiers() {
        return false;
    }

    @Override
    public boolean storesLowerCaseQuotedIdentifiers() {
        return false;
    }

    @Override
    public boolean storesMixedCaseIdentifiers() {
        return true;
    }

    @Override
    public boolean storesMixedCaseQuotedIdentifiers() {
        return true;
    }

    @Override
    public boolean storesUpperCaseIdentifiers() {
        return false;
    }

    @Override
    public boolean storesUpperCaseQuotedIdentifiers() {
        return false;
    }

    @Override
    public boolean supportsANSI92EntryLevelSQL() {
        return false;
    }

    @Override
    public boolean supportsANSI92FullSQL() {
        return false;
    }

    @Override
    public boolean supportsANSI92IntermediateSQL() {
        return false;
    }

    @Override
    public boolean supportsAlterTableWithAddColumn() {
        return true;
    }

    @Override
    public boolean supportsAlterTableWithDropColumn() {
        return true;
    }

    @Override
    public boolean supportsBatchUpdates() {
        return true;
    }

    @Override
    public boolean supportsCatalogsInDataManipulation() {
        return false;
    }

    @Override
    public boolean supportsCatalogsInIndexDefinitions() {
        return false;
    }

    @Override
    public boolean supportsCatalogsInPrivilegeDefinitions() {
        return false;
    }

    @Override
    public boolean supportsCatalogsInProcedureCalls() {
        return false;
    }

    @Override
    public boolean supportsCatalogsInTableDefinitions() {
        return false;
    }

    @Override
    public boolean supportsColumnAliasing() {
        return true;
    }

    @Override
    public boolean supportsConvert() {
        return false;
    }

    @Override
    public boolean supportsConvert(int fromType, int toType) {
        return false;
    }

    @Override
    public boolean supportsCoreSQLGrammar() {
        return false;
    }

    @Override
    public boolean supportsCorrelatedSubqueries() {
        return false;
    }

    @Override
    public boolean supportsDataDefinitionAndDataManipulationTransactions() {
        return false;
    }

    @Override
    public boolean supportsDataManipulationTransactionsOnly() {
        return false;
    }

    @Override
    public boolean supportsDifferentTableCorrelationNames() {
        return false;
    }

    @Override
    public boolean supportsExpressionsInOrderBy() {
        return true;
    }

    @Override
    public boolean supportsExtendedSQLGrammar() {
        return false;
    }

    @Override
    public boolean supportsFullOuterJoins() {
        return false;
    }

    @Override
    public boolean supportsGetGeneratedKeys() {
        return false;
    }

    @Override
    public boolean supportsGroupBy() {
        return true;
    }

    @Override
    public boolean supportsGroupByBeyondSelect() {
        return true;
    }

    @Override
    public boolean supportsGroupByUnrelated() {
        return true;
    }

    @Override
    public boolean supportsIntegrityEnhancementFacility() {
        return false;
    }

    @Override
    public boolean supportsLikeEscapeClause() {
        return true;
    }

    @Override
    public boolean supportsLimitedOuterJoins() {
        return true;
    }

    @Override
    public boolean supportsMinimumSQLGrammar() {
        return true;
    }

    @Override
    public boolean supportsMixedCaseIdentifiers() {
        return false;
    }

    @Override
    public boolean supportsMixedCaseQuotedIdentifiers() {
        return false;
    }

    @Override
    public boolean supportsMultipleOpenResults() {
        return false;
    }

    @Override
    public boolean supportsMultipleResultSets() {
        return false;
    }

    @Override
    public boolean supportsMultipleTransactions() {
        return false;
    }

    @Override
    public boolean supportsNamedParameters() {
        return false;
    }

    @Override
    public boolean supportsNonNullableColumns() {
        return true;
    }

    @Override
    public boolean supportsOpenCursorsAcrossCommit() {
        return false;
    }

    @Override
    public boolean supportsOpenCursorsAcrossRollback() {
        return false;
    }

    @Override
    public boolean supportsOpenStatementsAcrossCommit() {
        return false;
    }

    @Override
    public boolean supportsOpenStatementsAcrossRollback() {
        return false;
    }

    @Override
    public boolean supportsOrderByUnrelated() {
        return true;
    }

    @Override
    public boolean supportsOuterJoins() {
        return true;
    }

    @Override
    public boolean supportsPositionedDelete() {
        return false;
    }

    @Override
    public boolean supportsPositionedUpdate() {
        return false;
    }

    @Override
    public boolean supportsRefCursors() {
        return false;
    }

    @Override
    public boolean supportsSavepoints() {
        return false;
    }

    @Override
    public boolean supportsSchemasInDataManipulation() {
        return false;
    }

    @Override
    public boolean supportsSchemasInIndexDefinitions() {
        return false;
    }

    @Override
    public boolean supportsSchemasInPrivilegeDefinitions() {
        return false;
    }

    @Override
    public boolean supportsSchemasInProcedureCalls() {
        return false;
    }

    @Override
    public boolean supportsSchemasInTableDefinitions() {
        return false;
    }

    @Override
    public boolean supportsSelectForUpdate() {
        return false;
    }

    @Override
    public boolean supportsSharding() {
        return false;
    }

    @Override
    public boolean supportsStatementPooling() {
        return false;
    }

    @Override
    public boolean supportsStoredFunctionsUsingCallSyntax() {
        return false;
    }

    @Override
    public boolean supportsStoredProcedures() {
        return false;
    }

    @Override
    public boolean supportsSubqueriesInComparisons() {
        return false;
    }

    @Override
    public boolean supportsSubqueriesInExists() {
        return false;
    }

    @Override
    public boolean supportsSubqueriesInIns() {
        return true;
    }

    @Override
    public boolean supportsSubqueriesInQuantifieds() {
        return false;
    }

    @Override
    public boolean supportsTableCorrelationNames() {
        return true;
    }

    @Override
    public boolean supportsTransactions() {
        return false;
    }

    @Override
    public boolean supportsUnion() {
        return true;
    }

    @Override
    public boolean supportsUnionAll() {
        return true;
    }

    @Override
    public boolean updatesAreDetected(int type) {
        return false;
    }

    @Override
    public boolean usesLocalFilePerTable() {
        return false;
    }

    @Override
    public boolean usesLocalFiles() {
        return true;
    }

    @Override
    public int getMaxBinaryLiteralLength() {
        return 0;
    }

    @Override
    public int getMaxCatalogNameLength() {
        return 0;
    }

    @Override
    public int getMaxCharLiteralLength() {
        return 0;
    }

    @Override
    public int getMaxColumnNameLength() {
        return 0;
    }

    @Override
    public int getMaxColumnsInGroupBy() {
        return 0;
    }

    @Override
    public int getMaxColumnsInIndex() {
        return 0;
    }

    @Override
    public int getMaxColumnsInOrderBy() {
        return 0;
    }

    @Override
    public int getMaxColumnsInSelect() {
        return 0;
    }

    @Override
    public int getMaxColumnsInTable() {
        return 0;
    }

    @Override
    public int getMaxConnections() {
        return 0;
    }

    @Override
    public int getMaxCursorNameLength() {
        return 0;
    }

    @Override
    public int getMaxIndexLength() {
        return 0;
    }

    @Override
    public int getMaxProcedureNameLength() {
        return 0;
    }

    @Override
    public int getMaxRowSize() {
        return 0;
    }

    @Override
    public int getMaxSchemaNameLength() {
        return 0;
    }

    @Override
    public int getMaxStatementLength() {
        return 0;
    }

    @Override
    public int getMaxStatements() {
        return 0;
    }

    @Override
    public int getMaxTableNameLength() {
        return 0;
    }

    @Override
    public int getMaxTablesInSelect() {
        return 0;
    }

    @Override
    public int getMaxUserNameLength() {
        return 0;
    }
}
