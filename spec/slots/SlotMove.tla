------------------------------ MODULE SlotMove ------------------------------
(***************************************************************************)
(* A slot move whose source shard is replicated (docs/chloedb/server.md,   *)
(* "Moving slots"; chloedb-server/src/coord/moves.rs, src/cores/slots.rs,  *)
(* src/service/moving.rs, Service::set_slots).                             *)
(*                                                                         *)
(* Shard A's copies (SA) hold the slots; one of them, `primary`, takes the *)
(* writes that commit. Replication is abstract: a copy elected has every   *)
(* write that committed (spec/replication checks that), and an old primary *)
(* either restarts (a replica, its memory gone) or lives on as a zombie    *)
(* (it still thinks it takes writes, but none of its writes commits) until *)
(* it is fenced. Shard B (one server, b) is where the slots go. Only the   *)
(* writes to the moving slots are counted: write i is the i-th to commit   *)
(* on shard A; B holds every write <= copied.                              *)
(*                                                                         *)
(* The move job finds its source `src` once (MoveStep::Source), exports    *)
(* every row from it, then what changed (Service::export_slots: only on    *)
(* the server that made the export before, `exported`), holds writes there *)
(* (freeze_slots: `hold` = the shard's last write then; renewed, it runs   *)
(* out after 30 s unless asked for again), exports the last changes, and   *)
(* gives the flip map: first to the coordinator's view of A's primary and  *)
(* to src (Change::give), then to every other server. A server of A takes  *)
(* it (flip_ok) when it is no writer (a replica, or a primary another      *)
(* replaced), or when its hold began at or before its last export          *)
(* (Moving::covers). A first that refuses fails the change; one of the     *)
(* rest that refuses is missed. Once any server holds the flip, a          *)
(* coordinator takes it up (sync_heard installs the newest map a server    *)
(* holds) and routes the slots to B.                                       *)
(*                                                                         *)
(* Fix = "mark" is a proposal: the writer that takes the flip commits a    *)
(* mark through its shard's log first (write_flip already writes one,      *)
(* without waiting for a majority); a writer takes it only if its mark     *)
(* commits (a zombie's cannot), a replica only once it applied the mark,   *)
(* a primary whose log has the mark refuses writes to the slots and takes  *)
(* the flip, and a server forgets its hold and export when it stops being  *)
(* the writer.                                                             *)
(***************************************************************************)
EXTENDS Naturals, Integers, Sequences, FiniteSets

CONSTANTS SA, b, MaxWrites, MaxFailovers, MaxRestarts, Fix, StaleView

All == SA \cup {b}
None == -1

VARIABLES
    primary,    \* the copy of A whose writes commit
    writer,     \* each copy of A: whether it takes clients' writes (a primary nobody replaced)
    has,        \* each copy of A: the writes it holds
    aw,         \* the writes to the moving slots that committed on A (acknowledged)
    map,        \* each server: "old" (slots moving A -> B, copying) or "new" (the flip)
    hold,       \* each copy of A: the hold's `since`, or None
    exported,   \* each copy of A: its last export of the moving slots (a write number), or None
    copied,     \* B holds every write <= copied
    cview,      \* the coordinator's view of A's primary (its last Hello)
    cmap,       \* the map the coordinator routes by
    jpc,        \* the move job
    src,        \* its source, found once
    cpc,        \* the change giving the flip: "idle" | "first" | "rest" | "done" | "failed"
    firsts, k, took, rest,
    marked,     \* Fix: A's log holds the flip's mark (committed)
    sawMark,    \* Fix: each copy applied the mark
    failovers, restarts

vars == <<primary, writer, has, aw, map, hold, exported, copied, cview, cmap, jpc, src,
          cpc, firsts, k, took, rest, marked, sawMark, failovers, restarts>>

Max(x, y) == IF x > y THEN x ELSE y

Init ==
    /\ primary \in SA
    /\ writer = [s \in SA |-> s = primary]
    /\ has = [s \in SA |-> 0]
    /\ aw = 0
    /\ map = [s \in All |-> "old"]
    /\ hold = [s \in SA |-> None]
    /\ exported = [s \in SA |-> None]
    /\ copied = 0
    /\ cview = primary
    /\ cmap = "old"
    /\ jpc = "source" /\ src = primary
    /\ cpc = "idle" /\ firsts = <<>> /\ k = 1 /\ took = 0 /\ rest = {}
    /\ marked = FALSE /\ sawMark = [s \in SA |-> FALSE]
    /\ failovers = 0 /\ restarts = 0

Covers(s) == hold[s] # None /\ exported[s] # None /\ hold[s] <= exported[s]

(* Service::set_slots for the flip map (flip_ok), or the fix's rule. *)
TakesFlip(s) ==
    \/ s = b
    \/ map[s] = "new"
    \/ /\ s \in SA
       /\ IF Fix = "mark"
          THEN IF writer[s] THEN s = primary /\ (Covers(s) \/ marked) ELSE sawMark[s]
          ELSE ~ writer[s] \/ Covers(s)

-----------------------------------------------------------------------------
(* Shard A *)

(* A client's write to a moving slot, routed by the map before the flip,   *)
(* reaches A's primary (Coordinator::moved finds it after a refusal). A    *)
(* hold makes it wait; once the map changed it is refused, to go to B.     *)
Write ==
    /\ cmap = "old" /\ aw < MaxWrites
    /\ writer[primary] /\ map[primary] = "old" /\ hold[primary] = None
    /\ ~ (Fix = "mark" /\ marked)
    /\ aw' = aw + 1
    /\ has' = [has EXCEPT ![primary] = aw + 1]
    /\ UNCHANGED <<primary, writer, map, hold, exported, copied, cview, cmap, jpc, src,
                   cpc, firsts, k, took, rest, marked, sawMark, failovers, restarts>>

Ship(s) ==
    /\ s # primary /\ ~ writer[s]
    /\ (has[s] # aw \/ sawMark[s] # marked)
    /\ has' = [has EXCEPT ![s] = aw]
    /\ sawMark' = [sawMark EXCEPT ![s] = marked]
    /\ UNCHANGED <<primary, writer, aw, map, hold, exported, copied, cview, cmap, jpc, src,
                   cpc, firsts, k, took, rest, marked, failovers, restarts>>

(* A replica holding every committed write is elected; the old primary     *)
(* restarted (a replica, its hold and export forgotten) or is a zombie.    *)
Failover(q, zombie) ==
    /\ q # primary /\ ~ writer[q] /\ failovers < MaxFailovers
    /\ has[q] = aw
    /\ failovers' = failovers + 1
    /\ primary' = q
    /\ writer' = [writer EXCEPT ![q] = TRUE, ![primary] = zombie]
    /\ sawMark' = [sawMark EXCEPT ![q] = marked]
    /\ IF zombie
       THEN UNCHANGED <<hold, exported>>
       ELSE /\ hold' = [hold EXCEPT ![primary] = None]
            /\ exported' = [exported EXCEPT ![primary] = None]
    /\ UNCHANGED <<has, aw, map, copied, cview, cmap, jpc, src, cpc, firsts, k, took, rest,
                   marked, restarts>>

(* A zombie learns of the newer term (its replaced_by is set). *)
Fence(s) ==
    /\ s # primary /\ writer[s]
    /\ writer' = [writer EXCEPT ![s] = FALSE]
    /\ IF Fix = "mark"
       THEN /\ hold' = [hold EXCEPT ![s] = None]
            /\ exported' = [exported EXCEPT ![s] = None]
       ELSE UNCHANGED <<hold, exported>>
    /\ UNCHANGED <<primary, has, aw, map, copied, cview, cmap, jpc, src, cpc, firsts, k, took,
                   rest, marked, sawMark, failovers, restarts>>

(* A copy that is not the primary restarts: it forgets its hold and export. *)
Restart(s) ==
    /\ s # primary /\ restarts < MaxRestarts
    /\ (hold[s] # None \/ exported[s] # None \/ writer[s])
    /\ restarts' = restarts + 1
    /\ writer' = [writer EXCEPT ![s] = FALSE]
    /\ hold' = [hold EXCEPT ![s] = None]
    /\ exported' = [exported EXCEPT ![s] = None]
    /\ UNCHANGED <<primary, has, aw, map, copied, cview, cmap, jpc, src, cpc, firsts, k, took,
                   rest, marked, sawMark, failovers>>

(* A hold nobody asked for again within its 30 s runs out. *)
Expire(s) ==
    /\ hold[s] # None
    /\ hold' = [hold EXCEPT ![s] = None]
    /\ UNCHANGED <<primary, writer, has, aw, map, exported, copied, cview, cmap, jpc, src,
                   cpc, firsts, k, took, rest, marked, sawMark, failovers, restarts>>

-----------------------------------------------------------------------------
(* The coordinator *)

Hello ==
    /\ cview # primary
    /\ cview' = primary
    /\ UNCHANGED <<primary, writer, has, aw, map, hold, exported, copied, cmap, jpc, src,
                   cpc, firsts, k, took, rest, marked, sawMark, failovers, restarts>>

(* sync_heard: a server holds a newer map, which the coordinator installs. *)
Adopt ==
    /\ cmap = "old" /\ \E s \in All : map[s] = "new"
    /\ cmap' = "new"
    /\ UNCHANGED <<primary, writer, has, aw, map, hold, exported, copied, cview, jpc, src,
                   cpc, firsts, k, took, rest, marked, sawMark, failovers, restarts>>

Unchanged_A == UNCHANGED <<primary, writer, has, aw, marked, sawMark, failovers, restarts>>

(* moving_from_here: only a writer whose map moves the slots from it. *)
CanExport(s) == writer[s] /\ map[s] = "old"

(* Coordinator::primary. With StaleView, the Hellos that would tell the    *)
(* coordinator of a new primary may go unanswered (2 s) for a while.       *)
View == IF StaleView THEN cview ELSE primary

Source ==
    /\ jpc = "source"
    /\ src' = View /\ cview' = View /\ jpc' = "copy"
    /\ Unchanged_A
    /\ UNCHANGED <<map, hold, exported, copied, cmap, cpc, firsts, k, took, rest>>

(* An export of every row (changes = FALSE) or of what changed since this  *)
(* server's last export; one that fails gives the move up.                 *)
Export(changes, next) ==
    /\ IF CanExport(src) /\ (changes => exported[src] # None)
       THEN /\ copied' = Max(copied, has[src])
            /\ exported' = [exported EXCEPT ![src] = has[src]]
            /\ jpc' = next
       ELSE /\ jpc' = "aborted"
            /\ UNCHANGED <<copied, exported>>
    /\ Unchanged_A
    /\ UNCHANGED <<map, hold, cview, cmap, src, cpc, firsts, k, took, rest>>

FirstExport == jpc = "copy" /\ Export(FALSE, "catchup")
CatchUp == jpc = "catchup" /\ Export(TRUE, "catchup")

(* freeze_slots: a hold kept keeps its `since`. *)
Hold ==
    /\ jpc = "catchup"
    /\ IF CanExport(src)
       THEN /\ hold' = [hold EXCEPT ![src] = IF @ = None THEN has[src] ELSE @]
            /\ jpc' = "final"
       ELSE /\ jpc' = "aborted" /\ UNCHANGED hold
    /\ Unchanged_A
    /\ UNCHANGED <<map, exported, copied, cview, cmap, src, cpc, firsts, k, took, rest>>

(* The hold asked for again every 10 s while the move holds and flips; one *)
(* that ran out begins again, since now.                                   *)
Renew ==
    /\ jpc \in {"final", "flip"} /\ cpc \in {"idle", "first"}
    /\ CanExport(src) /\ hold[src] = None
    /\ hold' = [hold EXCEPT ![src] = has[src]]
    /\ Unchanged_A
    /\ UNCHANGED <<map, exported, copied, cview, cmap, jpc, src, cpc, firsts, k, took, rest>>

LastExport == jpc = "final" /\ Export(TRUE, "flip")

(* Change::synced and give: the view of the source (sync_heard asks every *)
(* server) first, then src.                                               *)
Sync ==
    /\ jpc = "flip" /\ cpc = "idle"
    /\ firsts' = IF View = src THEN <<src>> ELSE <<View, src>>
    /\ cview' = View
    /\ cpc' = "first" /\ k' = 1 /\ took' = 0
    /\ Unchanged_A
    /\ UNCHANGED <<map, hold, exported, copied, cmap, jpc, src, rest>>

Take(s) ==
    /\ map' = [map EXCEPT ![s] = "new"]
    /\ marked' = (marked \/ (Fix = "mark" /\ s \in SA /\ writer[s]))
    /\ sawMark' = IF Fix = "mark" /\ s \in SA /\ writer[s]
                  THEN [sawMark EXCEPT ![s] = TRUE] ELSE sawMark

GiveFirst ==
    /\ cpc = "first"
    /\ LET s == firsts[k] IN
       IF TakesFlip(s)
       THEN /\ Take(s)
            /\ took' = took + 1
            /\ IF k = Len(firsts)
               THEN /\ cpc' = "rest" /\ k' = k
                    /\ rest' = All \ {firsts[i] : i \in 1..Len(firsts)}
               ELSE /\ k' = k + 1 /\ UNCHANGED <<cpc, rest>>
            /\ UNCHANGED jpc
       ELSE \* Refused: the change fails; a flip no server took gives the move up,
            \* one a server took is in doubt, given again to the rest (below).
            /\ IF took = 0
               THEN /\ cpc' = "failed" /\ jpc' = "aborted" /\ UNCHANGED rest
               ELSE /\ cpc' = "rest" /\ UNCHANGED jpc
                    /\ rest' = {s2 \in All : map[s2] = "old"}
            /\ UNCHANGED <<map, marked, sawMark, took, k>>
    /\ UNCHANGED <<primary, writer, has, aw, hold, exported, copied, cview, cmap, src,
                   firsts, failovers, restarts>>

(* The rest, in parallel: a server that refuses is missed, and given the  *)
(* map again when it next answers (GiveAgain).                            *)
GiveRest(s) ==
    /\ cpc = "rest" /\ s \in rest
    /\ IF TakesFlip(s) THEN Take(s) ELSE UNCHANGED <<map, marked, sawMark>>
    /\ rest' = rest \ {s}
    /\ IF rest' = {}
       THEN cpc' = "done" /\ cmap' = "new" /\ jpc' = "done"
       ELSE UNCHANGED <<cpc, cmap, jpc>>
    /\ UNCHANGED <<primary, writer, has, aw, hold, exported, copied, cview, src, firsts, k, took,
                   failovers, restarts>>

GiveAgain(s) ==
    /\ cpc = "done" /\ map[s] = "old" /\ TakesFlip(s)
    /\ Take(s)
    /\ UNCHANGED <<primary, writer, has, aw, hold, exported, copied, cview, cmap, jpc, src,
                   cpc, firsts, k, took, rest, failovers, restarts>>

Next ==
    \/ Write \/ Hello \/ Adopt
    \/ \E s \in SA : Ship(s) \/ Fence(s) \/ Restart(s) \/ Expire(s)
    \/ \E q \in SA, z \in BOOLEAN : Failover(q, z)
    \/ Source \/ FirstExport \/ CatchUp \/ Hold \/ Renew \/ LastExport
    \/ Sync \/ GiveFirst
    \/ \E s \in All : GiveRest(s) \/ GiveAgain(s)

Spec == Init /\ [][Next]_vars

-----------------------------------------------------------------------------
TypeOK ==
    /\ primary \in SA /\ aw \in 0..MaxWrites /\ copied \in 0..MaxWrites
    /\ \A s \in All : map[s] \in {"old", "new"}

Flipped == \E s \in All : map[s] = "new"

(* Once any server holds the flip (and so the coordinators route the      *)
(* slots to B), B holds every write to them that A acknowledged, and A    *)
(* acknowledges no more.                                                  *)
NoLostWrite == Flipped => aw <= copied

(* Every server of the cluster can take the flip once it is made: none is  *)
(* left refusing it, and refusing the other maps after it, for good.       *)
NoStuckShard == ~ (cpc = "done" /\ map[primary] = "old" /\ ~ TakesFlip(primary))

=============================================================================
