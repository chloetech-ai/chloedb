//! BGP updates out of MRT files (RFC 6396), as RIPE RIS archives them every
//! five minutes per collector (`updates.YYYYMMDD.HHMM.gz`): the BGP4MP
//! messages each peer sent the collector. Only what the app keeps is read:
//! the time, the peer, the prefixes announced and withdrawn (IPv4 in the
//! message's own fields, IPv6 in MP_REACH and MP_UNREACH), the AS path and
//! the communities. Other records (state changes, table dumps) are skipped.

use std::io::Read;

/// One prefix announced or withdrawn by one peer, as the rows are.
#[derive(Debug, Clone, PartialEq)]
pub struct Update {
    /// Seconds since 1970, and the microseconds of BGP4MP_ET records.
    pub ts: u32,
    pub us: u32,
    pub peer_as: u32,
    pub peer_ip: String,
    /// true: announced; false: withdrawn.
    pub announce: bool,
    pub prefix: Prefix,
    /// The AS path, sets flattened in order (empty for a withdrawal).
    pub path: Vec<u32>,
    /// The last AS of the path when it is one AS (0 when it ends in a set
    /// of several, or for a withdrawal).
    pub origin: u32,
    /// Standard communities, `asn:value` each.
    pub communities: Vec<(u16, u16)>,
}

/// An IPv4 or IPv6 prefix, its address as 128 bits (IPv4 in the low 32).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Prefix {
    pub v6: bool,
    pub addr: u128,
    pub len: u8,
}

impl Prefix {
    pub fn bits(&self) -> u8 {
        if self.v6 {
            128
        } else {
            32
        }
    }

    /// The address with the bits past the length cleared.
    pub fn masked(mut self) -> Prefix {
        let w = self.bits() as u32;
        let l = self.len.min(self.bits()) as u32;
        if l < w {
            let keep = if l == 0 { 0 } else { (!0u128) << (w - l) };
            let all = if w == 128 { !0u128 } else { (1u128 << w) - 1 };
            self.addr &= keep & all;
        }
        self
    }

    /// The first and last address, as numbers ChloeDB keeps in INT columns:
    /// an IPv4 address itself, an IPv6 address's first 64 bits (shifted by
    /// 2^63 so that order is kept in a signed number).
    pub fn range(&self) -> (i64, i64) {
        let w = self.bits() as u32;
        let l = self.len.min(self.bits()) as u32;
        let span: u128 = if l == 0 { if w == 128 { !0u128 } else { (1u128 << w) - 1 } } else if l >= w { 0 } else { (1u128 << (w - l)) - 1 };
        let lo = self.addr;
        let hi = lo | span;
        if self.v6 {
            let f = |a: u128| ((a >> 64) as u64 ^ (1u64 << 63)) as i64;
            (f(lo), f(hi))
        } else {
            (lo as i64, hi as i64)
        }
    }

    /// Whether `other` is this prefix or inside it.
    pub fn covers(&self, other: &Prefix) -> bool {
        if self.v6 != other.v6 || other.len < self.len {
            return false;
        }
        let w = self.bits() as u32;
        let l = self.len as u32;
        if l == 0 {
            return true;
        }
        let shift = w - l;
        (self.addr >> shift) == (other.addr >> shift)
    }

    pub fn parse(s: &str) -> Option<Prefix> {
        let (a, l) = s.trim().split_once('/')?;
        let len: u8 = l.parse().ok()?;
        if a.contains(':') {
            let ip: std::net::Ipv6Addr = a.parse().ok()?;
            (len <= 128).then(|| Prefix { v6: true, addr: u128::from(ip), len }.masked())
        } else {
            let ip: std::net::Ipv4Addr = a.parse().ok()?;
            (len <= 32).then(|| Prefix { v6: false, addr: u32::from(ip) as u128, len }.masked())
        }
    }
}

impl std::fmt::Display for Prefix {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.v6 {
            write!(f, "{}/{}", std::net::Ipv6Addr::from(self.addr), self.len)
        } else {
            write!(f, "{}/{}", std::net::Ipv4Addr::from(self.addr as u32), self.len)
        }
    }
}

struct Cur<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Cur<'a> {
    fn new(b: &'a [u8]) -> Self {
        Cur { b, i: 0 }
    }
    fn left(&self) -> usize {
        self.b.len().saturating_sub(self.i)
    }
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.left() < n {
            return None;
        }
        let s = &self.b[self.i..self.i + n];
        self.i += n;
        Some(s)
    }
    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|s| s[0])
    }
    fn u16(&mut self) -> Option<u16> {
        self.take(2).map(|s| u16::from_be_bytes([s[0], s[1]]))
    }
    fn u32(&mut self) -> Option<u32> {
        self.take(4).map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
    }
}

fn ip(b: &[u8]) -> String {
    match b.len() {
        4 => std::net::Ipv4Addr::new(b[0], b[1], b[2], b[3]).to_string(),
        16 => {
            let mut a = [0u8; 16];
            a.copy_from_slice(b);
            std::net::Ipv6Addr::from(a).to_string()
        }
        _ => String::new(),
    }
}

/// Prefixes in BGP's encoding (a length in bits, then as many bytes as it
/// needs), until the slice ends.
fn prefixes(b: &[u8], v6: bool, out: &mut Vec<Prefix>) {
    let mut c = Cur::new(b);
    while c.left() > 0 {
        let Some(len) = c.u8() else { break };
        let max = if v6 { 128 } else { 32 };
        if len > max {
            break;
        }
        let n = (len as usize).div_ceil(8);
        let Some(bytes) = c.take(n) else { break };
        let mut a = [0u8; 16];
        let w = if v6 { 16 } else { 4 };
        a[..n].copy_from_slice(bytes);
        let mut addr = 0u128;
        for x in &a[..w] {
            addr = (addr << 8) | *x as u128;
        }
        out.push(Prefix { v6, addr, len }.masked());
    }
}

/// Reads every record of an MRT stream, calling `f` with each update.
/// Returns the number of records read; a record cut short ends the read.
pub fn read<R: Read>(mut r: R, mut f: impl FnMut(Update)) -> std::io::Result<u64> {
    let mut head = [0u8; 12];
    let mut body = Vec::new();
    let mut n = 0u64;
    loop {
        match r.read_exact(&mut head) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(n),
            Err(e) => return Err(e),
        }
        let ts = u32::from_be_bytes([head[0], head[1], head[2], head[3]]);
        let ty = u16::from_be_bytes([head[4], head[5]]);
        let sub = u16::from_be_bytes([head[6], head[7]]);
        let len = u32::from_be_bytes([head[8], head[9], head[10], head[11]]) as usize;
        body.resize(len, 0);
        match r.read_exact(&mut body) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(n),
            Err(e) => return Err(e),
        }
        n += 1;
        let (us, rest) = match ty {
            16 => (0, &body[..]),
            17 if body.len() >= 4 => (u32::from_be_bytes([body[0], body[1], body[2], body[3]]), &body[4..]),
            _ => continue,
        };
        // MESSAGE (1), MESSAGE_AS4 (4), and their _LOCAL forms (6, 7).
        let as4 = match sub {
            1 | 6 => false,
            4 | 7 => true,
            _ => continue,
        };
        message(ts, us, as4, rest, &mut f);
    }
}

fn message(ts: u32, us: u32, as4: bool, b: &[u8], f: &mut impl FnMut(Update)) -> Option<()> {
    let mut c = Cur::new(b);
    let peer_as = if as4 { c.u32()? } else { c.u16()? as u32 };
    let _local_as = if as4 { c.u32()? } else { c.u16()? as u32 };
    let _ifindex = c.u16()?;
    let afi = c.u16()?;
    let w = if afi == 2 { 16 } else { 4 };
    let peer_ip = ip(c.take(w)?);
    let _local_ip = c.take(w)?;
    // The BGP message: marker, length, type.
    let _marker = c.take(16)?;
    let blen = c.u16()? as usize;
    let btype = c.u8()?;
    if btype != 2 || blen < 19 {
        return None;
    }
    let msg = c.take((blen - 19).min(c.left()))?;
    let mut m = Cur::new(msg);
    let wlen = m.u16()? as usize;
    let mut withdrawn = Vec::new();
    prefixes(m.take(wlen)?, false, &mut withdrawn);
    let alen = m.u16()? as usize;
    let attrs = m.take(alen)?;
    let mut announced = Vec::new();
    prefixes(&msg[m.i..], false, &mut announced);

    let mut path: Vec<u32> = Vec::new();
    let mut as4_path: Option<Vec<u32>> = None;
    let mut last_set: Option<Vec<u32>> = None;
    let mut communities = Vec::new();
    let mut a = Cur::new(attrs);
    while a.left() > 0 {
        let flags = a.u8()?;
        let code = a.u8()?;
        let len = if flags & 0x10 != 0 { a.u16()? as usize } else { a.u8()? as usize };
        let v = a.take(len)?;
        match code {
            2 | 17 => {
                // AS_PATH, or AS4_PATH beside a 2-byte one.
                let size = if code == 17 || as4 { 4 } else { 2 };
                let mut p = Vec::new();
                let mut set = None;
                let mut s = Cur::new(v);
                while s.left() >= 2 {
                    let st = s.u8()?;
                    let cnt = s.u8()? as usize;
                    let mut seg = Vec::with_capacity(cnt);
                    for _ in 0..cnt {
                        seg.push(if size == 4 { s.u32()? } else { s.u16()? as u32 });
                    }
                    // Confederation segments are not part of the path seen from outside.
                    if st == 3 || st == 4 {
                        continue;
                    }
                    set = (st == 1).then(|| seg.clone());
                    p.extend(seg);
                }
                if code == 2 {
                    path = p;
                    last_set = set;
                } else {
                    as4_path = Some(p);
                }
            }
            8 => {
                let mut s = Cur::new(v);
                while s.left() >= 4 {
                    communities.push((s.u16()?, s.u16()?));
                }
            }
            14 => {
                let mut s = Cur::new(v);
                let afi = s.u16()?;
                let safi = s.u8()?;
                let nhl = s.u8()? as usize;
                s.take(nhl)?;
                s.u8()?;
                if safi == 1 && (afi == 1 || afi == 2) {
                    prefixes(&v[s.i..], afi == 2, &mut announced);
                }
            }
            15 => {
                let mut s = Cur::new(v);
                let afi = s.u16()?;
                let safi = s.u8()?;
                if safi == 1 && (afi == 1 || afi == 2) {
                    prefixes(&v[s.i..], afi == 2, &mut withdrawn);
                }
            }
            _ => {}
        }
    }
    // A 2-byte path with AS_TRANS (23456) is made whole by AS4_PATH (RFC 6793).
    if let Some(p4) = as4_path {
        if !as4 && p4.len() <= path.len() {
            let keep = path.len() - p4.len();
            path.truncate(keep);
            path.extend(p4);
        }
    }
    let origin = match &last_set {
        Some(s) if s.len() == 1 => s[0],
        Some(_) => 0,
        None => path.last().copied().unwrap_or(0),
    };
    for p in withdrawn {
        f(Update { ts, us, peer_as, peer_ip: peer_ip.clone(), announce: false, prefix: p, path: Vec::new(), origin: 0, communities: Vec::new() });
    }
    for p in announced {
        f(Update { ts, us, peer_as, peer_ip: peer_ip.clone(), announce: true, prefix: p, path: path.clone(), origin, communities: communities.clone() });
    }
    Some(())
}

/// Reads a gzipped MRT file (RIS's `updates.*.gz`).
pub fn read_gz(path: &std::path::Path, f: impl FnMut(Update)) -> std::io::Result<u64> {
    let file = std::fs::File::open(path)?;
    let gz = flate2::read::MultiGzDecoder::new(std::io::BufReader::with_capacity(1 << 20, file));
    read(std::io::BufReader::with_capacity(1 << 20, gz), f)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefixes_parse_and_cover() {
        let a = Prefix::parse("84.255.184.0/21").unwrap();
        let b = Prefix::parse("84.255.184.0/24").unwrap();
        assert!(a.covers(&b) && !b.covers(&a));
        assert_eq!(b.to_string(), "84.255.184.0/24");
        assert_eq!(Prefix::parse("10.1.2.3/8").unwrap().to_string(), "10.0.0.0/8");
        let (lo, hi) = b.range();
        assert_eq!(hi - lo, 255);
        let v6 = Prefix::parse("2a0e:2c0::/29").unwrap();
        assert_eq!(v6.to_string(), "2a0e:2c0::/29");
        let (lo, hi) = v6.range();
        assert!(lo < hi);
        assert!(Prefix::parse("2a0e::/16").unwrap().covers(&v6));
    }

    fn rec(ts: u32, body: &[u8]) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend(ts.to_be_bytes());
        v.extend(16u16.to_be_bytes());
        v.extend(4u16.to_be_bytes());
        v.extend((body.len() as u32).to_be_bytes());
        v.extend(body);
        v
    }

    #[test]
    fn reads_an_as4_update() {
        // peer AS 6453, local 12654, if 0, AFI 1, peer 1.2.3.4, local 5.6.7.8
        let mut b = Vec::new();
        b.extend(6453u32.to_be_bytes());
        b.extend(12654u32.to_be_bytes());
        b.extend(0u16.to_be_bytes());
        b.extend(1u16.to_be_bytes());
        b.extend([1, 2, 3, 4, 5, 6, 7, 8]);
        let mut msg = Vec::new();
        // withdrawn: 10.0.0.0/8
        msg.extend(2u16.to_be_bytes());
        msg.extend([8, 10]);
        // attrs: AS_PATH seq [6453, 5416], communities 6453:50
        let mut attrs = vec![0x40, 2, 10, 2, 2];
        attrs.extend(6453u32.to_be_bytes());
        attrs.extend(5416u32.to_be_bytes());
        attrs.extend([0xC0, 8, 4]);
        attrs.extend(6453u16.to_be_bytes());
        attrs.extend(50u16.to_be_bytes());
        msg.extend((attrs.len() as u16).to_be_bytes());
        msg.extend(&attrs);
        // NLRI: 84.255.184.0/24
        msg.extend([24, 84, 255, 184]);
        b.extend([0xff; 16]);
        b.extend(((19 + msg.len()) as u16).to_be_bytes());
        b.push(2);
        b.extend(&msg);
        let file = rec(1794876043, &b);
        let mut got = Vec::new();
        assert_eq!(read(&file[..], |u| got.push(u)).unwrap(), 1);
        assert_eq!(got.len(), 2);
        assert!(!got[0].announce);
        assert_eq!(got[0].prefix.to_string(), "10.0.0.0/8");
        assert!(got[1].announce);
        assert_eq!(got[1].prefix.to_string(), "84.255.184.0/24");
        assert_eq!(got[1].path, vec![6453, 5416]);
        assert_eq!(got[1].origin, 5416);
        assert_eq!(got[1].communities, vec![(6453, 50)]);
        assert_eq!(got[1].peer_ip, "1.2.3.4");
    }
}
