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

## What is next, by risk

The protocols left, ranked by what a bug would cost and how little the
tests reach them. Each would follow the code as these do.

1. **Handover and the read lease** (`handover_hold`, the handover vote
   order in `Replica::campaign`, `promote_check`): a replica elected by
   hand must not take writes inside the replaced primary's lease, and the
   primaries' votes come first. Needs time in the model (a lease), so a
   bounded clock or PlusCal with a tick action.
2. **Idempotency keys across failover** (the key kept in the write's own
   transaction, DS-5's `term_start`, retries whose answer was lost): a
   retried write applies once, and is answered as acknowledged only once
   a majority holds it in the current term. Extends `ShardElection`.
3. **Cluster map changes** (`Change::synced`, `give`, `failed`, maps in
   doubt, two coordinators, `sync_heard`'s take-up of the newest map): no
   two servers hold different maps of one epoch, and a change in doubt is
   redone before any other. Composes with `SlotMove`.
4. **WAL, flush, merge and the two meta slots** (`Database::commit`, group
   commit #273, `install_merge` outside the writer lock, crash recovery
   from either meta slot): a crash at any step recovers a prefix of the
   committed transactions, and a merge loses no delete that raced it.
5. **The operator's rolling upgrades** (one server at a time, handover
   before restart, the protocol version gate) and **ChloeCtx sync**
   (per-project keys, pushes that cross): liveness more than safety.

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
