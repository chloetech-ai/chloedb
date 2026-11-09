package ai.chloetech.strata.jdbc;

import java.io.InputStream;
import java.io.Reader;
import java.math.BigDecimal;
import java.sql.Array;
import java.sql.Blob;
import java.sql.Clob;
import java.sql.Date;
import java.sql.NClob;
import java.sql.Ref;
import java.sql.ResultSet;
import java.sql.RowId;
import java.sql.SQLException;
import java.sql.SQLFeatureNotSupportedException;
import java.sql.SQLType;
import java.sql.SQLXML;
import java.sql.Time;
import java.sql.Timestamp;

/**
 * A result set's writing half, which ChloeDB's result sets do not have:
 * they are read-only ({@code CONCUR_READ_ONLY}), and rows are written with
 * INSERT, UPDATE and DELETE.
 */
abstract class ReadOnlyResultSet implements ResultSet {
    static SQLFeatureNotSupportedException readOnly() {
        return Sql.unsupported("updating a result set: it is read-only; write rows with INSERT, UPDATE or DELETE");
    }

    @Override
    public void cancelRowUpdates() throws SQLException {
        throw readOnly();
    }

    @Override
    public void deleteRow() throws SQLException {
        throw readOnly();
    }

    @Override
    public void insertRow() throws SQLException {
        throw readOnly();
    }

    @Override
    public void moveToCurrentRow() throws SQLException {
        throw readOnly();
    }

    @Override
    public void moveToInsertRow() throws SQLException {
        throw readOnly();
    }

    @Override
    public void refreshRow() throws SQLException {
        throw readOnly();
    }

    @Override
    public boolean rowDeleted() {
        return false;
    }

    @Override
    public boolean rowInserted() {
        return false;
    }

    @Override
    public boolean rowUpdated() {
        return false;
    }

    @Override
    public void updateArray(String label, Array x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateArray(int i, Array x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateAsciiStream(String label, InputStream x, int n2) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateAsciiStream(String label, InputStream x, long n2) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateAsciiStream(String label, InputStream x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateAsciiStream(int i, InputStream x, int n2) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateAsciiStream(int i, InputStream x, long n2) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateAsciiStream(int i, InputStream x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateBigDecimal(String label, BigDecimal x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateBigDecimal(int i, BigDecimal x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateBinaryStream(String label, InputStream x, int n2) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateBinaryStream(String label, InputStream x, long n2) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateBinaryStream(String label, InputStream x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateBinaryStream(int i, InputStream x, int n2) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateBinaryStream(int i, InputStream x, long n2) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateBinaryStream(int i, InputStream x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateBlob(String label, InputStream x, long n2) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateBlob(String label, InputStream x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateBlob(String label, Blob x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateBlob(int i, InputStream x, long n2) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateBlob(int i, InputStream x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateBlob(int i, Blob x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateBoolean(String label, boolean x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateBoolean(int i, boolean x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateByte(String label, byte x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateByte(int i, byte x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateBytes(String label, byte[] x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateBytes(int i, byte[] x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateCharacterStream(String label, Reader x, int n2) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateCharacterStream(String label, Reader x, long n2) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateCharacterStream(String label, Reader x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateCharacterStream(int i, Reader x, int n2) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateCharacterStream(int i, Reader x, long n2) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateCharacterStream(int i, Reader x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateClob(String label, Reader x, long n2) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateClob(String label, Reader x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateClob(String label, Clob x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateClob(int i, Reader x, long n2) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateClob(int i, Reader x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateClob(int i, Clob x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateDate(String label, Date x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateDate(int i, Date x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateDouble(String label, double x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateDouble(int i, double x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateFloat(String label, float x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateFloat(int i, float x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateInt(String label, int x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateInt(int i, int x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateLong(String label, long x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateLong(int i, long x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateNCharacterStream(String label, Reader x, long n2) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateNCharacterStream(String label, Reader x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateNCharacterStream(int i, Reader x, long n2) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateNCharacterStream(int i, Reader x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateNClob(String label, Reader x, long n2) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateNClob(String label, Reader x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateNClob(String label, NClob x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateNClob(int i, Reader x, long n2) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateNClob(int i, Reader x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateNClob(int i, NClob x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateNString(String label, String x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateNString(int i, String x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateNull(String label) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateNull(int i) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateObject(String label, Object x, int n2) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateObject(String label, Object x, SQLType type, int n3) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateObject(String label, Object x, SQLType type) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateObject(String label, Object x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateObject(int i, Object x, int n2) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateObject(int i, Object x, SQLType type, int n3) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateObject(int i, Object x, SQLType type) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateObject(int i, Object x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateRef(String label, Ref x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateRef(int i, Ref x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateRow() throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateRowId(String label, RowId x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateRowId(int i, RowId x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateSQLXML(String label, SQLXML x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateSQLXML(int i, SQLXML x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateShort(String label, short x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateShort(int i, short x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateString(String label, String x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateString(int i, String x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateTime(String label, Time x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateTime(int i, Time x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateTimestamp(String label, Timestamp x) throws SQLException {
        throw readOnly();
    }

    @Override
    public void updateTimestamp(int i, Timestamp x) throws SQLException {
        throw readOnly();
    }
}
