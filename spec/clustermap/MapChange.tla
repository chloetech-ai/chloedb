------------------------------ MODULE MapChange ------------------------------
(***************************************************************************)
(* Changes of the cluster map (docs/chloedb/server.md, "Changing the       *)
(* cluster while it serves"), as chloedb-server/src/cores/slots.rs decides *)
(* them (Change: new, synced, give, give_next_first, landed, failed,       *)
(* again, answered, in_doubt, redone; given; take and flip_ok), as         *)
(* src/coord.rs drives them (Coordinator::change, change_held,             *)
(* sync_heard, install, watch, speak, connect_with's survey) and as        *)
(* Service::set_slots takes or refuses a map (src/service.rs).             *)
(*                                                                         *)
(* A map is the sequence of the changes that made it from the first map,   *)
(* <<>> (epoch 1): change k of coordinator c is the number 10 * c + k, and *)
(* a change made from map m is Append(m, it). So two maps of one epoch     *)
(* differ exactly when different changes, or the same change on different  *)
(* bases, made them. Each server keeps the map it holds across restarts    *)
(* (the slots file); a server takes a map of a newer epoch, holds the same *)
(* map again, and refuses an older one or another of its epoch (409).      *)
(*                                                                         *)
(* A change (Coordinator::change, one at a time on each coordinator):      *)
(* sync (sync_heard: the coordinator takes up the newest map of the        *)
(* servers that answer, atomically; the `heard` guard is then never        *)
(* needed, and is left out), synced: give the map in doubt again if it is  *)
(* newer than the map synced to, else make next = Append(now, change)      *)
(* (`f`, which may fail), and give: first to the servers of `first`, one   *)
(* at a time (FIRST_TRIES gives, each answer Took, Raced, Refused or       *)
(* Missed), then to the rest at once. A give is a call: either the server  *)
(* takes or refuses the map and answers, or the answer does not come (the  *)
(* call times out, Missed) and the call stays in flight: the server may    *)
(* take the map at any later time, or never. Landed, failed and again      *)
(* follow the code line by line; Done installs the map made, or the map    *)
(* in doubt given again and taken (redone), and keeps in_doubt.            *)
(*                                                                         *)
(* Every coordinator follows the servers' maps (watch, at any time) and    *)
(* gives the map it routes by to a server holding an older one (speak).    *)
(*                                                                         *)
(* Constants:                                                              *)
(*   Racing     both coordinators change the map (the simulator's          *)
(*              --racing), or only coordinator 1                           *)
(*   Forgetful  a coordinator keeps the map in doubt in memory, set when a *)
(*              change ends, and forgets it when it restarts (the server's *)
(*              coordinator, the simulator's --forgetful); otherwise it    *)
(*              also keeps each map as it gives it, and across restarts    *)
(*              (the simulator's default)                                  *)
(*   Lease      changes take the cluster's Lease (operator/lease.rs): one  *)
(*              change at a time in the cluster, and the Lease keeps the   *)
(*              map given, before it is given, and the map in doubt when   *)
(*              a change ends; a change starts from the newer of the       *)
(*              Lease's and its own. A Lease held by a coordinator that    *)
(*              restarts runs out                                          *)
(*   Survey     what a coordinator that restarts routes by: "kept" the map *)
(*              it routed by (the simulator); "all" the newest map of all  *)
(*              the servers (the server's survey, with shards of one       *)
(*              server, each of which must answer); "any" the newest map   *)
(*              of the servers that answer (connect_with's survey of the   *)
(*              copies of one shard, one of which must answer): here, the  *)
(*              map any one server holds                                   *)
(*   Move       slots copy from server Src in the first map, and           *)
(*              coordinator 1's first change is their flip: given to Src   *)
(*              first (the move job's `first`), and while the map synced   *)
(*              to copies, Src takes every change first (Change::give).    *)
(*              Src refuses a map with the flip while it holds a map       *)
(*              without, unless its hold covers its last export            *)
(*              (flip_ok; `covered`, lost once when the hold runs out or   *)
(*              Src restarts)                                              *)
(*   Quorum     a proposal: a change lands once a majority of the servers  *)
(*              took its map (else the map is in doubt); a sync, a         *)
(*              change's or a restart's survey, takes up the newest map    *)
(*              of a majority of the servers (else it waits); and a        *)
(*              change gives the map it synced to again, as a map in       *)
(*              doubt, until a majority holds it, before it makes its own  *)
(*   Bugs       the simulator's map bugs put back (Change's flags):        *)
(*              "forgets_doubt"  (gives-an-epoch-twice)                    *)
(*              "keeps_untaken"  (routes-by-a-map-no-server-took)          *)
(*              "skips_source"   (changes-around-the-source)               *)
(*              "forgets_redone" (forgets-a-map-given-again)               *)
(*                                                                         *)
(* Left out: the refusals of set_slots's row check (deterministic, and an  *)
(* arbitrary refusal would let a server refuse a map it takes later); a    *)
(* held_map call that fails after a refusal (a race read as Refused); the  *)
(* servers a map lists (every map names the same three); a pause longer    *)
(* than the Lease; and the move itself (spec/slots).                       *)
(***************************************************************************)
EXTENDS Naturals, Sequences, FiniteSets

CONSTANTS
    Servers,        \* the cluster's servers
    Src,            \* with Move, the server the slots copy from
    Racing, Forgetful, Lease, Move, Quorum,
    Survey,         \* "kept" | "all" | "any"
    Bugs,
    FirstTries,     \* FIRST_TRIES (3)
    ChangeTries,    \* CHANGE_TRIES (3)
    MaxChanges,     \* changes asked for, of both coordinators
    MaxRestarts     \* restarts of coordinators

ASSUME Survey \in {"kept", "all", "any"}
ASSUME Bugs \subseteq {"forgets_doubt", "keeps_untaken", "skips_source", "forgets_redone"}
ASSUME Src \in Servers /\ FirstTries >= 1 /\ ChangeTries >= 1

Coords == {1, 2}
Maj == Cardinality(Servers) \div 2 + 1
None == <<0>>                     \* no map (no change's number is 0)
Epoch(m) == Len(m) + 1
EpochOr0(m) == IF m = None THEN 0 ELSE Epoch(m)
Newer(a, b) == IF EpochOr0(a) > EpochOr0(b) THEN a ELSE b
Install(now, m) == IF Epoch(m) > Epoch(now) THEN m ELSE now   \* Coordinator::install
ID(c, k) == 10 * c + k
Flip == ID(1, 1)
HasFlip(m) == \E i \in 1..Len(m) : m[i] = Flip
Copying(m) == Move /\ ~ HasFlip(m)
IsFlip(c, k) == Move /\ ID(c, k) = Flip

VARIABLES
    held,       \* each server: the map it holds
    covered,    \* Move: Src's hold covers its last export
    msgs,       \* gives in flight: [to, m], whose answer did not come
    everHeld,   \* history: every map a server took
    cmap,       \* each coordinator: the map it routes by
    doubt,      \* each coordinator: its map in doubt (Coordinator::doubt), or None
    ch,         \* each coordinator: its change (core::Change), as a record
    lholder,    \* Lease: the coordinator holding it, or 0
    ldoubt,     \* Lease: the map it keeps in doubt, or None
    restarts

vars == <<held, covered, msgs, everHeld, cmap, doubt, ch, lholder, ldoubt, restarts>>

(* A coordinator that changes nothing. `n` counts its changes. *)
Idle(n) == [pc |-> "idle", n |-> n, next |-> None, cd |-> None, redoing |-> FALSE,
            base |-> None, took |-> 0, unans |-> FALSE, raced |-> FALSE,
            tries |-> 0, ftries |-> 0, waiting |-> {}]

Init ==
    /\ held = [s \in Servers |-> <<>>]
    /\ covered = TRUE
    /\ msgs = {}
    /\ everHeld = {<<>>}
    /\ cmap = [c \in Coords |-> <<>>]
    /\ doubt = [c \in Coords |-> None]
    /\ ch = [c \in Coords |-> Idle(0)]
    /\ lholder = 0 /\ ldoubt = None
    /\ restarts = 0

-----------------------------------------------------------------------------
(* A server given map m (Service::set_slots: flip_ok, then take). *)

Outcome(s, m) ==
    IF Move /\ s = Src /\ Copying(held[s]) /\ HasFlip(m) /\ Epoch(m) > Epoch(held[s]) /\ ~ covered
    THEN "refused"                                      \* flip_ok
    ELSE IF Epoch(m) < Epoch(held[s]) THEN "refused"    \* older than its own
    ELSE IF Epoch(m) = Epoch(held[s]) THEN (IF m = held[s] THEN "held" ELSE "refused")
    ELSE "took"

(* core::given, with the map the server says it holds after a refusal. *)
GivenAs(s, m) ==
    IF Outcome(s, m) \in {"took", "held"} THEN "Took"
    ELSE IF Epoch(held[s]) >= Epoch(m) /\ held[s] # m THEN "Raced" ELSE "Refused"

(* Calls in flight that can still change something. *)
Live(ms, h) == {x \in ms : Epoch(x.m) > Epoch(h[x.to])}

(* Server s processes map m; `extra` are calls left in flight meanwhile. *)
Serve(s, m, extra) ==
    /\ IF Outcome(s, m) = "took"
       THEN /\ held' = [held EXCEPT ![s] = m]
            /\ everHeld' = everHeld \cup {m}
       ELSE UNCHANGED <<held, everHeld>>
    /\ msgs' = Live(msgs \cup extra, held')

-----------------------------------------------------------------------------
(* Change's steps, as functions of its record x. A step's result: the     *)
(* record, whether the change ended ("no", "ok", "err"), the map made     *)
(* (for "ok") and the map it starts to give (or None).                    *)

R(x, end, made) == [x |-> x, end |-> end, made |-> made, give |-> None]

(* Change::landed *)
Landed(x) ==
    LET y == [x EXCEPT !.cd = None] IN
    IF x.redoing
    THEN R([y EXCEPT !.redoing = FALSE, !.base = x.next, !.pc = "sync"], "no", None)
    ELSE R(y, "ok", x.next)

(* Change::failed *)
Failed(x) ==
    IF x.took > 0 \/ x.unans THEN R([x EXCEPT !.cd = x.next], "err", None)
    ELSE IF x.redoing THEN R([x EXCEPT !.redoing = FALSE, !.cd = None, !.pc = "sync"], "no", None)
    ELSE R(x, "err", None)

(* Change::again *)
Again(x) ==
    IF x.tries + 1 >= ChangeTries THEN R([x EXCEPT !.tries = x.tries + 1], "err", None)
    ELSE R([x EXCEPT !.tries = x.tries + 1, !.pc = "sync"], "no", None)

(* Change::give and give_next_first(0): the first is Src, or none. *)
Give(x, c, m, now) ==
    LET first == Move /\ (IsFlip(c, x.n) \/ (Copying(now) /\ ~ ("skips_source" \in Bugs)))
    IN [x |-> [x EXCEPT !.next = m, !.took = 0, !.unans = FALSE, !.raced = FALSE,
                        !.pc = IF first THEN "first" ELSE "rest",
                        !.ftries = IF first THEN 1 ELSE 0,
                        !.waiting = IF first THEN {} ELSE Servers],
        end |-> "no", made |-> None, give |-> m]

(* Change::synced, the map synced to now0; `fails`: f refuses the map. *)
Synced(x, c, now0, fails) ==
    LET now == IF x.base # None /\ Epoch(x.base) > Epoch(now0) THEN x.base ELSE now0 IN
    IF x.cd # None /\ Epoch(x.cd) > Epoch(now)
    THEN Give([x EXCEPT !.redoing = TRUE], c, x.cd, now)
    ELSE LET y == [x EXCEPT !.cd = None] IN
         IF Quorum /\ now # x.base THEN Give([y EXCEPT !.redoing = TRUE], c, now, now)
         ELSE IF fails THEN R(y, "err", None)
         ELSE Give(y, c, Append(now, ID(c, x.n)), now)

(* Change::answered, Stage::First (the first is Src). *)
FirstAnswered(x, g) ==
    CASE g = "Took" -> R([x EXCEPT !.took = x.took + 1, !.pc = "rest", !.waiting = Servers \ {Src}], "no", None)
      [] g = "Raced" -> Again(x)
      [] g = "Refused" -> Failed(x)
      [] g = "Missed" -> IF x.ftries < FirstTries
                         THEN R([x EXCEPT !.unans = TRUE, !.ftries = x.ftries + 1], "no", None)
                         ELSE Failed([x EXCEPT !.unans = TRUE])

(* Whether the map given is made: a server took it (unless keeps_untaken), *)
(* or with Quorum a majority.                                              *)
Lands(y) == IF Quorum THEN 2 * y.took > Cardinality(Servers)
            ELSE y.took > 0 \/ "keeps_untaken" \in Bugs

(* Change::answered, Stage::Rest. *)
RestAnswered(x, s, g) ==
    LET y == [x EXCEPT !.waiting = x.waiting \ {s}, !.took = x.took + (IF g = "Took" THEN 1 ELSE 0),
                       !.raced = x.raced \/ g = "Raced", !.unans = x.unans \/ g = "Missed"]
    IN IF y.waiting # {} THEN R(y, "no", None)
       ELSE IF y.raced THEN Again([y EXCEPT !.raced = FALSE])
       ELSE IF Lands(y) THEN Landed(y)
       ELSE Failed(y)

(* Change::in_doubt *)
InDoubt(x) == IF "forgets_doubt" \in Bugs THEN None ELSE x.cd

(* ChangeStep::Done in change_held: install redone on an error, the map   *)
(* made on success.                                                       *)
DoneRoute(o, now) ==
    LET r1 == IF o.end = "err" /\ o.x.base # None /\ ~ ("forgets_redone" \in Bugs)
              THEN Install(now, o.x.base) ELSE now
    IN IF o.end = "ok" THEN Install(r1, o.made) ELSE r1

(* Coordinator c takes step o of its change, routing by `now` meanwhile. *)
Apply(c, o, now) ==
    /\ ch' = [ch EXCEPT ![c] = IF o.end = "no" THEN o.x ELSE Idle(o.x.n)]
    /\ cmap' = [cmap EXCEPT ![c] = IF o.end = "no" THEN now ELSE DoneRoute(o, now)]
    /\ doubt' = [doubt EXCEPT ![c] = IF o.end # "no" THEN InDoubt(o.x)
                                     ELSE IF o.give # None /\ ~ Forgetful THEN o.give
                                     ELSE @]
    /\ ldoubt' = IF ~ Lease THEN ldoubt
                 ELSE IF o.end # "no" THEN InDoubt(o.x)
                 ELSE IF o.give # None THEN o.give
                 ELSE ldoubt
    /\ lholder' = IF Lease /\ o.end # "no" THEN 0 ELSE lholder

-----------------------------------------------------------------------------
(* The coordinators *)

(* Coordinator::change: Change::new with the map in doubt. *)
Start(c) ==
    /\ ch[c].pc = "idle"
    /\ Racing \/ c = 1
    /\ ch[1].n + ch[2].n < MaxChanges
    /\ Lease => lholder = 0
    /\ ch' = [ch EXCEPT ![c] = [Idle(@.n + 1) EXCEPT !.pc = "sync",
                 !.cd = IF "forgets_doubt" \in Bugs THEN None
                        ELSE IF Lease THEN Newer(ldoubt, doubt[c]) ELSE doubt[c]]]
    /\ lholder' = IF Lease THEN c ELSE lholder
    /\ UNCHANGED <<held, covered, msgs, everHeld, cmap, doubt, ldoubt, restarts>>

(* The servers that may be the newest of those answering a sync: any (or *)
(* none), or with Quorum one newest of a majority.                         *)
NewestOfMajority == {s \in Servers : Cardinality({t \in Servers : Epoch(held[t]) <= Epoch(held[s])}) >= Maj}
SyncTo(c) == IF Quorum THEN {Install(cmap[c], held[s]) : s \in NewestOfMajority}
             ELSE {cmap[c]} \cup {held[s] : s \in {t \in Servers : Epoch(held[t]) > Epoch(cmap[c])}}

(* ChangeStep::Sync: sync_heard takes up the newest map of the servers    *)
(* that answer (maybe none), then Change::synced.                         *)
ChangeSync(c) ==
    /\ ch[c].pc = "sync"
    /\ \E now0 \in SyncTo(c), fails \in BOOLEAN :
          Apply(c, Synced(ch[c], c, now0, fails), now0)
    /\ UNCHANGED <<held, covered, msgs, everHeld, restarts>>

(* The first, Src, answers being given the map, or its answer does not    *)
(* come (a give in flight).                                               *)
FirstReply(c) ==
    /\ ch[c].pc = "first"
    /\ Serve(Src, ch[c].next, {})
    /\ Apply(c, FirstAnswered(ch[c], GivenAs(Src, ch[c].next)), cmap[c])
    /\ UNCHANGED <<covered, restarts>>

FirstMissed(c) ==
    /\ ch[c].pc = "first"
    /\ msgs' = Live(msgs \cup {[to |-> Src, m |-> ch[c].next]}, held)
    /\ Apply(c, FirstAnswered(ch[c], "Missed"), cmap[c])
    /\ UNCHANGED <<held, covered, everHeld, restarts>>

RestReply(c, s) ==
    /\ ch[c].pc = "rest" /\ s \in ch[c].waiting
    /\ Serve(s, ch[c].next, {})
    /\ Apply(c, RestAnswered(ch[c], s, GivenAs(s, ch[c].next)), cmap[c])
    /\ UNCHANGED <<covered, restarts>>

RestMissed(c, s) ==
    /\ ch[c].pc = "rest" /\ s \in ch[c].waiting
    /\ msgs' = Live(msgs \cup {[to |-> s, m |-> ch[c].next]}, held)
    /\ Apply(c, RestAnswered(ch[c], s, "Missed"), cmap[c])
    /\ UNCHANGED <<held, covered, everHeld, restarts>>

(* Coordinator::watch, and a Hello that answers a newer epoch: install. *)
Watch(c, s) ==
    /\ Epoch(held[s]) > Epoch(cmap[c])
    /\ cmap' = [cmap EXCEPT ![c] = held[s]]
    /\ UNCHANGED <<held, covered, msgs, everHeld, doubt, ch, lholder, ldoubt, restarts>>

(* Coordinator::speak: a server holding an older map is given the one the *)
(* coordinator routes by.                                                 *)
Speak(c, s) ==
    /\ Epoch(held[s]) < Epoch(cmap[c])
    /\ Outcome(s, cmap[c]) = "took"
    /\ Serve(s, cmap[c], {})
    /\ UNCHANGED <<covered, cmap, doubt, ch, lholder, ldoubt, restarts>>

(* A give whose answer did not come reaches its server late. *)
Deliver(x) ==
    /\ x \in msgs
    /\ Serve(x.to, x.m, {})
    /\ UNCHANGED <<covered, cmap, doubt, ch, lholder, ldoubt, restarts>>

(* A coordinator restarts: its change ends (its calls stay in flight), its *)
(* map in doubt is kept or forgotten, a Lease it held runs out, and it     *)
(* routes by the map Survey says.                                         *)
Restart(c) ==
    /\ restarts < MaxRestarts
    /\ restarts' = restarts + 1
    /\ LET awaited == CASE ch[c].pc = "first" -> {Src}
                        [] ch[c].pc = "rest" -> ch[c].waiting
                        [] OTHER -> {}
       IN msgs' = Live(msgs \cup {[to |-> s, m |-> ch[c].next] : s \in awaited}, held)
    /\ ch' = [ch EXCEPT ![c] = Idle(@.n)]
    /\ doubt' = IF Forgetful THEN [doubt EXCEPT ![c] = None] ELSE doubt
    /\ lholder' = IF lholder = c THEN 0 ELSE lholder
    /\ LET top == {s \in Servers : \A t \in Servers : Epoch(held[t]) <= Epoch(held[s])}
       IN CASE Survey = "kept" -> UNCHANGED cmap
            [] Survey = "all" -> \E s \in top : cmap' = [cmap EXCEPT ![c] = held[s]]
            [] Survey = "any" -> \E s \in (IF Quorum THEN NewestOfMajority ELSE Servers) :
                                    cmap' = [cmap EXCEPT ![c] = held[s]]
    /\ UNCHANGED <<held, covered, everHeld, ldoubt>>

(* Src's hold runs out (or Src restarts): it no longer covers the export. *)
Unhold ==
    /\ Move /\ covered
    /\ covered' = FALSE
    /\ UNCHANGED <<held, msgs, everHeld, cmap, doubt, ch, lholder, ldoubt, restarts>>

Next ==
    \/ \E c \in Coords : Start(c) \/ ChangeSync(c) \/ FirstReply(c) \/ FirstMissed(c) \/ Restart(c)
    \/ \E c \in Coords, s \in Servers : RestReply(c, s) \/ RestMissed(c, s) \/ Watch(c, s) \/ Speak(c, s)
    \/ \E x \in msgs : Deliver(x)
    \/ Unhold

Spec == Init /\ [][Next]_vars

-----------------------------------------------------------------------------
TypeOK ==
    /\ \A s \in Servers : held[s] \in Seq(Nat)
    /\ \A c \in Coords : ch[c].pc \in {"idle", "sync", "first", "rest"}
    /\ restarts <= MaxRestarts

(* No fork: no two servers ever took different maps of one epoch. *)
NoFork == \A m1, m2 \in everHeld : Len(m1) = Len(m2) => m1 = m2

(* A coordinator routes only by a map some server holds or held. *)
RoutesHeld == \A c \in Coords : cmap[c] \in everHeld

(* No server is left refusing every later map: Src, holding a map without *)
(* the flip and no longer covered, refuses every map made from the flip,  *)
(* which another server holds.                                            *)
NoStuck == ~ (Move /\ ~ covered /\ Copying(held[Src]) /\ \E s \in Servers : HasFlip(held[s]))

=============================================================================
