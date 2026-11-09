package ai.chloetech.strata;

/**
 * An error from the database: a bad schema, query, value or file, or a
 * database another process has open.
 *
 * <p>Arguments of the wrong kind throw {@link IllegalArgumentException}, and
 * a call on a closed {@link Database} {@link IllegalStateException}.
 */
public class StrataException extends RuntimeException {
    private static final long serialVersionUID = 1L;

    public StrataException(String message) {
        super(message);
    }
}
