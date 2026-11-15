# Formal specifications of ChloeDB's protocols

TLA+ specifications of the protocols in ChloeDB where a wrong interleaving
loses or duplicates data, each checked with the TLC model checker on small
models. Each specification follows one piece of the code (named at its top)
closely enough that a step in a trace names a function, and is kept small
enough that TLC explores every state of its models. A model that finds a
bug keeps finding it: its configuration says the invariant it breaks and in
how many states, and `check.py` fails if TLC finds anything else.

```bash
python3 spec/check.py            # the small models, as CI runs them (seconds each)
python3 spec/check.py --all      # every model (the large ones take minutes to hours)
python3 spec/check.py FixStall   # one model
python3 spec/check.py --list
```

It needs Java 11 or newer, and downloads `tla2tools.jar` (TLC 1.8.0,
checked against its SHA-256) into `~/.cache/chloedb-spec`, or takes `--jar
PATH` or `$TLA2TOOLS`. The workflow Specs (`.github/workflows/specs.yml`)
runs the small models on a pull request that changes `spec/`. To read a
trace, run TLC in the model's folder:

```bash
cd spec/replication && java -cp tla2tools.jar tlc2.TLC -deadlock -config CILostVolume.cfg MCShardElection.tla
```

`-deadlock` is needed: the models bound their writes, crashes and terms,
so their behaviours end. The numbers below were measured on the 4-core
cloud container these were written in, one TLC worker a model, 2026-11-15.

## What is specified

| specification | the code | what TLC checks |
|---|---|---|
| [replication/ShardElection.tla](replication/ShardElection.tla) | `chloedb-server/src/cores/shard.rs`: `decide`, `Replica::take`, `joined`, `counted`, `Campaign`, `Primary::fence`, `term_start` | one copy a term, no acknowledged write lost, logs that match, and that a shard can always elect a primary |
| [storage/SnapshotPublish.tla](storage/SnapshotPublish.tla) | `chloedb/src/storage/db.rs`: `Database::snapshot`, `republish` (`publish_snapshots`) | a reader's snapshots never go back, and each holds every write that returned before it was asked for |
| [slots/SlotMove.tla](slots/SlotMove.tla) | `chloedb-server/src/coord/moves.rs`, `src/cores/slots.rs` (`flip_ok`, `Hold`, `Change::give`), `src/service/moving.rs`, `Service::set_slots` | once any server holds a move's flip, the shard the slots go to holds every write to them that was acknowledged, and none comes after |
| [clustermap/MapChange.tla](clustermap/MapChange.tla) | `chloedb-server/src/coord.rs` (`Coordinator::change`, `sync_heard`, the map Lease, `connect_with`'s survey), `src/cores/slots.rs` (`Change::synced`, `give`, `answered`, `in_doubt`, `flip_ok`), `Service::set_slots` | no two maps of one epoch, a coordinator routes only by a map a server held, and no server is left refusing every later map |
| [idempotency/IdempotencyKeys.tla](idempotency/IdempotencyKeys.tla) | `chloedb-server/src/idem.rs` (`Keys::begin`, `finish`, `record`, `recorded`), `src/service.rs` (`begin_key`, `apply_keyed_in`, `replicated`), `Primary::holding`, `term_opened` | a write retried with its key is applied once, and answered as acknowledged only while a majority holds it |
| [handover/Handover.tla](handover/Handover.tla) | `chloedb-server/src/replication.rs` (`Follow::promote`, `catch_up`, `campaign`), `src/cores/shard.rs` (`promote_check`, `hand_over`, `handover_hold`, `Replica::vote`), `src/service/failover.rs` (`Failover::due`) | a read served under the primary's lease holds every write acknowledged, one copy a term, and a handover leaves a primary serving |
| [wal/WalRecovery.tla](wal/WalRecovery.tla) | `chloedb/src/db/writes.rs` (group commit), `chloedb/src/storage/` (`commit_logged`, `flush_inner`, `install_merge`, `write_version`, `write_meta`, `Wal::reset`, `Database::load`, `damaged_slots`, `fallback_decision`, `fallback_keeps_commits`) | after any crash the rows are a prefix of the commits holding every acknowledged one, merges lose no delete, an open that lost nothing is not refused, and no loss goes unreported |

### Shard elections (`replication/`)

Three copies; a primary writes and ships to its replicas, answers a write
once a majority holds it, and crashes and starts again as a replica; a
replica asks to be elected (with a pre-vote, or by hand), the others vote,
and a newer term fences a primary; a replica loses its volume and starts
again empty with the same `--replica`. `Rule` picks the vote rule: 0.3.3's
(`pre-ne2`), main's before 0.3.4 (`ne2`, #512), the first fix tried for
0.3.4 (`narrow`), and 0.3.4's (`fix`, #551: `called_first`).
`MembersCampaign` is the follow-up below. `LossGuard` says when a volume
may be lost: at any time, as the simulator loses one (`may_lose_volume`),
or as the simulator's `--lost-volume` placement does (`quiet`: nothing the
lost copy forgets is unknown to the others). Time is not modelled: the
sticky delay and the lease only refuse more votes, so a safety property
that holds here holds with them.

| model | rule | bounds (terms, writes, crashes, lost volumes) | result | states |
|---|---|---|---|---|
| PreNE2LostVolume | pre-ne2 | 3, 2, 2, 1 (`quiet`) | **lost write** in 7 states (N-E2 as the audit found it) | 9,620 |
| NarrowLostVolume | narrow | 3, 2, 2, 1 (`quiet`) | **lost write** in 9 states | 39,830 |
| NE2Stall | ne2 | 3, 1, 1, 0 | **stall** in 4 states (below) | 54 |
| FixStall | fix | 3, 1, 1, 0 | no stall | 809,403 (6 min) |
| FixLostVolume | fix | 3, 2, 2, 1 (`quiet`) | **lost write** in 9 states (below) | 70,090 |
| MembersStall | fix + members | 3, 1, 1, 0 | no stall | 954,774 (6.5 min) |
| MembersLostVolume | fix + members | 3, 2, 2, 1 (`quiet`) | no error in every behaviour of up to 15 steps (the bug takes 9; stopped after an hour) | 14,755,974 |
| MembersAmnesia | fix + members | 3, 2, 2, 1 (`sim`) | **two primaries of one term** in 10 states (below) | 189,921 |
| CINE2Stall, CIFixStall, CIFixNoLoss, CILostVolume, CIMembersLostVolume, CIMembersStall | | terms 2 | as their large models | 32 to 73,116 (CI) |

What TLC found:

1. **The 0.3.4 blocker (#512's rule, `NE2Stall`).** c0 writes; its first
   call to c1 finds c1 behind, so c1 is no member; c0 crashes and starts
   again as a replica; c2 has heard from no primary. No copy can win an
   election: c1 grants no vote, c2 votes only for a copy that heard from
   none either, and c0 needs one of them. #551 fixed it (c1 grants its vote
   to the primary that called it), and `FixStall` finds no stall.
2. **A lost write after one lost volume (`FixLostVolume`, on #512 and on
   #551).** c0 calls c1 once (both empty), writes w1 and ships it to c2,
   which commits it; c2 loses its volume and starts again empty; c0's next
   call to c2 cannot be applied (c2 lacks w1), but c2 keeps the
   membership the call names; c2 asks to be elected in term 2 holding
   nothing, and c1, a member that missed w1, votes for it; c2 wins without
   w1. **Fixed here**: a copy no primary counts a member asks for no votes
   (`Replica::counted`), as Raft's learners do not; `MembersLostVolume`
   finds no lost write and `MembersStall` no stall. The simulator places
   it with `chloedb-sim --uncounted-candidate` and puts the bug back with
   `--bug campaigns-uncounted`.
3. **Lost volumes forget votes (`MembersAmnesia`, open).** With volumes
   lost as the simulator may lose them, a copy that lost its volume
   forgets the term it voted in: c1 is elected in term 2 by hand, c2 asks
   for term 2 by hand too, c1 loses its volume, starts again as a first
   copy that heard from no primary, and grants c2's request (which holds
   nothing and names no membership, as a new shard's would); two copies
   began term 2. The vote rules cannot tell this copy from one of a new
   shard. The direction that closes it: a replica started on an empty
   volume asks the shard's other copies first (as a primary started on
   one does since 0.3.4, `copies_holding`) and, when one knows a term,
   starts as a copy no primary counted, which neither votes nor asks.

### Published snapshots (`storage/`)

A writer applies versions and republishes; readers set the read flag and
take the published snapshot or build the current one. `SnapFALSE` is the
code before 5248809 (#339): **a reader's snapshot goes back** in 9 states
(it builds version 3 while the writer applies it unpublished, then takes
the published version 2), the "970 after 980" the test
`published_snapshots_never_go_back` caught. `SnapTRUE`, with the recheck,
finds nothing in 14,087 states (two readers, four versions).

### Slot moves from a replicated shard (`slots/`)

Shard A has two copies and one writer whose writes commit; replication is
abstract (a copy elected holds every committed write; the old primary
either restarts as a replica or lives on as a zombie until fenced). Shard
B (one server) is where the slots go. The move job finds its source once,
exports, holds writes there, exports the last changes, and gives the flip:
first to the coordinator's view of A's primary (refreshed by the change's
sync, `StaleView = FALSE`) and the source, then to every other server.

1. **A lost write when the source fails over during the flip (`MoveNow`,
   open).** The move holds writes on a1 and exports the last changes; the
   change's sync finds a1 primary; a1 crashes and starts again as a
   replica, and a2 is elected; a client's write to a moving slot reaches
   a2, which holds no hold, and commits; the flip is given to a1 first,
   which takes it, as a replica passes `flip_ok` (`covered = !writer`); B
   lacks the write, and coordinators route the slots to B. The same
   happens when a1 takes the flip and fails over before a2 is given it.
   The cluster simulator cannot find it: its shards have one server each.
2. **The new primary refuses the flip for good (`MoveStuck`).** In the
   same trace, a2 is a writer with no hold, so it refuses the flip in the
   rest of the change and every time it is given again: shard A's primary
   keeps the map before the flip while the cluster routes by the one after.
3. **A fix TLC passes (`Fix = "mark"`, `MoveMark`, `MoveMarkStale`).** The
   writer that takes the flip commits a mark through its shard's log first
   (`write_flip` writes one already, without waiting for a majority); a
   writer takes it only if its mark commits (a zombie's cannot), a replica
   only once it applied the mark, a primary whose log holds the mark
   refuses writes to the slots and takes the flip, and a server forgets
   its hold and export when it stops being the writer. TLC finds neither
   bug, with stale views too (27,010 states).

### Changes of the cluster map (`clustermap/`)

Three servers that keep their map across restarts, and two coordinators.
A map is the sequence of the changes that made it, so two maps of one
epoch differ exactly when different changes made them. A change syncs
(the newest map of the servers that answer), gives its map in doubt again
or makes its own, and gives it to the source first, one try at a time,
then to the rest; each answer is Took, Raced, Refused or lost, and a lost
call may still arrive later. Coordinators also follow the servers' maps
(watch) and give theirs to servers behind (speak). Constants: `Racing`,
`Forgetful`, `Lease`, `Survey` (what a restarted coordinator routes by:
`kept`, `all`, `any`), `Move` (the flip, source first, `flip_ok`), `Bugs`
(the simulator's four map bugs put back), `LeaseKeeps` (the Lease keeps
the newest map a server may hold, which `Coordinator::change` does since
TLC found `LeaseRacing`) and `Quorum` (a proposal).
Invariants: `NoFork` (no two maps of one epoch ever taken), `RoutesHeld`
(a coordinator routes only by a map some server holds or held) and
`NoStuck` (no server left refusing every later map).

| model | setting | changes, restarts | result | states | s |
|---|---|---|---|---|---|
| GivesTwice | `forgets_doubt` | 2, 0 | **fork** in 10 | 188 | 2 |
| RoutesUntaken | `keeps_untaken` | 2, 0 | **RoutesHeld** in 6 | 27 | 2 |
| ForgetsRedone | `forgets_redone` | 3, 0 | **fork** in 15 | 1,595 | 4 |
| AroundSource | Move, `skips_source` | 2, 0 | **NoStuck** in 9 | 354 | 2 |
| Forgetful | one coordinator, forgetful | 2, 1 | **fork** in 8 | 128 | 2 |
| Racing | racing | 2, 0 | **fork** in 7 | 107 | 2 |
| LeaseRacing | racing under the Lease | 2, 0 | **fork** in 9 (below) | 319 | 2 |
| SurveyAny | one coordinator keeping its doubt, `any` | 2, 1 | **fork** in 10 (below) | 559 | 2 |
| KeepsRacing | as LeaseRacing, LeaseKeeps | 2, 0 | no error | 4,552 | 2 |
| Keeps (CIKeeps) | racing, Lease, forgetful, `any`, Move, LeaseKeeps | 4, 2 (2, 1) | no error | 10,581,320 (42,650) | 1,552 (7) |
| QuorumRacing | Quorum, no Lease | 2, 0 | **fork** in 15 | 2,593 | 4 |
| OneKeeps (CIOneKeeps) | one coordinator keeping its doubt | 5, 2 (3, 1) | no error | 198,101 (9,103) | 51 (4) |
| OneMove (CIOneMove) | as OneKeeps, Move | 4, 2 (3, 1) | no error | 297,659 (43,206) | 55 (8) |
| LeaseOne (CILeaseOne) | one coordinator, Lease, `all`, Move | 4, 2 (3, 1) | no error | 409,678 (60,678) | 69 (13) |
| Quorum (CIQuorum) | racing, Lease, forgetful, `any`, Move, Quorum | 4, 2 (2, 1) | no error | 2,789,110 (29,280) | 444 (10) |

What TLC found:

1. **The documentation holds where it says.** TLC finds each of the four
   map bugs the simulator puts back, and the fork with racing and with
   forgetful coordinators; it finds no fork and no stuck source with one
   coordinator that keeps its doubt (as the simulator's do, in memory, or
   in the Lease with shards of one server).
2. **The Lease did not close the fork (`LeaseRacing`, fixed).**
   [server.md](../docs/chloedb/server.md) says the Lease narrows the fork
   to a coordinator paused for longer than 90 s. But c1's change takes the
   Lease, gives map `<<11>>` (epoch 2), and lands on s1 and s2 while s3's
   answer times out (Missed). The change is done, so no map is left in
   doubt and `release(None)` empties the Lease's. c2, cut off from s1 and
   s2 (its watch never heard of `<<11>>`), takes the Lease, syncs from s3
   alone (`sync_heard` needs no answer from any particular server), makes
   `<<21>>` of epoch 2, and s3 takes it: two maps of epoch 2.

   Fixed (`LeaseKeeps`): a change that ends keeps in the Lease the newest
   map a server may hold, the map it made among them, not only the one in
   doubt, and the next change gives it again first unless the servers it
   reaches hold it. c2 then gives `<<11>>` to s3 and makes its change from
   it. The Lease is an agreement (the API server's compare-and-swap), so
   this closes the fork for coordinators the operator runs without
   waiting for a majority of servers, which `Quorum` would: a cluster of
   two servers with one down could not change its map. `KeepsRacing` and
   `Keeps`, with restarts surveying any copy and a move, find no fork;
   `tests/map_lease.rs` makes the same race with real servers.
3. **A restart forks with replicated shards (`SurveyAny`, open without the
   Lease).** The same
   first steps without the Lease: c1 starts again, and `connect_with`'s
   `survey` fails only when every copy of a shard fails, so with s3 alone
   answering c1 routes by epoch 1, and its next change gives `<<12>>` of
   epoch 2 to s3. The cluster simulator cannot find this: its shards have
   one server each and its coordinators keep their map when they crash.
   Under the Lease the fix above closes it too (c1 finds `<<11>>` in the
   Lease, `Keeps`); without it, it is the fork server.md names for a
   coordinator that starts again.
4. **A direction TLC passes (`Quorum`).** Under the Lease, a change lands
   once a majority took its map, a sync takes up the newest map of a
   majority, and a change first gives the map it synced to again until a
   majority holds it. Without that last rule TLC finds a fork in 14 states
   (the sync takes up a map one server holds, the change drops it from
   doubt, its give fails, and the next coordinator's majority misses it);
   without the Lease, two changes at once still fork (`QuorumRacing`).

### Idempotency keys across failover (`idempotency/`)

One shard of three copies (`idem.rs`, `Service::begin_key`,
`apply_keyed_in`, `replicated`, `Primary::holding`, `term_opened`); a
client's keyed write, retried through the coordinator to any primary; the
key in the write's own log entry; memory lost on restart and kept on
stepping down. Elections are abstract: a copy at least as up to date as a
majority wins. `ForgetOnStepDown` and `WriterChecks` are fixes;
`AcksAnOlderTerm` puts DS-5 back.

| model | fixes | bounds (terms, sends, other writes, crashes, evictions) | result | states |
|---|---|---|---|---|
| KeysNow | none | 3, 2, 0, 0, 0 | **lost write** in 10 states (below) | 3,489 |
| KeysDS5 | memory, DS-5 back | 4, 2, 1, 1, 0 | **lost write** in 12 states | 160,437 |
| KeysFix (CIKeysFix) | memory | 4, 2, 1, 1, 0 (3, 2, 1, 1, 0) | no error | 1,129,195, 4 min (127,595) |
| KeysEvict | memory | 1, 2, 0, 0, 1 | **applied twice** in 6 states (below) | 24 |
| EvictFix (CIEvictFix) | both | 4, 3, 0, 1, 1 (3, 3, 0, 1, 1) | no error | 1,102,814, 3 min (129,499) |
| KeysRowSeq | memory | 2, 2, 0, 1, 0 | `SameSeq` broken in 8 states | 613 |

What TLC found:

1. **The keys in memory outlive the log (`KeysNow`, open).**
   `Service.idempotency` is made once (`Service::new`) and nothing clears
   it, while the role changes inside the process (`step_down`,
   `fail_over`, `promote`, `configure`), `install_copy` replaces `_sources`
   but not the keys in memory, and `begin_key` asks memory before the
   database:
   1. c0 applies w with key k as transaction 1 of term 1, and
      `Keys::finish` keeps its seq_no 1; its replicas miss it, so the
      client's answer is a 504 or in doubt.
   2. c1 is elected in term 2 with c2's vote, fences c0, and
      `Service::step_down` makes c0 a replica in its process, with its
      memory.
   3. c1 ships to c0, whose differing log is replaced by a copy: w and
      its `_sources` row are gone.
   4. c0 is elected in term 3 (by a handover, or after c1 fails).
   5. The retry reaches c0, and `Keys::begin` answers Done with seq 1.
      c1's transaction 2 is acknowledged, so `acked(1)` holds and the
      retry is answered 200, though no copy holds w.

   The fix TLC passes: forget the keys whenever the role changes (or tag
   each with its term). The simulator answers retries only from the
   database (`sim/copy.rs`), so it cannot find this.
2. **DS-5 (`KeysDS5`).** Put back, a retry answered from the key's row
   with the database's last number is acknowledged by an older term's
   transaction, and the write is then lost; the same bounds without it
   find nothing (`KeysFix`).
3. **A key evicted while its write runs (`KeysEvict`, open).**
   `Keys::begin` forgets the oldest keys past the TTL or 100,000 whatever
   their state, `Busy` ones too. A retry while the write is in flight is
   then New, no row is there yet (`idem::record` writes it in the write's
   own transaction, with `set_source`, which replaces), and both attempts
   commit. It needs 100,000 other keys claimed on the shard during one
   write, which a busy coordinator (every write it sends has a key) can
   reach while a write waits for an embedding core. The fix TLC passes:
   the writer looks the key's row up in the write's own transaction.
4. **The documentation (`KeysRowSeq`).** server.md says a retry is
   answered the first attempt's seq_no; one answered from the key's row
   gets the database's last number, as its section on keys says
   elsewhere. `RetrySeesWrite` (the number covers the write) holds.

### Handover and the read lease (`handover/`)

A discrete clock; each duration the code measures is an age capped at its
bound. A promotion by hand: the copies' `Hello` (`promote_check`), the
pre-vote that pauses a serving primary (`hand_over`), `vote_self`, the
copies of `Asked.primaries` asked first, then the rest, and
`handover_hold`. Automatic failover: `Failover::due` and `Replica::vote`'s
sticky refusal. Calls may be lost; `Drift` takes durations at their worst.
Invariants: `LeaseReadsFresh` (a read served under the lease holds every
write acknowledged to its client), `OneCopyATerm`, `OldPrimaryAcksNothing`,
`OneWriter` (which the code does not promise: `TwoWriters`) and
`NoOrphanedFence` (with two copies by hand: never a fenced primary and none
serving). Three copies, terms 2 and a failover delay of 3 ticks unless a
model says otherwise.

| model | result | states (time) |
|---|---|---|
| LeaseBug (`--bug takes-writes-in-a-lease`) | **stale read** in 11 states | 5,986 (4 s) |
| CILease, Lease (delay 4) | no error | 94,335 (49 s), 277,263 (141 s) |
| HoldUnasked (terms 3) | **stale read** in 19 states (below) | 450,708 (123 s) |
| HoldAllGranted (its fix, terms 3, behaviours of 21 steps) | no error | 908,342 (349 s) |
| Auto | no error | 9,790 (8 s) |
| Restart, RestartFix (its fix) | **stale read** in 11 states (below), no error | 6,656 (4 s), 72,480 (36 s) |
| Drift1, Drift2 (delay 8, clocks off by 1 or 2) | no error, **stale read** in 13 states | 154,024 (58 s), 10,286 (6 s) |
| TwoWriters (no failover) | **two writers** in 8 states | 561 (2 s) |
| OrderBug, Order (two copies, terms 3) | **orphaned fence** in 9 states, no error | 94, 502 (3 s) |
| OrderLossy, OrderRace | **orphaned fence** in 8 and 18 states | 236, 716 (5 s) |

What TLC found:

1. **`handover_hold` trusts a `Hello` that failed (`HoldUnasked`, open).**
   1. c1 is promoted while no `Hello` is answered; c0, term 1's primary,
      answers only the pre-vote, and c1 wins term 2 with c2.
   2. c2 is promoted: c0, not fenced yet, answers `Hello` and c1's
      fails, so `Asked.primaries` is [c0].
   3. c1's `Replicate` to c2 is answered, and c2 votes for itself.
   4. c0 grants and steps down; c2 wins term 3, and as every copy of
      `Asked.primaries` granted, it holds no lease wait.
   5. c2's write is acknowledged with c0's copy, and c1 serves reads
      without it until it is fenced.

   Rare (the copy that did not answer must be the newest primary), but
   real. The fix TLC passes: no wait only when every other copy granted.
2. **A replica started again votes at once (`Restart`, open).**
   `Replica::vote` refuses a vote for the failover delay after a call of
   its primary only when `called_at` is set, and a replica started again
   has none. c0's calls to c2 fail for the delay; c1 answers c0 and
   starts again; c2 asks to be elected (`Failover::due`) and c1 grants;
   c2 wins term 2, its write is acknowledged with c1, and c0 still serves
   reads on c1's answer from before. It needs a restart quicker than the
   lease (the simulator's crashes last 50 ms to 4 s). The fix TLC passes:
   refuse for the delay from `opened` too.
3. **The vote order covers its own path only (`Order`).** A lost `Hello`
   or vote, the pause running out, or two promotions at once leave the
   candidate's own vote fencing the primary: two copies with no automatic
   failover have no primary until an operator promotes again (which
   works).
4. **Clocks.** The lease holds with each copy's clock off by an eighth of
   the failover delay (`Drift1`), not by more (`Drift2`).

`LeaseReadsFresh` counts the writes acknowledged to their client: a write
answered in doubt that a later term commits may be seen only on the new
primary. Promotions that overlap are checked with two copies only.

### Commit path and crash recovery (`wal/`)

Group commit, one log append and fdatasync per transaction, foreground and
background flushes, a merge built outside the writer lock, the two meta
slots, a crash at any step, and `Database::load`. Sync is on unless a
model says otherwise. After every recovery the rows are a prefix of the
commits and hold every acknowledged one, merges neither bring back nor
drop rows, and a writer is told of its write only once it is durable
(`CIRecover`, `CIGroups`, and `Recover` with 757,341 states).
`CIMergeNoCarry` and `CINoSyncFirst` each drop a step the code takes, to
show why it is there.

| model | differs from the default (w1 upserts k1 twice, w2 deletes it; 3 versions, 1 merge, 1 crash, slots written whole) | result | states | s |
|---|---|---|---|---|
| CIRecover | | no error | 74,275 | 11 |
| CIMergeNoCarry | a merge drops the deletes made since its snapshot | `LiveRowsCommitted` broken in 19 states | 8,561 | 3 |
| CINoSyncFirst | no sync, nor `sync_appends` | `RecoveredPrefix` broken in 15 states | 4,704 | 2 |
| CINoSync | no sync | no error | 128,965 | 20 |
| CITornSlot | slots can tear | **`OpensAfterCrash` broken** in 14 states (below) | 2,549 | 2 |
| CITornSafe | slots can tear, without `OpensAfterCrash` | no error | 90,973 | 15 |
| CIRot | slots can tear or be damaged later | `NoSilentLoss` broken in 13 states (documented) | 2,696 | 2 |
| CIRotTold, RotTold | as CIRot but the documented case, 1 write each (default writes) | no error | 25,346 (709,663) | 5 (113) |
| CIFixIntent, FixTorn | the fix, slots can tear (2 crashes) | no error | 114,545 (369,966) | 18 (120) |
| CIFixRot, FixRot | the fix, later damage, 1 write each (default writes) | no error | 37,342 (985,908) | 6 (156) |
| CIGroups | groups that fail, 2 keys | no error | 209,294 | 28 |
| Recover | either writer upserts or deletes, 2 crashes | no error | 757,341 | 221 |

What TLC found:

1. **A torn slot refuses an open that lost nothing (`CITornSlot`, open).**
   1. w1's upsert commits as transaction 2.
   2. A flush writes version 3 to slot 1, and `Wal::reset` empties the
      log.
   3. `install_merge` writes version 4 to slot 0, and a crash in
      `write_meta` tears it: it fails its checksum and its fields cannot
      be read.
   4. `Database::load`: `damaged_slots` answers no manifest, so
      `fallback_decision` does not know the newer version, and
      `fallback_keeps_commits(3, 2, None, [])` refuses to open without
      `recover`, though version 3 holds every write.

   It needs a device that tears a write inside the 84-byte meta (the
   tests' power-loss model writes sectors whole). Torn slots never lose
   data (`CITornSafe`).
2. **Slot 1 zeroed later, beside the first version (`CIRot`, documented
   in [index.md](../docs/chloedb/index.md)).** The open goes ahead at
   version 1 and loses the first flush's commits without a word. Other
   damage after the fact is caught (`CIRotTold`, `RotTold`).
3. **A fix TLC passes (`FixIntent`).** Before it writes a slot,
   `write_version` writes and syncs a record of the version's transaction
   and slot, and marks it done after the slot's sync and before
   `Wal::reset` or `remove_sealed`; an open marks it done when it opens
   that version or a newer one. A damaged slot the record names in flight,
   newer than the version opened, is then a write the crash cut short,
   not damage; and any record shows that slot 1 was written. No clean
   crash is refused and no loss goes unreported (`CIFixIntent`,
   `FixTorn`, `CIFixRot`, `FixRot`).

## What is next, by risk

The open findings above, by what they cost and how likely they are, each
with the fix its specification checks:

1. **A slot move loses a write when its source fails over during the
   flip** (`slots/`, `MoveNow`): the mark committed through the source's
   log.
2. **The cluster map forks under the Lease, and on a restart with
   replicated shards** (`clustermap/`, `LeaseRacing`, `SurveyAny`): fixed
   under the Lease, which keeps the newest map made (`Keeps`); without the
   Lease, changes that land on, and sync from, a majority (`Quorum`).
3. **A retry is answered 200 for a write no copy holds, or applied twice**
   (`idempotency/`, `KeysNow`, `KeysEvict`): forget the keys at a change of
   role, and look the key's row up in the write's own transaction.
4. **Stale reads under the lease** (`handover/`, `HoldUnasked`, `Restart`):
   no wait only when every other copy granted, and the sticky refusal
   counted from the replica's start.
5. **A copy that lost its volume forgets its vote** (`replication/`,
   `MembersAmnesia`): ask the other copies first, as a primary does.
6. **A torn meta slot refuses an open that lost nothing, and a slot zeroed
   later loses the first flush unreported** (`wal/`, `CITornSlot`,
   `CIRot`): a record of the version in flight.

Not specified yet: **the operator's rolling upgrades** (one server at a
time, handover before restart, the protocol version gate) and **ChloeCtx
sync** (per-project keys, pushes that cross), liveness more than safety;
**follow** between clusters (`src/operator/follow.rs`); and **merges'
deletes across a slot move** (where `slots/` and `wal/` meet).

Where another tool fits better:

- **Stateright** (Rust, the actor model) could check `cores/shard.rs` and
  `cores/slots.rs` themselves rather than a model of them: they are pure
  decisions already, and the simulator drives them. It would find what
  these find without the translation, on smaller bounds.
- **Kani** fits the byte-level code the fuzzers and Miri cover: the
  manifest's two meta slots, the WAL's record framing, the posting-list
  codecs (`chloedb/src/postings/`), proving no panic and that decode
  inverts encode for every input up to a size.
- **Trace validation**: the simulator (`chloedb-sim --trace`) already logs
  each step of `cores::shard` it takes; mapping those lines to the actions
  of `ShardElection` and checking each run's trace is a behaviour of the
  specification (TLC with the trace as a constraint) would tie the model
  to the code, so that a change to one without the other fails.
