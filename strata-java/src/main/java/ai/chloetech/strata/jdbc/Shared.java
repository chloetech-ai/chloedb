package ai.chloetech.strata.jdbc;

import ai.chloetech.strata.Database;
import java.nio.file.Path;
import java.sql.SQLException;
import java.util.HashMap;
import java.util.Map;
import java.util.function.Supplier;

/**
 * The databases JDBC connections have open, one per file: a process may
 * open a database once, and a pool opens many connections to it.
 */
final class Shared {
    private Shared() {}

    private static final Map<Path, Entry> OPEN = new HashMap<>();

    /** A database and the connections holding it. */
    static final class Entry {
        final Path path;
        final Database db;
        private int holders = 1;

        Entry(Path path, Database db) {
            this.path = path;
            this.db = db;
        }

        /** Lets go of it: the last holder closes it. */
        void release() {
            synchronized (OPEN) {
                if (--holders == 0) {
                    OPEN.remove(path);
                    db.close();
                }
            }
        }
    }

    static Entry open(Path path, Supplier<Database> opener) throws SQLException {
        synchronized (OPEN) {
            Entry e = OPEN.get(path);
            if (e != null) {
                e.holders++;
                return e;
            }
            try {
                e = new Entry(path, opener.get());
            } catch (RuntimeException x) {
                throw Sql.error(x);
            }
            OPEN.put(path, e);
            return e;
        }
    }
}
