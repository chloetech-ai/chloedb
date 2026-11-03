/**
 * Strata from Java: the database in this process, read and written by
 * Strata's native library over JNI, with no server, as sqlite-jdbc opens a
 * SQLite file.
 *
 * <p>{@link ai.chloetech.strata.Database} creates or opens a database file
 * and runs SQL over it; {@link ai.chloetech.strata.Table} writes and reads
 * one table's rows by key and searches it; a query answers a
 * {@link ai.chloetech.strata.Result}. Errors from the database are
 * {@link ai.chloetech.strata.StrataException}s.
 */
package ai.chloetech.strata;
