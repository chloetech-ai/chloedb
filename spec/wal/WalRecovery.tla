---------------------------- MODULE WalRecovery ----------------------------
(***************************************************************************)
(* The storage engine's commit path and crash recovery, one file plus its  *)
(* -wal sidecar (Layout::SingleFile, the default), as                      *)
(* chloedb/src/storage/ does each step:                                    *)
(*                                                                         *)
(*  * group commit (chloedb/src/db/writes.rs, Queue::submit, take,         *)
(*    Db::apply_group, apply_alone): writers queue their writes, the first *)
(*    leads, takes every write waiting, applies them in one transaction    *)
(*    (Database::begin, WriteTxn::{upsert,delete,insert}) and answers each *)
(*    once it committed; a group that fails runs each write alone; the     *)
(*    lead is handed to the oldest write still waiting;                    *)
(*  * a commit (Database::commit_logged): the transaction's records are    *)
(*    appended to the log with one write and made durable with one         *)
(*    fdatasync (Wal::append), then applied (State::apply): a delete of an *)
(*    on-disk row is pending until a version holds it;                     *)
(*  * a flush (flush_inner): the memtable as a segment and the pending     *)
(*    deletes into delete sets, one version (VersionWriter::write_version: *)
(*    the log's unsynced appends synced, the pages written and synced, the *)
(*    other meta slot written and synced), published, then the log emptied *)
(*    (Wal::reset, a new file renamed over it);                            *)
(*  * a background flush (background_flush_inner): under the writer lock  *)
(*    the memtable is sealed and the log renamed to <wal>.<txn> (a new one *)
(*    takes commits); outside it the segment is built; under it again it   *)
(*    is installed with what commits since deleted of the sealed rows      *)
(*    (SealedMem::since), and the sealed log removed (remove_sealed);      *)
(*  * a merge (claim_merge, merge_group, install_merge): its inputs are    *)
(*    read in a snapshot taken without the writer lock, its image written  *)
(*    without it into space reserved under it (write_reserved, reserve),   *)
(*    and installed under it with the inputs' rows deleted since the       *)
(*    snapshot as the merged segment's delete set;                         *)
(*  * a crash at any step: what was not synced is lost (a log append past  *)
(*    the durable prefix survives or not), and the meta slot being written *)
(*    is left old, new, or (Tear = "garbage") failing its checksum;        *)
(*  * recovery (Database::load, fallback_decision, fallback_keeps_commits, *)
(*    assemble, read_sealed_wals, Wal::open): the valid slot with the      *)
(*    newest txn, a decision whether opening may fall back past a damaged  *)
(*    slot, the sealed logs and the log replayed past the version's        *)
(*    wal_txn.                                                             *)
(*                                                                         *)
(* Rows are <<key, commit id, place in its transaction>>: an upsert is a   *)
(* delete of the key's live row (when there is one) and an insert. Rows    *)
(* carry no other value, in-place updates (Op::Set) are left out: they     *)
(* are carried as deletes are (install_merge's `updated`). The pages of a  *)
(* version are modelled as durable once written: the code syncs them       *)
(* before the slot that names them, and reuses no page the current         *)
(* version or the one before it reaches (FreeList::release at             *)
(* meta.txn - 1), which this model takes as given. A directory operation   *)
(* (a rename) is atomic and ordered before later ones, as on a journaled   *)
(* file system. Read-only processes, snapshots' pins, the log record       *)
(* (WriterLock::vouch, for a deleted log) and I/O errors other than a     *)
(* crash are left out. One merge runs at a time.                           *)
(*                                                                         *)
(* Sync is DbOptions::sync (TRUE by default). SyncFirst = FALSE drops      *)
(* write_version's first wal.sync_appends(); MergeCarries = FALSE drops    *)
(* install_merge's deletes since the snapshot. Both are what the code does *)
(* not do, to show what each step is for.                                  *)
(*                                                                         *)
(* MaxRot > 0 lets a written slot fail its checksum later (Rot), which     *)
(* fallback_decision is there to catch. FixIntent is not in the code: a    *)
(* fix for a refusal TLC found (CITornSlot). write_version records, and    *)
(* syncs, the txn and slot of the version it writes before the slot, and   *)
(* marks the record done once the slot is synced, before Wal::reset or     *)
(* remove_sealed; an open marks it done when it opens that version or a   *)
(* newer one. A damaged slot the record names as in flight, with a txn    *)
(* past the version opened, is a write a crash cut, not damage. And any   *)
(* record shows slot 1 written: all zeros there is damage.                *)
(*                                                                         *)
(* ZeroSlot1 lets Rot zero slot 1 beside the version of creation, which   *)
(* damaged_slots leaves out and fallback_decision opens past when the log *)
(* is empty ("An empty log leaves it unproven"), as the code says.        *)
(***************************************************************************)
EXTENDS Integers, Sequences, FiniteSets, TLC

CONSTANTS
    Writers,      \* threads writing through Db::write_ops, e.g. {w1, w2}
    Keys,         \* primary keys, e.g. {k1}
    Menu,         \* Menu[w]: the writes w may make, [kind, key] with kind
                  \* "put" (upsert), "del" (delete) or "ins" (insert, which
                  \* fails if the key is live)
    Writes,       \* Writes[w]: how many writes w makes
    Flushers,     \* which flushes run: SUBSET {"fg", "bg"}
    MaxVersions,  \* versions written (flushes, background installs, merges)
    MaxMerges,    \* merges claimed
    MaxCrashes,
    Sync,         \* DbOptions::sync
    SyncFirst,    \* write_version syncs the log's appends first (the code)
    MergeCarries, \* install_merge carries deletes since its snapshot (the code)
    Tear,         \* "sector": a slot write at a crash is left old or new;
                  \* "garbage": it may also fail its checksum, its fields
                  \* naming the old version, the new one, or none readable
    MaxRot,       \* slots damaged after they were written (bit rot), at most
    ZeroSlot1,    \* Rot may zero slot 1 beside the version of creation
    FixIntent     \* the fix below: the writer records the version it writes
                  \* before its slot, and when the slot is synced

ASSUME Tear \in {"sector", "garbage"}
ASSUME Flushers \subseteq {"fg", "bg"}

SegIds == 1..(MaxVersions + MaxMerges)
NoW == "none"
Empty == [s \in SegIds |-> {}]
Max(S) == CHOOSE x \in S : \A y \in S : y <= x
Min(S) == CHOOSE x \in S : \A y \in S : x <= y
Range(f) == {f[i] : i \in DOMAIN f}

(* A version: its txn, the txn of the last logged transaction it holds    *)
(* (Manifest::wal_txn), its segments and their delete sets, and the next  *)
(* segment id (Manifest::next_segment_id).                                *)
NoV == [txn |-> 0, wal |-> 0, segs |-> {}, del |-> Empty, next |-> 0]
V1  == [txn |-> 1, wal |-> 0, segs |-> {}, del |-> Empty, next |-> 1]   \* Database::create

(* A meta slot's bytes: "zeros", a "valid" Meta, or "torn" (fails its     *)
(* checksum; v is the version its fields name, NoV when none is readable).*)
Slot(k, v) == [kind |-> k, v |-> v]
Zeros == Slot("zeros", NoV)

Del(k) == [op |-> "del", key |-> k, row |-> <<>>]
Ins(r) == [op |-> "ins", key |-> r[1], row |-> r]

VARIABLES
    \* On disk.
    slots,      \* slots[0], slots[1]
    inflight,   \* the slot being written, not synced yet: [on, slot, v, old]
    wal,        \* the log: <<[txn, id, ops]>>, appended in order
    walDur,     \* how many of them are durable
    sealed,     \* sealed logs <wal>.<t>, oldest first: <<[t, recs]>>
    segRows,    \* each segment's rows (its pages, synced before any slot names them)
    fileGrown,  \* the file holds pages past what slot 0's version says
    \* In memory (lost at a crash).
    cur,        \* the version published (State.manifest)
    slotCur,    \* its slot (State.slot)
    pend,       \* deletes of on-disk rows not in a version yet (State.pending)
    mem,        \* the memtable's rows, deleted ones too
    memDel,     \* the memtable's deleted rows
    stTxn,      \* State.txn
    lock,       \* the writer lock: "free" | "commit" | "flush" | "bg" | "merge"
    job,        \* the version being written under it, and what follows
    bg,         \* the background flush: [st, t, all, live]
    mg,         \* the merge: [st, ins, snap, m]
    \* Group commit.
    wst,        \* each writer: "idle" | "queued" | "take" | "lead"
    wop,        \* its write in flight
    wleft,      \* writes it has still to make
    queue,      \* Waiting.writes
    leading,    \* Waiting.leading
    leader,     \* the writer leading a group (NoW: none)
    group,      \* its group
    todo,       \* the transactions still to run: <<group>>, or each alone
    ctx,        \* the transaction under way: [ph, ws, ops, id, txn]
    ans,        \* answers kept for the group: -2 none, -1 failed,
                \* 0 committed with nothing to log, n > 0 committed as id n
    \* History.
    hist,       \* the commits, in order: <<[id, txn, live]>>, live the rows
                \* live after it
    nextId,
    absorbed,   \* commits a reopen emptied the log of, which the version holds
    told,       \* commits a writer was told of
    earlyTold,  \* a writer was told of a commit not yet durable
    badPrefix,  \* a recovery held no prefix of the commits
    lostAcked,  \* a recovery lacked a commit a writer was told of
    refused,    \* a recovery refused to open (fallback_decision)
    crashes, versions, merges, nextSeg,
    \* Damage after the fact, and the fix.
    intent,     \* FixIntent: the version being written [txn, slot, done], synced
    silent,     \* a recovery that did not refuse lost a commit (or held no prefix)
    rots

disk  == <<slots, inflight, wal, walDur, sealed, segRows, fileGrown>>
memv  == <<cur, slotCur, pend, mem, memDel, stTxn, lock, job, bg, mg>>
wrv   == <<wst, wop, wleft, queue, leading, leader, group, todo, ctx, ans>>
ghost == <<hist, nextId, absorbed, told, earlyTold, badPrefix, lostAcked, refused,
           crashes, versions, merges, nextSeg>>
rare  == <<intent, silent, rots>>
vars  == <<disk, memv, wrv, ghost, rare>>

NoFlight == [on |-> FALSE, slot |-> 0, v |-> NoV, old |-> Zeros]
NoJob    == [kind |-> "none", v |-> NoV]
BgIdle   == [st |-> "idle", t |-> 0, all |-> {}, live |-> {}]
MgIdle   == [st |-> "idle", ins |-> {}, snap |-> Empty, m |-> 0]
NoCtx    == [ph |-> "none", ws |-> <<>>, ops |-> <<>>, id |-> 0, txn |-> 0]

-----------------------------------------------------------------------------
(* The database as State::apply sees it.                                   *)

DB == [segs |-> cur.segs, del |-> cur.del, pend |-> pend, mem |-> mem, memDel |-> memDel]
LiveIn(db, s) == segRows[s] \ (db.del[s] \cup db.pend[s])
Live(db) == (db.mem \ db.memDel) \cup UNION {LiveIn(db, s) : s \in db.segs}
KeyOf(r) == r[1]

(* State::apply. A delete finds the key's live row (State::find_live: the *)
(* memtable's first, then the segments'): one in the memtable is marked   *)
(* deleted, one on disk becomes a pending delete, and none is a no-op. An *)
(* insert goes into the memtable.                                         *)
ApplyOp(db, op) ==
    IF op.op = "ins" THEN [db EXCEPT !.mem = @ \cup {op.row}]
    ELSE LET m == {r \in db.mem \ db.memDel : KeyOf(r) = op.key}
             d == UNION {{<<s, r>> : r \in {x \in LiveIn(db, s) : KeyOf(x) = op.key}} : s \in db.segs}
         IN IF m # {} THEN [db EXCEPT !.memDel = @ \cup {CHOOSE r \in m : TRUE}]
            ELSE IF d # {} THEN LET sr == CHOOSE x \in d : TRUE
                                IN [db EXCEPT !.pend[sr[1]] = @ \cup {sr[2]}]
            ELSE db

RECURSIVE ApplyOps(_, _)
ApplyOps(db, ops) == IF ops = <<>> THEN db ELSE ApplyOps(ApplyOp(db, Head(ops)), Tail(ops))

(* Replay (assemble): each record past the version's wal_txn, in order.   *)
RECURSIVE Replay(_, _, _)
Replay(db, recs, w) ==
    IF recs = <<>> THEN db
    ELSE Replay(IF Head(recs).txn > w THEN ApplyOps(db, Head(recs).ops) ELSE db, Tail(recs), w)

RECURSIVE Cat(_)
Cat(ls) == IF ls = <<>> THEN <<>> ELSE Head(ls).recs \o Cat(Tail(ls))

(* The operations a transaction of the writes of `ws` logs (WriteOp::apply *)
(* and WriteTxn's insert_as, upsert_as, delete_in), from the rows live    *)
(* before it: whether it may commit, and its operations.                  *)
RECURSIVE TxnOps(_, _, _, _, _)
TxnOps(ws, i, keys, acc, id) ==
    IF i > Len(ws) THEN [ok |-> TRUE, ops |-> acc]
    ELSE LET o == wop[ws[i]]
             r == <<o.key, id, i>>
         IN CASE o.kind = "put" ->
                   TxnOps(ws, i + 1, keys \cup {o.key},
                          acc \o (IF o.key \in keys THEN <<Del(o.key)>> ELSE <<>>) \o <<Ins(r)>>, id)
              [] o.kind = "del" ->
                   IF o.key \in keys THEN TxnOps(ws, i + 1, keys \ {o.key}, Append(acc, Del(o.key)), id)
                   ELSE TxnOps(ws, i + 1, keys, acc, id)
              [] o.kind = "ins" ->
                   IF o.key \in keys THEN [ok |-> FALSE, ops |-> <<>>]
                   ELSE TxnOps(ws, i + 1, keys \cup {o.key}, Append(acc, Ins(r)), id)

StateAt(n) == IF n = 0 THEN {} ELSE hist[n].live

(* The version a slot set opens at: the valid slot with the newest txn.   *)
ValidSlots(sl) == {s \in {0, 1} : sl[s].kind = "valid"}
NewestSlot(sl) == CHOOSE s \in ValidSlots(sl) : \A x \in ValidSlots(sl) : sl[x].v.txn <= sl[s].v.txn

(* Whether commit `id` is durable: its record is in the log's durable     *)
(* prefix or a sealed log, or the version on disk holds it.               *)
Durable(id) ==
    \/ \E k \in 1..walDur : wal[k].id = id
    \/ \E l \in Range(sealed) : \E r \in Range(l.recs) : r.id = id
    \/ \E i \in DOMAIN hist : hist[i].id = id /\ hist[i].txn <= slots[NewestSlot(slots)].v.wal

-----------------------------------------------------------------------------
(* fallback_decision and fallback_keeps_commits.                           *)

(* fallback_keeps_commits(older_txn, wal_txn, newer_wal, logged); nw is   *)
(* [k |-> "known", w] or [k |-> "unknown"].                              *)
Keeps(older, w, nw, logged) ==
    IF nw.k = "known" /\ (nw.w <= w \/ \E t \in logged : t > w /\ t <= nw.w) THEN TRUE
    ELSE LET above == {t \in logged : t > w}
             first == above # {} /\ (LET c == Min(above) IN c = w + 1 \/ (older = w + 1 /\ c = w + 2))
         IN IF ~first \/ nw.k = "known" THEN first
            ELSE LET after == {t \in logged : t > older}
                 IN after = (older + 1)..(older + Cardinality(after))

(* Whether opening at version V (in slot vs) of slot set sl refuses, the  *)
(* logs holding the txns `logged` (wal::logged_txns: every sealed log and *)
(* the open one). pager::damaged_slots: a torn slot, or zeros where a     *)
(* version was written (slot 0, or slot 1 once slot 0 is past txn 1).     *)
Refuses(sl, V, logged, grown) ==
    LET \* FixIntent: a record of a version past creation shows slot 1
        \* written too (the first version after creation goes there)
        slot1Written == \/ sl[0].kind = "valid" /\ sl[0].v.txn > 1
                        \/ FixIntent /\ intent.txn > 0
        \* FixIntent: the slot of a version whose write the record shows
        \* begun and not synced, newer than V, is no damage: a crash cut it
        inFlight(s) == FixIntent /\ ~intent.done /\ intent.slot = s /\ intent.txn > V.txn
        damaged(s) == ~inFlight(s) /\
                      (sl[s].kind = "torn" \/ (sl[s].kind = "zeros" /\ (s = 0 \/ slot1Written)))
        \* the newer version's wal_txn where its manifest reads, slot 0 first
        step(n, s) ==
            IF ~damaged(s) THEN n
            ELSE IF sl[s].v.txn > 0
                 THEN IF sl[s].v.txn < V.txn THEN n ELSE [k |-> "known", w |-> sl[s].v.wal]
                 ELSE IF n.k = "known" THEN n ELSE [k |-> "unknown"]
        n1 == step(step([k |-> "none"], 0), 1)
        \* slot 1 of zeros beside the version of creation, the file longer
        \* than it: a log past the transactions after it shows a version gone
        n2 == IF n1.k = "none" /\ V.txn = 1 /\ grown /\ sl[1].kind = "zeros"
                 /\ logged # {} /\ ~Keeps(1, V.wal, [k |-> "unknown"], logged)
              THEN [k |-> "unknown"] ELSE n1
    IN n2.k # "none" /\ ~Keeps(V.txn, V.wal, n2, logged)

-----------------------------------------------------------------------------
Init ==
    /\ slots = [s \in {0, 1} |-> IF s = 0 THEN Slot("valid", V1) ELSE Zeros]
    /\ inflight = NoFlight
    /\ wal = <<>> /\ walDur = 0 /\ sealed = <<>>
    /\ segRows = [s \in SegIds |-> {}]
    /\ fileGrown = FALSE
    /\ cur = V1 /\ slotCur = 0 /\ pend = Empty /\ mem = {} /\ memDel = {} /\ stTxn = 1
    /\ lock = "free" /\ job = NoJob /\ bg = BgIdle /\ mg = MgIdle
    /\ wst = [w \in Writers |-> "idle"]
    /\ wop = [w \in Writers |-> CHOOSE o \in Menu[w] : TRUE]
    /\ wleft = Writes
    /\ queue = <<>> /\ leading = FALSE /\ leader = NoW /\ group = <<>> /\ todo = <<>>
    /\ ctx = NoCtx
    /\ ans = [w \in Writers |-> -2]
    /\ hist = <<>> /\ nextId = 1 /\ absorbed = {} /\ told = {}
    /\ earlyTold = FALSE /\ badPrefix = FALSE /\ lostAcked = FALSE /\ refused = FALSE
    /\ crashes = 0 /\ versions = 0 /\ merges = 0 /\ nextSeg = 1
    /\ intent = [txn |-> 0, slot |-> 0, done |-> TRUE] /\ silent = FALSE /\ rots = 0

-----------------------------------------------------------------------------
(* Group commit (Queue::submit).                                           *)

(* A thread hands in its write; it leads if no thread does. *)
Submit(w) ==
    /\ wst[w] = "idle" /\ wleft[w] > 0
    /\ \E o \in Menu[w] :
         /\ wop' = [wop EXCEPT ![w] = o]
         /\ wleft' = [wleft EXCEPT ![w] = @ - 1]
         /\ queue' = Append(queue, w)
         /\ wst' = [wst EXCEPT ![w] = IF leading THEN "queued" ELSE "take"]
         /\ leading' = TRUE
    /\ UNCHANGED <<disk, memv, leader, group, todo, ctx, ans, ghost, rare>>

(* take: the writes waiting, its own first; all fit one group. *)
Take(w) ==
    /\ wst[w] = "take"
    /\ leader' = w /\ group' = queue /\ todo' = <<queue>> /\ queue' = <<>>
    /\ wst' = [wst EXCEPT ![w] = "lead"]
    /\ UNCHANGED <<disk, memv, wop, wleft, leading, ctx, ans, ghost, rare>>

Answer(ws, a) == [w \in Writers |-> IF \E i \in DOMAIN ws : ws[i] = w THEN a ELSE ans[w]]

(* Database::begin and the writes of the next transaction: a group, or a  *)
(* write alone. A group that cannot commit is dropped (it wrote nothing)  *)
(* and each of its writes runs alone (apply_group); one alone fails.      *)
(* A transaction with nothing to log commits at once (commit_logged).     *)
Begin ==
    /\ leader # NoW /\ todo # <<>> /\ ctx.ph = "none" /\ lock = "free"
    /\ LET ws == Head(todo)
           t  == TxnOps(ws, 1, {KeyOf(r) : r \in Live(DB)}, <<>>, nextId)
       IN IF ~t.ok THEN
             /\ todo' = IF Len(ws) > 1 THEN [i \in 1..Len(ws) |-> <<ws[i]>>] \o Tail(todo) ELSE Tail(todo)
             /\ ans' = IF Len(ws) > 1 THEN ans ELSE Answer(ws, -1)
             /\ UNCHANGED <<lock, ctx>>
          ELSE IF t.ops = <<>> THEN
             /\ todo' = Tail(todo) /\ ans' = Answer(ws, 0)
             /\ UNCHANGED <<lock, ctx>>
          ELSE
             /\ lock' = "commit"
             /\ ctx' = [ph |-> "begun", ws |-> ws, ops |-> t.ops, id |-> nextId, txn |-> stTxn + 1]
             /\ UNCHANGED <<todo, ans>>
    /\ UNCHANGED <<disk, cur, slotCur, pend, mem, memDel, stTxn, job, bg, mg,
                   wst, wop, wleft, queue, leading, leader, group, ghost, rare>>

(* Wal::append: the records written with one write, not yet synced. *)
Append_ ==
    /\ ctx.ph = "begun"
    /\ wal' = Append(wal, [txn |-> ctx.txn, id |-> ctx.id, ops |-> ctx.ops])
    /\ hist' = Append(hist, [id |-> ctx.id, txn |-> ctx.txn, live |-> Live(ApplyOps(DB, ctx.ops))])
    /\ nextId' = nextId + 1
    /\ ctx' = [ctx EXCEPT !.ph = "appended"]
    /\ UNCHANGED <<slots, inflight, walDur, sealed, segRows, fileGrown, memv,
                   wst, wop, wleft, queue, leading, leader, group, todo, ans,
                   absorbed, told, earlyTold, badPrefix, lostAcked, refused,
                   crashes, versions, merges, nextSeg, rare>>

(* ... and its fdatasync, with sync. *)
SyncAppend ==
    /\ ctx.ph = "appended" /\ Sync
    /\ walDur' = Len(wal)
    /\ ctx' = [ctx EXCEPT !.ph = "synced"]
    /\ UNCHANGED <<slots, inflight, wal, sealed, segRows, fileGrown, memv,
                   wst, wop, wleft, queue, leading, leader, group, todo, ans, ghost, rare>>

(* State::apply of each operation; the writer lock is released. *)
Apply ==
    /\ ctx.ph = IF Sync THEN "synced" ELSE "appended"
    /\ LET db == ApplyOps(DB, ctx.ops)
       IN /\ pend' = db.pend /\ mem' = db.mem /\ memDel' = db.memDel
    /\ stTxn' = ctx.txn
    /\ lock' = "free"
    /\ ans' = Answer(ctx.ws, ctx.id)
    /\ todo' = Tail(todo)
    /\ ctx' = NoCtx
    /\ UNCHANGED <<disk, cur, slotCur, job, bg, mg, wst, wop, wleft, queue, leading,
                   leader, group, ghost, rare>>

(* Each write of the group gets its answer, and the lead goes to the      *)
(* oldest write waiting (Leading::drop).                                  *)
Finish ==
    /\ leader # NoW /\ todo = <<>> /\ ctx.ph = "none"
    /\ LET gs == Range(group)
           ok == {ans[w] : w \in {x \in gs : ans[x] > 0}}
           idle == [w \in Writers |-> IF w \in gs THEN "idle" ELSE wst[w]]
       IN /\ told' = told \cup ok
          /\ earlyTold' = (earlyTold \/ \E id \in ok : ~Durable(id))
          /\ IF queue # <<>>
             THEN wst' = [idle EXCEPT ![Head(queue)] = "take"] /\ UNCHANGED leading
             ELSE wst' = idle /\ leading' = FALSE
    /\ leader' = NoW /\ group' = <<>>
    /\ ans' = [w \in Writers |-> -2]
    /\ UNCHANGED <<disk, memv, wop, wleft, queue, todo, ctx,
                   hist, nextId, absorbed, badPrefix, lostAcked, refused,
                   crashes, versions, merges, nextSeg, rare>>

-----------------------------------------------------------------------------
(* Versions (VersionWriter::write_version), under the writer lock.         *)

(* The log's unsynced appends synced (SyncFirst), the pages written and   *)
(* synced, the other slot written: not durable until VersionSync.         *)
WriteVersion(U, j, who) ==
    /\ walDur' = IF SyncFirst THEN Len(wal) ELSE walDur
    /\ inflight' = [on |-> TRUE, slot |-> 1 - slotCur, v |-> U, old |-> slots[1 - slotCur]]
    /\ job' = j
    /\ lock' = who
    /\ fileGrown' = TRUE
    /\ versions' = versions + 1
    /\ intent' = IF FixIntent THEN [txn |-> U.txn, slot |-> 1 - slotCur, done |-> FALSE] ELSE intent
    /\ UNCHANGED <<silent, rots>>

Pending == \E s \in SegIds : pend[s] # {}

(* flush_inner: the memtable's live rows as a segment, the pending        *)
(* deletes into the delete sets, wal_txn the last transaction.            *)
Flush ==
    /\ "fg" \in Flushers /\ lock = "free" /\ versions < MaxVersions
    /\ mem # {} \/ Pending
    /\ LET live == mem \ memDel
           id == nextSeg
           U == [txn |-> stTxn + 1, wal |-> stTxn,
                 segs |-> cur.segs \cup (IF live # {} THEN {id} ELSE {}),
                 del |-> [s \in SegIds |-> IF s \in cur.segs THEN cur.del[s] \cup pend[s] ELSE {}],
                 next |-> IF live # {} THEN id + 1 ELSE id]
       IN /\ live # {} => id \in SegIds
          /\ segRows' = IF live # {} THEN [segRows EXCEPT ![id] = live] ELSE segRows
          /\ nextSeg' = IF live # {} THEN nextSeg + 1 ELSE nextSeg
          /\ WriteVersion(U, [kind |-> "flush", v |-> U], "flush")
    /\ UNCHANGED <<slots, wal, sealed, cur, slotCur, pend, mem, memDel, stTxn, bg, mg, wrv,
                   hist, nextId, absorbed, told, earlyTold, badPrefix, lostAcked, refused,
                   crashes, merges>>

(* background_flush_inner, under the lock: seal the memtable, and the log *)
(* if it holds commits: synced, renamed to <wal>.<txn>, a new one started.*)
BgSeal ==
    /\ "bg" \in Flushers /\ lock = "free" /\ bg.st = "idle" /\ versions < MaxVersions
    /\ mem # {} \/ Pending
    /\ bg' = [st |-> "sealed", t |-> stTxn, all |-> mem, live |-> mem \ memDel]
    /\ IF wal # <<>>
       THEN /\ sealed' = Append(sealed, [t |-> stTxn, recs |-> wal])
            /\ wal' = <<>> /\ walDur' = 0
       ELSE UNCHANGED <<sealed, wal, walDur>>
    /\ UNCHANGED <<slots, inflight, segRows, fileGrown, cur, slotCur, pend, mem, memDel,
                   stTxn, lock, job, mg, wrv, ghost, rare>>

(* ... and, the segment built meanwhile without the lock, under it again: *)
(* nothing if a flush of everything came first; else the sealed live rows *)
(* as a segment whose delete set is those deleted since the seal, the     *)
(* pending deletes into the delete sets, wal_txn the seal's txn.          *)
BgInstall ==
    /\ bg.st = "sealed" /\ lock = "free"
    /\ IF cur.wal >= bg.t
       THEN /\ bg' = BgIdle
            /\ UNCHANGED <<slots, inflight, wal, walDur, sealed, segRows, fileGrown, cur,
                           slotCur, pend, mem, memDel, stTxn, lock, job, mg, wrv, ghost, rare>>
       ELSE /\ versions < MaxVersions
            /\ LET id == nextSeg
                   new == bg.live # {}
                   U == [txn |-> stTxn + 1, wal |-> bg.t,
                         segs |-> cur.segs \cup (IF new THEN {id} ELSE {}),
                         del |-> [s \in SegIds |-> IF s \in cur.segs THEN cur.del[s] \cup pend[s]
                                                   ELSE IF new /\ s = id THEN bg.live \cap memDel
                                                   ELSE {}],
                         next |-> IF new THEN id + 1 ELSE id]
               IN /\ new => id \in SegIds
                  /\ segRows' = IF new THEN [segRows EXCEPT ![id] = bg.live] ELSE segRows
                  /\ nextSeg' = IF new THEN nextSeg + 1 ELSE nextSeg
                  /\ WriteVersion(U, [kind |-> "bg", v |-> U], "bg")
            /\ UNCHANGED <<slots, wal, sealed, cur, slotCur, pend, mem, memDel, stTxn, bg, mg,
                           wrv, hist, nextId, absorbed, told, earlyTold, badPrefix,
                           lostAcked, refused, crashes, merges>>

(* claim_merge: inputs no other merge holds, read in a snapshot taken     *)
(* without the writer lock (its live rows: pending deletes count).        *)
MergeClaim ==
    /\ mg.st = "idle" /\ merges < MaxMerges
    /\ \E S \in SUBSET cur.segs :
         /\ S # {}
         /\ mg' = [st |-> "claimed", ins |-> S, m |-> 0,
                   snap |-> [s \in SegIds |-> IF s \in S THEN LiveIn(DB, s) ELSE {}]]
    /\ merges' = merges + 1
    /\ UNCHANGED <<disk, cur, slotCur, pend, mem, memDel, stTxn, lock, job, bg, wrv,
                   hist, nextId, absorbed, told, earlyTold, badPrefix, lostAcked, refused,
                   crashes, versions, nextSeg, rare>>

(* merge_group, write_reserved: the merged image, written and synced into *)
(* pages reserved under the lock (reserve), no version naming them yet;   *)
(* none when every input row was deleted.                                 *)
MergeBuild ==
    /\ mg.st = "claimed" /\ lock = "free"
    /\ LET rows == UNION {mg.snap[s] : s \in mg.ins}
       IN IF rows = {}
          THEN /\ mg' = [mg EXCEPT !.st = "built", !.m = 0]
               /\ UNCHANGED <<segRows, nextSeg>>
          ELSE /\ nextSeg \in SegIds
               /\ segRows' = [segRows EXCEPT ![nextSeg] = rows]
               /\ mg' = [mg EXCEPT !.st = "built", !.m = nextSeg]
               /\ nextSeg' = nextSeg + 1
    /\ UNCHANGED <<slots, inflight, wal, walDur, sealed, fileGrown, cur, slotCur, pend, mem,
                   memDel, stTxn, lock, job, bg, wrv, hist, nextId, absorbed, told,
                   earlyTold, badPrefix, lostAcked, refused, crashes, versions, merges, rare>>

(* install_merge, under the lock: the inputs replaced by the merged      *)
(* segment, whose delete set is its rows deleted since the snapshot       *)
(* (stored or pending: State::is_deleted); wal_txn as it was.             *)
MergeInstall ==
    /\ mg.st = "built" /\ lock = "free"
    /\ IF ~(mg.ins \subseteq cur.segs)
       THEN /\ mg' = MgIdle
            /\ UNCHANGED <<slots, inflight, wal, walDur, sealed, segRows, fileGrown, cur,
                           slotCur, pend, mem, memDel, stTxn, lock, job, bg, wrv, ghost, rare>>
       ELSE /\ versions < MaxVersions
            /\ LET m == mg.m
                   deleted == IF MergeCarries /\ m # 0
                              THEN {r \in segRows[m] : \E s \in mg.ins : r \in mg.snap[s] /\ r \in cur.del[s] \cup pend[s]}
                              ELSE {}
                   U == [txn |-> stTxn + 1, wal |-> cur.wal,
                         segs |-> (cur.segs \ mg.ins) \cup (IF m # 0 THEN {m} ELSE {}),
                         del |-> [s \in SegIds |-> IF s \in mg.ins THEN {}
                                                   ELSE IF m # 0 /\ s = m THEN deleted
                                                   ELSE cur.del[s]],
                         next |-> nextSeg]
               IN WriteVersion(U, [kind |-> "merge", v |-> U], "merge")
            /\ UNCHANGED <<slots, wal, sealed, segRows, cur, slotCur, pend, mem, memDel, stTxn,
                           bg, mg, wrv, hist, nextId, absorbed, told, earlyTold, badPrefix,
                           lostAcked, refused, crashes, merges, nextSeg>>

(* The slot synced: the version is the one on disk; then it is published  *)
(* (Database::publish) as each kind does.                                 *)
VersionSync ==
    /\ inflight.on
    /\ slots' = [slots EXCEPT ![inflight.slot] = Slot("valid", inflight.v)]
    /\ inflight' = NoFlight
    /\ cur' = job.v /\ slotCur' = inflight.slot /\ stTxn' = job.v.txn
    /\ CASE job.kind = "flush" ->
              /\ mem' = {} /\ memDel' = {} /\ pend' = Empty
              /\ job' = [kind |-> IF FixIntent THEN "markflush" ELSE "reset", v |-> job.v]
              /\ UNCHANGED <<sealed, lock, bg, mg>>
         [] job.kind = "bg" ->
              /\ mem' = mem \ bg.all /\ memDel' = memDel \ bg.all /\ pend' = Empty
              /\ bg' = BgIdle
              /\ IF FixIntent
                 THEN /\ job' = [kind |-> "markbg", v |-> job.v]
                      /\ UNCHANGED <<sealed, lock>>
                 ELSE /\ sealed' = SelectSeq(sealed, LAMBDA l : l.t > bg.t)   \* remove_sealed
                      /\ job' = NoJob /\ lock' = "free"
              /\ UNCHANGED mg
         [] job.kind = "merge" ->
              /\ pend' = [s \in SegIds |-> IF s \in mg.ins THEN {} ELSE pend[s]]
              /\ mg' = MgIdle
              /\ job' = IF FixIntent THEN [kind |-> "markmerge", v |-> job.v] ELSE NoJob
              /\ lock' = IF FixIntent THEN lock ELSE "free"
              /\ UNCHANGED <<mem, memDel, sealed, bg>>
    /\ UNCHANGED <<wal, walDur, segRows, fileGrown, wrv, ghost, rare>>

(* FixIntent: the record marked done and synced once the slot is, before  *)
(* any log the version holds is removed (Wal::reset, remove_sealed).      *)
IntentDone ==
    /\ job.kind \in {"markflush", "markbg", "markmerge"}
    /\ intent' = [intent EXCEPT !.done = TRUE]
    /\ CASE job.kind = "markflush" ->
              /\ job' = [kind |-> "reset", v |-> job.v]
              /\ UNCHANGED <<sealed, lock>>
         [] job.kind = "markbg" ->
              /\ sealed' = SelectSeq(sealed, LAMBDA l : l.t > job.v.wal)   \* remove_sealed
              /\ job' = NoJob /\ lock' = "free"
         [] job.kind = "markmerge" ->
              /\ job' = NoJob /\ lock' = "free"
              /\ UNCHANGED sealed
    /\ UNCHANGED <<slots, inflight, wal, walDur, segRows, fileGrown, cur, slotCur, pend, mem,
                   memDel, stTxn, bg, mg, wrv, ghost, silent, rots>>

(* Damage after the fact (MaxRot): a slot written and synced fails its    *)
(* checksum later, its fields readable or not, or reads as zeros, while   *)
(* the other slot holds a valid version.                                  *)
Rot ==
    /\ rots < MaxRot
    /\ \E s \in {0, 1} :
         /\ slots[s].kind = "valid" /\ slots[1 - s].kind = "valid"
         /\ ~(inflight.on /\ inflight.slot = s)
         /\ \E d \in {Slot("torn", slots[s].v), Slot("torn", NoV), Zeros} :
              /\ ZeroSlot1 \/ ~(d = Zeros /\ s = 1 /\ slots[0].v.txn = 1)
              /\ slots' = [slots EXCEPT ![s] = d]
    /\ rots' = rots + 1
    /\ UNCHANGED <<inflight, wal, walDur, sealed, segRows, fileGrown, memv, wrv, ghost,
                   intent, silent>>

(* flush_inner, last: Wal::reset (a new log renamed over it), and the     *)
(* sealed logs the version holds removed.                                 *)
WalReset ==
    /\ job.kind = "reset"
    /\ wal' = <<>> /\ walDur' = 0
    /\ sealed' = SelectSeq(sealed, LAMBDA l : l.t > job.v.wal)
    /\ job' = NoJob /\ lock' = "free"
    /\ UNCHANGED <<slots, inflight, segRows, fileGrown, cur, slotCur, pend, mem, memDel,
                   stTxn, bg, mg, wrv, ghost, rare>>

-----------------------------------------------------------------------------
(* A crash, and the open that follows (Database::load, assemble).          *)

(* What a crash may leave of the slot being written. *)
SlotsAfter ==
    IF ~inflight.on THEN {slots[inflight.slot]}
    ELSE {inflight.old, Slot("valid", inflight.v)} \cup
         (IF Tear = "garbage"
          THEN {Slot("torn", inflight.v), Slot("torn", NoV)} \cup
               (IF inflight.old.kind # "zeros" THEN {Slot("torn", inflight.old.v)} ELSE {})
          ELSE {})

(* No crash follows a recovery that held no prefix (with damage after    *)
(* the fact an open may refuse, rightly, and have lost commits).          *)
Crash ==
    /\ crashes < MaxCrashes /\ ~badPrefix
    /\ \E j \in walDur..Len(wal), so \in SlotsAfter :
         LET walD   == SubSeq(wal, 1, j)
             slotsD == [slots EXCEPT ![inflight.slot] = so]
             vs     == NewestSlot(slotsD)
             V      == slotsD[vs].v
             logged == {r.txn : r \in Range(Cat(sealed) \o walD)}
             kept   == SelectSeq(sealed, LAMBDA l : l.t > V.wal)   \* read_sealed_wals
             recs   == Cat(kept) \o walD
             db0    == [segs |-> V.segs, del |-> V.del, pend |-> Empty, mem |-> {}, memDel |-> {}]
             db     == Replay(db0, recs, V.wal)
             replayed == {r \in Range(recs) : r.txn > V.wal}
             skipped  == \E r \in Range(recs) : r.txn <= V.wal
             reset  == skipped /\ db.mem = {} /\ \A s \in SegIds : db.pend[s] = {}
             \* the commits recovered: those the version holds, and replayed
             got    == {hist[i].id : i \in {x \in DOMAIN hist : hist[x].txn <= V.wal}}
                       \cup {r.id : r \in replayed} \cup absorbed
             n      == Cardinality(got)
             prefix == /\ got = {hist[i].id : i \in 1..n}
                       /\ Live(db) = StateAt(n)
             refuses == Refuses(slotsD, V, logged, fileGrown)
         IN \* with no valid slot the open fails outright (loud): left out
            /\ ValidSlots(slotsD) # {}
            /\ badPrefix' = (badPrefix \/ ~prefix)
            /\ lostAcked' = (lostAcked \/ ~(told \subseteq got))
            /\ refused' = (refused \/ refuses)
            /\ silent' = (silent \/ ((~prefix \/ ~(told \subseteq got)) /\ ~refuses))
            \* FixIntent: the open marks the record done once it opens a
            \* version at least as new, before it may reset the log
            /\ intent' = IF FixIntent /\ intent.txn <= V.txn
                          THEN [intent EXCEPT !.done = TRUE] ELSE intent
            /\ hist' = SubSeq(hist, 1, n)
            /\ told' = told \cap got
            /\ absorbed' = IF reset THEN got ELSE absorbed \cap got
            /\ slots' = slotsD /\ inflight' = NoFlight
            /\ wal' = IF reset THEN <<>> ELSE walD
            /\ walDur' = IF reset THEN 0 ELSE j
            /\ sealed' = IF reset THEN <<>> ELSE kept
            /\ fileGrown' = FALSE
            /\ cur' = V /\ slotCur' = vs
            /\ pend' = db.pend /\ mem' = db.mem /\ memDel' = db.memDel
            /\ stTxn' = Max({V.txn} \cup {r.txn : r \in replayed})
            \* ids past the version's are given again (pages no version names)
            /\ nextSeg' = V.next
            /\ segRows' = [s \in SegIds |-> IF s < V.next THEN segRows[s] ELSE {}]
    /\ lock' = "free" /\ job' = NoJob /\ bg' = BgIdle /\ mg' = MgIdle
    /\ wst' = [w \in Writers |-> "idle"]
    /\ queue' = <<>> /\ leading' = FALSE /\ leader' = NoW /\ group' = <<>> /\ todo' = <<>>
    /\ ctx' = NoCtx /\ ans' = [w \in Writers |-> -2]
    /\ crashes' = crashes + 1
    /\ UNCHANGED <<wop, wleft, nextId, earlyTold, versions, merges, rots>>

-----------------------------------------------------------------------------
Next ==
    \/ \E w \in Writers : Submit(w) \/ Take(w)
    \/ Begin \/ Append_ \/ SyncAppend \/ Apply \/ Finish
    \/ Flush \/ BgSeal \/ BgInstall \/ VersionSync \/ IntentDone \/ WalReset
    \/ MergeClaim \/ MergeBuild \/ MergeInstall
    \/ Rot \/ Crash

Spec == Init /\ [][Next]_vars

-----------------------------------------------------------------------------
(* 1. After a crash the database holds exactly the rows after a prefix of  *)
(* the commits (none partly), and every commit a writer was told of.       *)
RecoveredPrefix == ~ badPrefix
RecoveredAcknowledged == ~ lostAcked

(* 2. The rows live are those the commits left: a merge (or a flush)      *)
(* brings back no row deleted while it was built, and drops none.         *)
LiveRowsCommitted ==
    \/ badPrefix    \* (RecoveredPrefix's to report)
    \/ Live(DB) = StateAt(Len(hist) - (IF ctx.ph \in {"appended", "synced"} THEN 1 ELSE 0))

(* 3. A writer is told its write committed only once its records are      *)
(* durable.                                                               *)
ToldOnlyDurable == ~ earlyTold

(* A key has one live row at most. *)
OneRowAKey == badPrefix \/ \A k \in Keys : Cardinality({r \in Live(DB) : KeyOf(r) = k}) <= 1

(* An open after a crash, which lost no write, does not refuse. *)
OpensAfterCrash == ~ refused

(* With damage after the fact: an open that does not refuse lost no       *)
(* commit a writer was told of, and holds a prefix of the commits.        *)
NoSilentLoss == ~ silent

=============================================================================
