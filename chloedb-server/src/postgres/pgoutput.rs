//! The messages of a logical replication stream: the walsender's frames
//! (XLogData, keepalives) and, inside XLogData, those of the `pgoutput`
//! plugin, protocol version 1 (Postgres 10 and later), with values as text.

use super::wire::{Error, Reader};

/// A position in the write-ahead log.
pub type Lsn = u64;

/// `lsn` as Postgres writes it, `16/B374D848`.
pub fn lsn_text(lsn: Lsn) -> String {
    format!("{:X}/{:X}", lsn >> 32, lsn & 0xffff_ffff)
}

/// An LSN as Postgres writes it.
pub fn parse_lsn(s: &str) -> Option<Lsn> {
    let (hi, lo) = s.trim().split_once('/')?;
    Some((u64::from_str_radix(hi, 16).ok()? << 32) | u64::from_str_radix(lo, 16).ok()?)
}

/// Microseconds from Postgres's epoch (2000-01-01) to Unix's.
const PG_EPOCH_MICROS: i64 = 946_684_800_000_000;

/// Now, in microseconds since Postgres's epoch.
pub fn pg_now() -> i64 {
    chloedb::timestamp::now() - PG_EPOCH_MICROS
}

/// A frame of the replication stream (a CopyData's bytes).
#[derive(Debug, PartialEq)]
pub enum Frame<'a> {
    /// WAL data: where it starts, the end of the server's WAL, and the
    /// plugin's message.
    XLogData { start: Lsn, end: Lsn, data: &'a [u8] },
    /// The end of the server's WAL, and whether it asks for an answer now.
    Keepalive { end: Lsn, reply: bool },
}

pub fn frame(b: &[u8]) -> Result<Frame<'_>, Error> {
    let mut r = Reader::new(b);
    match r.u8()? {
        b'w' => {
            let start = r.u64()?;
            let end = r.u64()?;
            let _time = r.u64()?;
            Ok(Frame::XLogData { start, end, data: r.rest() })
        }
        b'k' => {
            let end = r.u64()?;
            let _time = r.u64()?;
            Ok(Frame::Keepalive { end, reply: r.u8()? == 1 })
        }
        other => Err(Error { message: format!("a replication frame of kind {:?}", other as char), code: None, io: true }),
    }
}

/// A Standby Status Update: what this client wrote, flushed and applied.
pub fn status(written: Lsn, flushed: Lsn, applied: Lsn, reply: bool) -> Vec<u8> {
    let mut m = vec![b'r'];
    m.extend_from_slice(&written.to_be_bytes());
    m.extend_from_slice(&flushed.to_be_bytes());
    m.extend_from_slice(&applied.to_be_bytes());
    m.extend_from_slice(&pg_now().to_be_bytes());
    m.push(reply as u8);
    m
}

/// A column of a relation, as the stream describes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Column {
    /// Part of the replica identity.
    pub key: bool,
    pub name: String,
    pub type_oid: u32,
    pub typmod: i32,
}

/// A table, as the stream describes it before the first change of it a
/// session sends, and again after it changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Relation {
    pub oid: u32,
    /// Its schema (`""` for `pg_catalog`).
    pub namespace: String,
    pub name: String,
    /// `REPLICA IDENTITY`: `d`efault, `n`othing, `f`ull, or `i`ndex.
    pub identity: u8,
    pub columns: Vec<Column>,
}

/// One column's value in a row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Datum {
    Null,
    /// A large value the update did not change, which the stream leaves
    /// out (it is in TOAST storage).
    Unchanged,
    Text(String),
}

pub type Tuple = Vec<Datum>;

/// The old row an update or delete sends: its replica identity's
/// columns (`Key`), or with `REPLICA IDENTITY FULL` every column (`Full`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Old {
    Key(Tuple),
    Full(Tuple),
}

impl Old {
    pub fn tuple(&self) -> &Tuple {
        match self {
            Old::Key(t) | Old::Full(t) => t,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Message {
    /// A transaction begins: where its commit record is.
    Begin { final_lsn: Lsn, xid: u32 },
    /// It committed: its commit record, and where it ends (the position a
    /// stream started again from no longer sends it).
    Commit { commit_lsn: Lsn, end_lsn: Lsn },
    Relation(Relation),
    /// A type's name (for one not built in).
    Type { oid: u32, namespace: String, name: String },
    Insert { rel: u32, new: Tuple },
    Update { rel: u32, old: Option<Old>, new: Tuple },
    Delete { rel: u32, old: Old },
    Truncate { rels: Vec<u32> },
    /// Origin, logical decoding messages, and kinds of later protocols.
    Other(u8),
}

fn tuple(r: &mut Reader<'_>) -> Result<Tuple, Error> {
    let n = r.i16()? as usize;
    let mut t = Vec::with_capacity(n);
    for _ in 0..n {
        t.push(match r.u8()? {
            b'n' => Datum::Null,
            b'u' => Datum::Unchanged,
            b't' => {
                let len = r.i32()?;
                let b = r.bytes(usize::try_from(len).map_err(|_| Error { message: "a value of negative length".into(), code: None, io: true })?)?;
                Datum::Text(String::from_utf8(b.to_vec()).map_err(|_| Error { message: "a value that is not UTF-8: the database's encoding is not UTF8".into(), code: None, io: true })?)
            }
            other => return Err(Error { message: format!("a value of kind {:?} (binary values are not asked for)", other as char), code: None, io: true }),
        });
    }
    Ok(t)
}

/// Decodes a pgoutput message.
pub fn message(b: &[u8]) -> Result<Message, Error> {
    let mut r = Reader::new(b);
    Ok(match r.u8()? {
        b'B' => {
            let final_lsn = r.u64()?;
            let _ts = r.u64()?;
            Message::Begin { final_lsn, xid: r.u32()? }
        }
        b'C' => {
            let _flags = r.u8()?;
            let commit_lsn = r.u64()?;
            Message::Commit { commit_lsn, end_lsn: r.u64()? }
        }
        b'R' => {
            let oid = r.u32()?;
            let namespace = r.cstr()?;
            let name = r.cstr()?;
            let identity = r.u8()?;
            let n = r.i16()? as usize;
            let mut columns = Vec::with_capacity(n);
            for _ in 0..n {
                let key = r.u8()? & 1 == 1;
                columns.push(Column { key, name: r.cstr()?, type_oid: r.u32()?, typmod: r.i32()? });
            }
            Message::Relation(Relation { oid, namespace, name, identity, columns })
        }
        b'Y' => Message::Type { oid: r.u32()?, namespace: r.cstr()?, name: r.cstr()? },
        b'I' => {
            let rel = r.u32()?;
            r.u8()?; // 'N'
            Message::Insert { rel, new: tuple(&mut r)? }
        }
        b'U' => {
            let rel = r.u32()?;
            let mut old = None;
            let mut kind = r.u8()?;
            if kind == b'K' || kind == b'O' {
                let t = tuple(&mut r)?;
                old = Some(if kind == b'K' { Old::Key(t) } else { Old::Full(t) });
                kind = r.u8()?;
            }
            if kind != b'N' {
                return Err(Error { message: format!("an update's new row marked {:?}", kind as char), code: None, io: true });
            }
            Message::Update { rel, old, new: tuple(&mut r)? }
        }
        b'D' => {
            let rel = r.u32()?;
            let kind = r.u8()?;
            let t = tuple(&mut r)?;
            Message::Delete { rel, old: if kind == b'O' { Old::Full(t) } else { Old::Key(t) } }
        }
        b'T' => {
            let n = r.i32()?.max(0) as usize;
            let _options = r.u8()?;
            let mut rels = Vec::with_capacity(n);
            for _ in 0..n {
                rels.push(r.u32()?);
            }
            Message::Truncate { rels }
        }
        other => Message::Other(other),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put_str(v: &mut Vec<u8>, s: &str) {
        v.extend_from_slice(s.as_bytes());
        v.push(0);
    }

    fn put_text(v: &mut Vec<u8>, s: &str) {
        v.push(b't');
        v.extend_from_slice(&(s.len() as i32).to_be_bytes());
        v.extend_from_slice(s.as_bytes());
    }

    #[test]
    fn lsns_read_and_print_as_postgres_writes_them() {
        assert_eq!(parse_lsn("16/B374D848"), Some(0x16_B374_D848));
        assert_eq!(lsn_text(0x16_B374_D848), "16/B374D848");
        assert_eq!(lsn_text(0), "0/0");
        assert_eq!(parse_lsn("nope"), None);
    }

    #[test]
    fn messages_decode() {
        let mut m = vec![b'R'];
        m.extend_from_slice(&16384u32.to_be_bytes());
        put_str(&mut m, "public");
        put_str(&mut m, "items");
        m.push(b'd');
        m.extend_from_slice(&2i16.to_be_bytes());
        m.push(1);
        put_str(&mut m, "id");
        m.extend_from_slice(&23u32.to_be_bytes());
        m.extend_from_slice(&(-1i32).to_be_bytes());
        m.push(0);
        put_str(&mut m, "body");
        m.extend_from_slice(&25u32.to_be_bytes());
        m.extend_from_slice(&(-1i32).to_be_bytes());
        let Message::Relation(r) = message(&m).unwrap() else { panic!() };
        assert_eq!((r.oid, r.name.as_str(), r.identity, r.columns.len(), r.columns[0].key, r.columns[1].key), (16384, "items", b'd', 2, true, false));

        let mut u = vec![b'U'];
        u.extend_from_slice(&16384u32.to_be_bytes());
        u.push(b'K');
        u.extend_from_slice(&2i16.to_be_bytes());
        put_text(&mut u, "1");
        u.push(b'n');
        u.push(b'N');
        u.extend_from_slice(&2i16.to_be_bytes());
        put_text(&mut u, "2");
        u.push(b'u');
        assert_eq!(
            message(&u).unwrap(),
            Message::Update { rel: 16384, old: Some(Old::Key(vec![Datum::Text("1".into()), Datum::Null])), new: vec![Datum::Text("2".into()), Datum::Unchanged] }
        );

        let mut c = vec![b'C', 0];
        c.extend_from_slice(&10u64.to_be_bytes());
        c.extend_from_slice(&20u64.to_be_bytes());
        c.extend_from_slice(&0u64.to_be_bytes());
        assert_eq!(message(&c).unwrap(), Message::Commit { commit_lsn: 10, end_lsn: 20 });

        let mut k = vec![b'k'];
        k.extend_from_slice(&99u64.to_be_bytes());
        k.extend_from_slice(&0u64.to_be_bytes());
        k.push(1);
        assert_eq!(frame(&k).unwrap(), Frame::Keepalive { end: 99, reply: true });
        assert!(message(&[b'I', 0, 0]).is_err());
    }
}
