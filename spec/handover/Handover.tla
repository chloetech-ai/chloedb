------------------------------ MODULE Handover ------------------------------
(***************************************************************************)
(* A shard's primary handed over by hand, and the primary's lease on       *)
(* reads, as chloedb-server/src/cores/shard.rs decides each step and       *)
(* src/replication.rs runs them:                                           *)
(*                                                                         *)
(*  - a promotion by hand (Follow::promote): the copies' Hello             *)
(*    (promote_check, Asked.primaries), a handover pre-vote that stops a   *)
(*    serving primary taking writes (Primary::answer, hand_over,           *)
(*    answered_handover, HANDOVER_PAUSE), the candidate's own vote         *)
(*    (Campaign::vote_self), the votes of the copies that said they serve  *)
(*    as primary asked first while the candidate answers no call           *)
(*    (Follow::campaign's `first` and its `one` lock), then the others',   *)
(*    and the wait before the winner takes writes (handover_hold,          *)
(*    Primary::writes_from, admit);                                        *)
(*  - automatic failover (--failover-after-ms, service/failover.rs): a     *)
(*    replica silent for the delay asks to be elected (Failover::due,      *)
(*    Replica::silent_for), a replica refuses a vote that is no handover   *)
(*    while its primary called within the delay (Replica::vote), a         *)
(*    serving primary refuses all but a handover's (Primary::vote), a      *)
(*    replaced primary steps down (Service::step_down);                    *)
(*  - the lease (failover::tuned: three quarters of the delay): a primary  *)
(*    serves reads while a majority of its copies answered a call started  *)
(*    within it (Primary::readable, Peer::answered); without failover the  *)
(*    lease is zero and so is the hold;                                    *)
(*  - shipping, fencing and acknowledgment as spec/replication has them    *)
(*    (Replica::take, Primary::shipped_to, fence, holding, term_start).    *)
(*                                                                         *)
(* Time is a discrete clock (Tick). What the code measures is kept as      *)
(* ages, each capped at the one bound it is compared with, so the state    *)
(* space is finite: for each primary and replica, the ticks since the      *)
(* start of the last call the replica answered (the lease); for each       *)
(* replica, the ticks since its primary called it or it started (the       *)
(* sticky refusal), and since then or its last vote (silent_for); for each *)
(* primary, the ticks before it takes writes (handover_hold) and before    *)
(* its handover pause runs out. Clocks run apart by up to Drift ticks on   *)
(* any duration, and each bound is taken at its worst: a lease lasts       *)
(* Lease + Drift, a delay, a hold or a pause lasts its length less Drift.  *)
(* Every other choice of drift lets fewer reads, votes or writes happen,   *)
(* so these behaviours include all of them: no error here is no error for  *)
(* any drift up to Drift. (A trace found may ask one copy's clock to be    *)
(* fast and slow at once; the traces in the README were checked not to.)   *)
(*                                                                         *)
(* Abstracted, and why safety still holds:                                 *)
(*  - A call is one atomic step that may or may not happen (a call lost is *)
(*    a step not taken); a Replicate's answer may be lost on its way back. *)
(*    Taking a call the moment it starts is the worst case for the lease:  *)
(*    the replica's silence (and so its next vote) starts as early as it   *)
(*    can, and the answer counts from that start as the code counts it.    *)
(*  - A handover waits for every copy's answer or its call_timeout; here   *)
(*    it may win or give up once its answers allow, the rest lost.         *)
(*    Follow::promote's catch_up round is left out (it only stops writes,  *)
(*    which the campaign's own pre-vote does again).                       *)
(*  - What only an unfenced primary reads (its copies' answers and         *)
(*    acknowledgments, its hold and pause) is forgotten once it is fenced, *)
(*    and a campaign's answers once past their round: states that behave   *)
(*    alike are one.                                                       *)
(*  - A write is admitted and applied in one step (no write in flight      *)
(*    across a pause); admit's check that a majority is in sync, the       *)
(*    failover jitter (Failover::next) and the terms copies told a         *)
(*    candidate (Replica::heard: any newer term is asked for) are left     *)
(*    out: they only refuse or delay.                                      *)
(*  - The membership, lost volumes and first copies are spec/replication's *)
(*    (every copy is a member here).                                       *)
(*  - A read is not a step: LeaseReadsFresh checks, in every state, that a *)
(*    read a primary would serve then holds every client write             *)
(*    acknowledged so far. Replicas promise no lease: they serve what they *)
(*    hold while their primary's last call found them caught up, and a     *)
(*    read that must see a write names it (docs/chloedb/server.md, "Reads  *)
(*    on a replica"), so they are not checked.                             *)
(*                                                                         *)
(* Bug puts a fixed bug back:                                              *)
(*   "takes-writes-in-a-lease"  a replica promoted by hand takes writes at *)
(*       once (the simulator's --bug takes-writes-in-a-lease)              *)
(*   "asks-every-copy-at-once"  a handover asks every copy for its vote at *)
(*       once and answers its primary's calls meanwhile, as before         *)
(*       Follow::campaign took `first`                                     *)
(* HoldRule "all-granted" and RestartFix are fixes proposed here (README). *)
(* Prompt: nothing a handover sends is lost (every copy answers its Hello, *)
(* its pre-vote and its vote) and no tick passes from its pre-vote until   *)
(* the primaries voted, as when nothing stalls. Overlap FALSE: one         *)
(* election at a time (the next starts once the last won or gave up).      *)
(***************************************************************************)
EXTENDS Naturals, Sequences, FiniteSets, TLC

CONSTANTS
    Copies,      \* the shard's copies
    First,       \* the copy started as the shard's primary
    Auto,        \* TRUE: --failover-after-ms; FALSE: failover by hand only
    ByHand,      \* TRUE: an operator promotes replicas (POST /v1/admin/promote)
    Campaigns,   \* TRUE: with Auto, replicas ask to be elected on their own, as the
                 \* code does; FALSE leaves that out to reach a trace in fewer states
                 \* (an error found is the code's; no error proves nothing of them)
    After,       \* the failover delay, in ticks
    Drift,       \* how far a copy's measure of a duration may be off, in ticks
    Pause,       \* HANDOVER_PAUSE, in ticks
    Bug,         \* "none" | "takes-writes-in-a-lease" | "asks-every-copy-at-once"
    HoldRule,    \* "primaries": handover_hold; "all-granted": no hold only when every copy voted
    RestartFix,  \* FALSE: Replica::vote refuses only once its primary called since it
                 \* started; TRUE: for the delay from its start too
    Prompt,      \* see above
    Overlap,     \* TRUE: two copies' elections may be under way at once
    MaxTerm,     \* terms asked for stay at or below this
    MaxWrites,   \* clients' writes, in all
    MaxRestarts  \* copies crashing and starting again, in all

ASSUME Bug \in {"none", "takes-writes-in-a-lease", "asks-every-copy-at-once"}
ASSUME HoldRule \in {"primaries", "all-granted"}
ASSUME First \in Copies /\ Drift \in Nat /\ After > Drift

Nil == [none |-> TRUE]
Maj == Cardinality(Copies) \div 2 + 1
Max(a, b) == IF a >= b THEN a ELSE b
Min(a, b) == IF a <= b THEN a ELSE b
Less(a, b) == IF a > b THEN a - b ELSE 0

(* failover::tuned: the lease is three quarters of the delay (zero without *)
(* failover); handover_hold: the lease and a third more (zero with none).  *)
Lease == IF Auto THEN (After * 3) \div 4 ELSE 0
Hold  == IF Lease = 0 THEN 0 ELSE Lease + Lease \div 3
(* The same, in true ticks, at their worst (above). *)
LeaseLong  == IF Lease = 0 THEN 0 ELSE Lease + Drift
AfterShort == After - Drift
HoldShort  == Less(Hold, Drift)
PauseShort == Less(Pause, Drift)

VARIABLES
    log,        \* log[c]: the terms of the transactions c holds, in order
    dbTerm,     \* the database's term
    role,       \* "primary" | "replica"
    followsT,   \* the term of the primary it follows (Kept.follows; 0: none)
    replacedBy, \* the newer term that fenced it as a primary (Kept.replaced_by, Primary::fenced)
    voted,      \* [t, c] or Nil (Kept.voted)
    acked,      \* acked[p][r]: what primary p last heard r holds (Peer.acked)
    termStart,  \* termStart[p]: its term's first transaction (0: none)
    ans,        \* ans[p][r]: ticks since the start of the last call of p that r
                \* answered (Peer.answered), capped at LeaseLong: not within the lease
    heard,      \* heard[r]: ticks since its primary called or it started, capped
    called,     \* called[r]: a primary called it since it started (Replica.called_at)
    silent,     \* silent[r]: ticks since then or since it granted a vote (silent_for)
    holdW,      \* holdW[p]: ticks before it takes writes (Primary.writes_from)
    pause,      \* pause[p]: ticks before its handover pause runs out (Primary.handing_over)
    campaign,   \* campaign[c]: an election of c under way, or Nil
    committed,  \* history: <<number, term, term of the primary that answered>>
    clientW,    \* history: <<number, term>> of each client's write (not term starts)
    began,      \* history: <<term, copy>> of each term begun
    lateAck,    \* history: a primary acknowledged a write once a newer term had begun
    refusedKnowing, \* history: <<t, p>>: primary p asked first refused a handover's
                    \* vote of term t, knowing term t already
    writes, restarts

vars == <<log, dbTerm, role, followsT, replacedBy, voted, acked, termStart, ans, heard,
          called, silent, holdW, pause, campaign, committed, clientW, began, lateAck,
          refusedKnowing, writes, restarts>>

-----------------------------------------------------------------------------
(* What a copy knows and holds.                                            *)

VotedT(c) == IF voted[c] = Nil THEN 0 ELSE voted[c].t

(* Kept::newest *)
Newest(c) == Max(Max(dbTerm[c], followsT[c]), Max(replacedBy[c], VotedT(c)))

LastTerm(c) == IF log[c] = <<>> THEN 0 ELSE log[c][Len(log[c])]
Last(c) == <<LastTerm(c), Len(log[c])>>
GEQ(a, b) == a[1] > b[1] \/ (a[1] = b[1] /\ a[2] >= b[2])
GT(a, b) == ~ GEQ(b, a)
(* promote_check: a copy that holds no transaction is taken at its term. *)
HelloLast(c) == IF log[c] = <<>> THEN <<dbTerm[c], 0>> ELSE Last(c)

LCP(a, b) ==
    LET n == IF Len(a) < Len(b) THEN Len(a) ELSE Len(b)
        same == {i \in 0..n : \A j \in 1..i : a[j] = b[j]}
    IN CHOOSE i \in same : \A k \in same : k <= i

(* Primary::holding *)
CommitIndex(p, a) ==
    LET H(j) == Cardinality({p} \cup {q \in Copies \ {p} : a[q] >= j})
        ok   == {j \in 0..Len(log[p]) : j = 0 \/ H(Max(j, termStart[p])) >= Maj}
    IN CHOOSE j \in ok : \A x \in ok : x <= j

Holds(c, e) == Len(log[c]) >= e[1] /\ log[c][e[1]] = e[2]

(* A vote request (Campaign::ask). *)
Req(c, t, hand) == [c |-> c, t |-> t, last |-> Last(c), hand |-> hand]

(* decide, for copies that are all members of one membership. *)
Decide(v, q, pre) ==
    /\ \/ ~ pre /\ voted[v] = [t |-> q.t, c |-> q.c]
       \/ q.t > Newest(v)
    /\ GEQ(q.last, Last(v))

(* Replica::vote: refused, but for a handover, while its primary called    *)
(* within the delay (and only once one called since it started, but with   *)
(* RestartFix).                                                            *)
Sticky(v, q) ==
    /\ ~ q.hand /\ role[v] = "replica"
    /\ called[v] \/ RestartFix
    /\ heard[v] < AfterShort

(* Primary::vote: a primary that serves grants only a handover. *)
ServingRefuses(v, q) == role[v] = "primary" /\ replacedBy[v] = 0 /\ ~ q.hand

WouldGrant(v, q, pre) == ~ Sticky(v, q) /\ ~ ServingRefuses(v, q) /\ Decide(v, q, pre)

(* Campaign::pre_voted for an election on its own: a majority would grant *)
(* it, none of them knowing its term; copies that do not answer are left   *)
(* out.                                                                    *)
PreVoted(c, t) ==
    Cardinality({c} \cup {v \in Copies \ {c} :
                    t > Newest(v) /\ WouldGrant(v, Req(c, t, FALSE), TRUE)}) >= Maj

(* Primary::readable *)
Readable(p) ==
    /\ role[p] = "primary" /\ replacedBy[p] = 0
    /\ Lease = 0 \/ Cardinality({p} \cup {r \in Copies \ {p} : ans[p][r] < LeaseLong}) >= Maj

(* Primary::admit, but for the copies in sync (it only refuses). *)
Writer(p) == role[p] = "primary" /\ replacedBy[p] = 0 /\ holdW[p] = 0 /\ pause[p] = 0

(* A candidate asking the primaries first holds its `one` lock: it takes   *)
(* no call of a primary.                                                    *)
Locked(r) == campaign[r] # Nil /\ campaign[r].phase = "first"

NoAges == [r \in Copies |-> LeaseLong]
Zeros  == [r \in Copies |-> 0]

NewCampaign(t, hand, phase, req) ==
    [t |-> t, hand |-> hand, phase |-> phase, req |-> req, granted |-> {}, answered |-> {},
     prim |-> {}, seen |-> {}, pre |-> {}, preG |-> {}, newer |-> 0]

-----------------------------------------------------------------------------
Init ==
    /\ log        = [c \in Copies |-> <<>>]
    /\ dbTerm     = [c \in Copies |-> 1]
    /\ role       = [c \in Copies |-> IF c = First THEN "primary" ELSE "replica"]
    /\ followsT   = [c \in Copies |-> 0]
    /\ replacedBy = [c \in Copies |-> 0]
    /\ voted      = [c \in Copies |-> Nil]
    /\ acked      = [p \in Copies |-> Zeros]
    /\ termStart  = [c \in Copies |-> 0]
    /\ ans        = [p \in Copies |-> NoAges]
    /\ heard      = [c \in Copies |-> 0]
    /\ called     = [c \in Copies |-> FALSE]
    /\ silent     = [c \in Copies |-> 0]
    /\ holdW      = [c \in Copies |-> 0]
    /\ pause      = [c \in Copies |-> 0]
    /\ campaign   = [c \in Copies |-> Nil]
    /\ committed  = {}
    /\ clientW    = {}
    /\ began      = {<<1, First>>}
    /\ lateAck    = FALSE
    /\ refusedKnowing = {}
    /\ writes = 0 /\ restarts = 0

(* The clock moves on. With Prompt, not from a handover's pre-vote until  *)
(* the primaries voted.                                                    *)
Busy == \E c \in Copies :
          /\ campaign[c] # Nil /\ campaign[c].hand
          /\ \/ campaign[c].phase = "pre"
             \/ campaign[c].phase \in {"first", "rest"} /\ ~ campaign[c].prim \subseteq campaign[c].answered

Tick ==
    /\ Prompt => ~ Busy
    /\ ans'    = [p \in Copies |-> [r \in Copies |-> Min(ans[p][r] + 1, LeaseLong)]]
    \* (only automatic elections read a replica's silence)
    /\ heard'  = [c \in Copies |-> IF Auto /\ Campaigns /\ role[c] = "replica" THEN Min(heard[c] + 1, AfterShort) ELSE heard[c]]
    /\ silent' = [c \in Copies |-> IF Auto /\ Campaigns /\ role[c] = "replica" THEN Min(silent[c] + 1, AfterShort) ELSE silent[c]]
    /\ holdW'  = [c \in Copies |-> Less(holdW[c], 1)]
    /\ pause'  = [c \in Copies |-> Less(pause[c], 1)]
    /\ UNCHANGED <<log, dbTerm, role, followsT, replacedBy, voted, acked, termStart, called,
                   campaign, committed, clientW, began, lateAck, refusedKnowing, writes, restarts>>

(* A client's write, which a primary that admits it commits to its log. *)
Write(p) ==
    /\ Writer(p) /\ writes < MaxWrites
    /\ log' = [log EXCEPT ![p] = Append(@, dbTerm[p])]
    /\ clientW' = clientW \cup {<<Len(log[p]) + 1, dbTerm[p]>>}
    /\ writes' = writes + 1
    /\ UNCHANGED <<dbTerm, role, followsT, replacedBy, voted, acked, termStart, ans, heard,
                   called, silent, holdW, pause, campaign, committed, began, lateAck,
                   refusedKnowing, restarts>>

(* Primary p calls replica r (Replicate), which takes its log up to k      *)
(* (Replica::take, applied) and hears from a primary (took: called_at).    *)
(* Its answer may be lost; answered, it counts toward p's lease from the   *)
(* call's start (Primary::answered) and acknowledges what a majority holds.*)
(* A fenced primary still ships, and acknowledges nothing (Primary::acked).*)
Ship(p, r) ==
    /\ role[p] = "primary" /\ role[r] = "replica" /\ p # r
    /\ ~ Locked(r)
    /\ dbTerm[p] >= Newest(r)
    /\ \E k \in 0..Len(log[p]), answered \in BOOLEAN :
         LET pt  == dbTerm[p]
             n   == Max(k, LCP(log[r], log[p]))
             row == [acked[p] EXCEPT ![r] = n]
             new == {<<i, log[p][i], pt>> : i \in 1..CommitIndex(p, row)} \ committed
         IN
         /\ log'        = [log EXCEPT ![r] = IF n > LCP(log[r], log[p]) THEN SubSeq(log[p], 1, n) ELSE @]
         /\ dbTerm'     = [dbTerm EXCEPT ![r] = Max(@, pt)]
         /\ followsT'   = [followsT EXCEPT ![r] = pt]
         /\ replacedBy' = [replacedBy EXCEPT ![r] = 0]
         /\ heard'      = [heard EXCEPT ![r] = 0]
         /\ silent'     = [silent EXCEPT ![r] = 0]
         /\ called'     = [called EXCEPT ![r] = ~ RestartFix]
         /\ IF answered /\ replacedBy[p] = 0
            THEN /\ acked'     = [acked EXCEPT ![p] = row]
                 /\ ans'       = [ans EXCEPT ![p][r] = 0]
                 /\ committed' = committed \cup new
                 /\ lateAck'   = (lateAck \/ (new # {} /\ \E b \in began : b[1] > pt))
            ELSE UNCHANGED <<acked, ans, committed, lateAck>>
         /\ UNCHANGED <<role, voted, termStart, holdW, pause, campaign, clientW, began,
                        refusedKnowing, writes, restarts>>

(* p is fenced by term t (Primary::fence): it serves no reads and takes no *)
(* writes from then, so what only those read (its copies' answers and      *)
(* acknowledgments, its hold and pause) is forgotten, which merges states  *)
(* that behave alike.                                                      *)
Fence(p, t) ==
    /\ replacedBy' = [replacedBy EXCEPT ![p] = t]
    /\ ans'   = [ans EXCEPT ![p] = NoAges]
    /\ acked' = [acked EXCEPT ![p] = Zeros]
    /\ holdW' = [holdW EXCEPT ![p] = 0]
    /\ pause' = [pause EXCEPT ![p] = 0]

(* Replica r knows a newer term than p's: it answers with it (take), and p *)
(* is fenced (Primary::outcome, fence): no writes, no reads.               *)
ShipFenced(p, r) ==
    /\ role[p] = "primary" /\ role[r] = "replica" /\ p # r
    /\ ~ Locked(r)
    /\ dbTerm[p] < Newest(r) /\ replacedBy[p] < Newest(r)
    /\ Fence(p, Newest(r))
    /\ UNCHANGED <<log, dbTerm, role, followsT, voted, termStart, heard, called,
                   silent, campaign, committed, clientW, began, lateAck,
                   refusedKnowing, writes, restarts>>

(* p ships to q, a primary too (Primary::shipped_to): the older is fenced. *)
ShipPrimary(p, q) ==
    /\ role[p] = "primary" /\ role[q] = "primary" /\ p # q
    /\ \/ dbTerm[p] < dbTerm[q] /\ replacedBy[p] < dbTerm[q] /\ Fence(p, dbTerm[q])
       \/ dbTerm[p] > dbTerm[q] /\ replacedBy[q] < dbTerm[p] /\ Fence(q, dbTerm[p])
    /\ UNCHANGED <<log, dbTerm, role, followsT, voted, termStart, heard, called,
                   silent, campaign, committed, clientW, began, lateAck,
                   refusedKnowing, writes, restarts>>

(* With failover, a replaced primary becomes a replica (Service::step_down,*)
(* Follow::open: it has heard from no primary since it started).           *)
StepDown(p) ==
    /\ Auto /\ role[p] = "primary" /\ replacedBy[p] > 0
    /\ role'      = [role EXCEPT ![p] = "replica"]
    /\ followsT'  = [followsT EXCEPT ![p] = 0]
    /\ heard'     = [heard EXCEPT ![p] = 0]
    /\ silent'    = [silent EXCEPT ![p] = 0]
    /\ called'    = [called EXCEPT ![p] = FALSE]
    /\ ans'       = [ans EXCEPT ![p] = NoAges]
    /\ acked'     = [acked EXCEPT ![p] = Zeros]
    /\ termStart' = [termStart EXCEPT ![p] = 0]
    /\ holdW'     = [holdW EXCEPT ![p] = 0]
    /\ pause'     = [pause EXCEPT ![p] = 0]
    /\ UNCHANGED <<log, dbTerm, replacedBy, voted, campaign, committed, clientW, began,
                   lateAck, refusedKnowing, writes, restarts>>

(* Copy c crashes and starts again (shard::restart): with failover, or     *)
(* replaced, as a replica that heard from no primary since it started;     *)
(* otherwise a primary starts again as one, its replicas not heard from    *)
(* (Primary::new). What it was doing is gone; what it keeps is kept.       *)
Restart(c) ==
    /\ restarts < MaxRestarts
    /\ restarts' = restarts + 1
    /\ ans'       = [ans EXCEPT ![c] = NoAges]
    /\ acked'     = [acked EXCEPT ![c] = Zeros]
    /\ termStart' = [termStart EXCEPT ![c] = 0]
    /\ holdW'     = [holdW EXCEPT ![c] = 0]
    /\ pause'     = [pause EXCEPT ![c] = 0]
    /\ campaign'  = [campaign EXCEPT ![c] = Nil]
    /\ IF role[c] = "primary" /\ replacedBy[c] = 0 /\ ~ Auto
       THEN UNCHANGED <<role, followsT, heard, silent, called>>
       ELSE /\ role'     = [role EXCEPT ![c] = "replica"]
            /\ followsT' = [followsT EXCEPT ![c] = IF role[c] = "primary" THEN 0 ELSE @]
            /\ heard'    = [heard EXCEPT ![c] = 0]
            /\ silent'   = [silent EXCEPT ![c] = 0]
            /\ called'   = [called EXCEPT ![c] = FALSE]
    /\ UNCHANGED <<log, dbTerm, replacedBy, voted, committed, clientW, began, lateAck,
                   refusedKnowing, writes>>

-----------------------------------------------------------------------------
(* Elections.                                                              *)

(* A replica silent for the delay asks to be elected (Failover::due),     *)
(* after the pre-vote, keeping its own vote first.                         *)
StartAuto(c, t) ==
    /\ Auto /\ Campaigns /\ role[c] = "replica" /\ campaign[c] = Nil
    /\ Overlap \/ \A d \in Copies : campaign[d] = Nil
    /\ silent[c] >= AfterShort
    /\ t > Newest(c) /\ t <= MaxTerm
    /\ PreVoted(c, t)
    /\ voted'    = [voted EXCEPT ![c] = [t |-> t, c |-> c]]
    /\ campaign' = [campaign EXCEPT ![c] = NewCampaign(t, FALSE, "rest", Req(c, t, FALSE))]
    /\ UNCHANGED <<log, dbTerm, role, followsT, replacedBy, acked, termStart, ans, heard,
                   called, silent, holdW, pause, committed, clientW, began, lateAck,
                   refusedKnowing, writes, restarts>>

(* An operator promotes replica c (Service::promote, Follow::promote). *)
StartHandover(c) ==
    /\ ByHand /\ role[c] = "replica" /\ campaign[c] = Nil
    /\ Overlap \/ \A d \in Copies : campaign[d] = Nil
    /\ campaign' = [campaign EXCEPT ![c] = NewCampaign(0, TRUE, "hello", Nil)]
    /\ UNCHANGED <<log, dbTerm, role, followsT, replacedBy, voted, acked, termStart, ans,
                   heard, called, silent, holdW, pause, committed, clientW, began, lateAck,
                   refusedKnowing, writes, restarts>>

(* Copy q answers its Hello (promote_check): one holding a later           *)
(* transaction refuses the promotion; one that serves as primary, not      *)
(* replaced, is among Asked.primaries.                                     *)
Hello(c, q) ==
    /\ campaign[c] # Nil /\ campaign[c].phase = "hello"
    /\ q # c /\ q \notin campaign[c].seen
    /\ LET cc == campaign[c] IN
       campaign' = [campaign EXCEPT ![c] =
           IF GT(HelloLast(q), HelloLast(c)) THEN Nil
           ELSE [cc EXCEPT !.seen = cc.seen \cup {q},
                           !.prim = IF role[q] = "primary" /\ replacedBy[q] = 0
                                    THEN cc.prim \cup {q} ELSE cc.prim]]
    /\ UNCHANGED <<log, dbTerm, role, followsT, replacedBy, voted, acked, termStart, ans,
                   heard, called, silent, holdW, pause, committed, clientW, began, lateAck,
                   refusedKnowing, writes, restarts>>

(* The rest did not answer: Follow::campaign asks for term t. With Prompt,*)
(* every copy answered.                                                    *)
HelloDone(c, t) ==
    /\ campaign[c] # Nil /\ campaign[c].phase = "hello"
    /\ Prompt => campaign[c].seen = Copies \ {c}
    /\ t > Newest(c) /\ t <= MaxTerm
    /\ campaign' = [campaign EXCEPT ![c].phase = "pre", ![c].t = t, ![c].req = Req(c, t, TRUE),
                                    ![c].seen = {}]
    /\ UNCHANGED <<log, dbTerm, role, followsT, replacedBy, voted, acked, termStart, ans,
                   heard, called, silent, holdW, pause, committed, clientW, began, lateAck,
                   refusedKnowing, writes, restarts>>

(* Copy v answers the handover's pre-vote. A primary that serves stops     *)
(* taking writes for the pause (hand_over), and goes on stopped when it    *)
(* would grant, or refuses a copy only behind it (answered_handover).      *)
PreAnswer(c, v) ==
    /\ campaign[c] # Nil /\ campaign[c].phase = "pre"
    /\ v # c /\ v \notin campaign[c].pre
    /\ LET cc      == campaign[c]
           g       == Decide(v, cc.req, TRUE)
           behind  == GT(Last(v), cc.req.last)
           serving == role[v] = "primary" /\ replacedBy[v] = 0
       IN
       /\ campaign' = [campaign EXCEPT ![c] = [cc EXCEPT !.pre = cc.pre \cup {v},
                                                    !.preG = IF g THEN cc.preG \cup {v} ELSE cc.preG,
                                                    !.newer = Max(cc.newer, Newest(v))]]
       /\ pause' = IF serving THEN [pause EXCEPT ![v] = IF g \/ behind THEN PauseShort ELSE 0]
                   ELSE pause
    /\ UNCHANGED <<log, dbTerm, role, followsT, replacedBy, voted, acked, termStart, ans,
                   heard, called, silent, holdW, committed, clientW, began, lateAck,
                   refusedKnowing, writes, restarts>>

(* Campaign::pre_voted, then vote_self: the candidate keeps its vote, and *)
(* asks the primaries first, holding its lock (or every copy at once,     *)
(* with the bug).                                                          *)
PreDone(c) ==
    /\ campaign[c] # Nil /\ campaign[c].phase = "pre"
    /\ Prompt => campaign[c].pre = Copies \ {c}
    /\ LET cc == campaign[c] IN
       IF cc.newer < cc.t /\ Cardinality(cc.preG) + 1 >= Maj /\ Newest(c) < cc.t
       THEN /\ voted' = [voted EXCEPT ![c] = [t |-> cc.t, c |-> c]]
            /\ campaign' = [campaign EXCEPT ![c] =
                 [cc EXCEPT !.phase = IF Bug = "asks-every-copy-at-once" THEN "rest" ELSE "first",
                            !.req = Req(c, cc.t, TRUE), !.pre = {}, !.preG = {}, !.newer = 0]]
       ELSE /\ campaign' = [campaign EXCEPT ![c] = Nil]
            /\ UNCHANGED voted
    /\ UNCHANGED <<log, dbTerm, role, followsT, replacedBy, acked, termStart, ans, heard,
                   called, silent, holdW, pause, committed, clientW, began, lateAck,
                   refusedKnowing, writes, restarts>>

(* Copy v answers c's vote (Replica::vote, Primary::answer). A replica     *)
(* that grants waits a whole delay before it asks itself (granted). A      *)
(* primary that grants steps down at once (gave_vote, Service::vote); one  *)
(* that serves and refuses a handover takes writes again.                  *)
Ask(v, c) ==
    /\ campaign[c] # Nil /\ campaign[c].phase \in {"first", "rest"}
    /\ v # c /\ v \notin campaign[c].answered
    /\ LET cc      == campaign[c]
           t       == cc.t
           g       == WouldGrant(v, cc.req, FALSE)
           serving == role[v] = "primary" /\ replacedBy[v] = 0
       IN
       /\ cc.phase = "first" => v \in cc.prim
       /\ (cc.phase = "rest" /\ cc.hand /\ Bug # "asks-every-copy-at-once") => v \notin cc.prim
       /\ campaign' = [campaign EXCEPT ![c] = [cc EXCEPT !.answered = cc.answered \cup {v},
                                                    !.granted = IF g THEN cc.granted \cup {v} ELSE cc.granted]]
       /\ refusedKnowing' = IF ~ g /\ v \in cc.prim /\ Newest(v) >= t
                            THEN refusedKnowing \cup {<<t, v>>} ELSE refusedKnowing
       /\ IF g
          THEN /\ voted' = [voted EXCEPT ![v] = [t |-> t, c |-> c]]
               /\ IF role[v] = "primary"
                  THEN /\ role'       = [role EXCEPT ![v] = "replica"]
                       /\ replacedBy' = [replacedBy EXCEPT ![v] = Max(@, t)]
                       /\ followsT'   = [followsT EXCEPT ![v] = 0]
                       /\ heard'      = [heard EXCEPT ![v] = 0]
                       /\ silent'     = [silent EXCEPT ![v] = 0]
                       /\ called'     = [called EXCEPT ![v] = FALSE]
                       /\ ans'        = [ans EXCEPT ![v] = NoAges]
                       /\ acked'      = [acked EXCEPT ![v] = Zeros]
                       /\ termStart'  = [termStart EXCEPT ![v] = 0]
                       /\ holdW'      = [holdW EXCEPT ![v] = 0]
                       /\ pause'      = [pause EXCEPT ![v] = 0]
                  ELSE /\ silent' = [silent EXCEPT ![v] = 0]
                       /\ UNCHANGED <<role, replacedBy, followsT, heard, called, ans, acked,
                                      termStart, holdW, pause>>
          ELSE /\ pause' = IF serving /\ cc.hand THEN [pause EXCEPT ![v] = 0] ELSE pause
               /\ UNCHANGED <<voted, role, replacedBy, followsT, heard, silent, called, ans,
                              acked, termStart, holdW>>
    /\ UNCHANGED <<log, dbTerm, committed, clientW, began, lateAck, writes, restarts>>

(* The primaries answered, or their calls failed: the candidate lets go of *)
(* its lock and asks the others. With Prompt, every primary answered.      *)
FirstDone(c) ==
    /\ campaign[c] # Nil /\ campaign[c].phase = "first"
    /\ Prompt => campaign[c].prim \subseteq campaign[c].answered
    /\ campaign' = [campaign EXCEPT ![c].phase = "rest"]
    /\ UNCHANGED <<log, dbTerm, role, followsT, replacedBy, voted, acked, termStart, ans,
                   heard, called, silent, holdW, pause, committed, clientW, began, lateAck,
                   refusedKnowing, writes, restarts>>

(* handover_hold: no wait when every copy that said it was the primary     *)
(* voted (or, proposed, when every copy voted).                           *)
HoldOf(c, cc) ==
    IF Bug = "takes-writes-in-a-lease" THEN 0
    ELSE IF HoldRule = "primaries"
         THEN IF cc.prim # {} /\ cc.prim \subseteq cc.granted THEN 0 ELSE HoldShort
         ELSE IF (Copies \ {c}) \subseteq cc.granted THEN 0 ELSE HoldShort

(* c won with a majority (Campaign::voted) and begins the term             *)
(* (take_over, term_opened); by hand, it takes writes after the hold.      *)
Win(c) ==
    /\ campaign[c] # Nil /\ campaign[c].phase = "rest" /\ role[c] = "replica"
    /\ LET cc == campaign[c]
           t  == cc.t
       IN
       /\ Cardinality(cc.granted) + 1 >= Maj
       /\ voted[c] = [t |-> t, c |-> c] /\ Newest(c) = t
       /\ role'       = [role EXCEPT ![c] = "primary"]
       /\ dbTerm'     = [dbTerm EXCEPT ![c] = t]
       /\ log'        = [log EXCEPT ![c] = Append(@, t)]
       /\ termStart'  = [termStart EXCEPT ![c] = Len(log[c]) + 1]
       /\ followsT'   = [followsT EXCEPT ![c] = 0]
       /\ replacedBy' = [replacedBy EXCEPT ![c] = 0]
       /\ acked'      = [acked EXCEPT ![c] = Zeros]
       /\ ans'        = [ans EXCEPT ![c] = NoAges]
       /\ heard'      = [heard EXCEPT ![c] = 0]
       /\ silent'     = [silent EXCEPT ![c] = 0]
       /\ called'     = [called EXCEPT ![c] = FALSE]
       /\ holdW'      = [holdW EXCEPT ![c] = IF cc.hand THEN HoldOf(c, cc) ELSE 0]
       /\ pause'      = [pause EXCEPT ![c] = 0]
       /\ campaign'   = [campaign EXCEPT ![c] = Nil]
       /\ began'      = began \cup {<<t, c>>}
    /\ UNCHANGED <<voted, committed, clientW, lateAck, refusedKnowing, writes, restarts>>

(* An election that did not win ends (409, or Campaign::decided): the     *)
(* votes it got stay kept, its own among them. With Prompt, a handover    *)
(* ends once every copy answered.                                          *)
GiveUp(c) ==
    /\ campaign[c] # Nil /\ campaign[c].phase = "rest"
    /\ Prompt /\ campaign[c].hand => campaign[c].answered = Copies \ {c}
    /\ Cardinality(campaign[c].granted) + 1 < Maj \/ Newest(c) # campaign[c].t
    /\ campaign' = [campaign EXCEPT ![c] = Nil]
    /\ UNCHANGED <<log, dbTerm, role, followsT, replacedBy, voted, acked, termStart, ans,
                   heard, called, silent, holdW, pause, committed, clientW, began, lateAck,
                   refusedKnowing, writes, restarts>>

Next ==
    \/ Tick
    \/ \E p \in Copies : \/ Write(p) \/ StepDown(p) \/ Restart(p) \/ StartHandover(p)
                         \/ PreDone(p) \/ FirstDone(p) \/ Win(p) \/ GiveUp(p)
    \/ \E p, r \in Copies : \/ Ship(p, r) \/ ShipFenced(p, r) \/ ShipPrimary(p, r)
                            \/ Hello(p, r) \/ PreAnswer(p, r) \/ Ask(p, r)
    \/ \E c \in Copies, t \in 1..MaxTerm : StartAuto(c, t) \/ HelloDone(c, t)

Spec == Init /\ [][Next]_vars

-----------------------------------------------------------------------------
(* Properties.                                                             *)

TypeOK ==
    /\ \A c \in Copies : role[c] \in {"primary", "replica"}
    /\ \A p, r \in Copies : ans[p][r] \in 0..LeaseLong
    /\ \A c \in Copies : holdW[c] \in 0..HoldShort /\ pause[c] \in 0..PauseShort

(* 1. A read a primary serves under its lease holds every client write    *)
(* acknowledged before it (the simulator's fourth check): checked in every *)
(* state, for a read served then. A write is acknowledged to its client    *)
(* when the primary of its term commits it; one that a newer term commits  *)
(* was answered in doubt (409) when its primary was fenced, and may or may *)
(* not be seen. Without failover a primary holds no lease, and serves what *)
(* it holds until it is fenced.                                            *)
AckedWrites == {e \in committed : <<e[1], e[2]>> \in clientW /\ e[2] = e[3]}
LeaseReadsFresh ==
    Lease > 0 => \A p \in Copies : Readable(p) => \A e \in AckedWrites : Holds(p, e)

(* 2. What the code promises of writers: one copy begins each term, so at *)
(* most one takes writes in a term; and once a copy began a term, no       *)
(* primary of an older term acknowledges a write.                          *)
OneCopyATerm == \A a, b \in began : a[1] = b[1] => a[2] = b[2]
OldPrimaryAcksNothing == ~ lateAck
(* What it does not promise: two copies may take writes at one moment (an *)
(* old primary that has not heard of the new term applies writes it can   *)
(* no longer acknowledge, docs/chloedb/server.md, "Failover").            *)
OneWriter == Cardinality({p \in Copies : Writer(p)}) <= 1

(* As spec/replication: no acknowledged write is lost. *)
NoLostWrite ==
    \A e \in committed : \A c \in Copies :
        (role[c] = "primary" /\ dbTerm[c] > e[3]) => Holds(c, e)

(* 3. The handover's vote order: a primary asked first answers the vote   *)
(* before it can know its term (the replication.rs comment) ...            *)
PrimaryAskedFirst == refusedKnowing = {}
(* ... and no primary is left fenced with no primary serving and no       *)
(* election under way (by hand, nothing but an operator elects one then).  *)
NoOrphanedFence ==
    ~ /\ \E p \in Copies : role[p] = "primary" /\ replacedBy[p] > 0
      /\ ~ \E q \in Copies : role[q] = "primary" /\ replacedBy[q] = 0
      /\ \A c \in Copies : campaign[c] = Nil

=============================================================================
