package ai.chloetech.chloedb.jdbc;

import java.sql.BatchUpdateException;
import java.sql.SQLException;
import java.util.Arrays;

/** A batch that stopped at a failed statement. */
final class Batch {
    private Batch() {}

    /** The error of statement {@code at}, with the counts of the ones before it, which stay written. */
    static BatchUpdateException failed(long[] counts, int at, SQLException cause) {
        long[] done = Arrays.copyOf(counts, at);
        BatchUpdateException e = new BatchUpdateException(
                "statement " + (at + 1) + " of the batch failed (the " + at + " before it are written): " + cause.getMessage(),
                cause.getSQLState(), cause.getErrorCode(), done, cause);
        return e;
    }
}
