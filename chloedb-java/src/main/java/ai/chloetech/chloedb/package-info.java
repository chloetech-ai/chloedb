/**
 * ChloeDB from Java: the database in this process, read and written by
 * ChloeDB's native library over JNI, with no server, as sqlite-jdbc opens a
 * SQLite file.
 *
 * <p>{@link ai.chloetech.chloedb.Database} creates or opens a database file
 * and runs SQL over it; {@link ai.chloetech.chloedb.Table} writes and reads
 * one table's rows by key and searches it; a query answers a
 * {@link ai.chloetech.chloedb.Result}. Errors from the database are
 * {@link ai.chloetech.chloedb.ChloeDBException}s.
 */
package ai.chloetech.chloedb;
