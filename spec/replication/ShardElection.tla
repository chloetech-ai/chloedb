--------------------------- MODULE ShardElection ---------------------------
(***************************************************************************)
(* One shard's copies under automatic failover, as                         *)
(* chloedb-server/src/cores/shard.rs decides each step: a primary writes   *)
(* and ships to its replicas (Replica::take, Replica::applied,             *)
(* Replica::joined) and answers a write once a majority holds it           *)
(* (Primary::holding, term_start); a replica asks to be elected            *)
(* (Campaign, Replica::take_over) and the others vote (decide); a primary  *)
(* is fenced by a newer term (Primary::fence); a primary crashes and       *)
(* starts again as a replica, as it does with failover; and a replica      *)
(* loses its volume and starts again empty with the same --replica         *)
(* (finding N-E2).                                                         *)
(*                                                                         *)
(* Rule picks the vote rule:                                               *)
(*   "pre-ne2" 0.3.3's rule (the simulator's --bug votes-with-a-lost-volume)*)
(*   "ne2"     main since #512: a first copy a primary called grants no    *)
(*             vote until a call finds it caught up                        *)
(*   "narrow"  the first fix tried for 0.3.4: a first copy stays a member  *)
(*             once a primary called it                                    *)
(*   "fix"     #551, in 0.3.4: as "ne2", but a first copy grants its vote  *)
(*             to the primary that called it first (Kept::called_first)    *)
(*             until a call finds it caught up                             *)
(*                                                                         *)
(* MembersCampaign: a copy that does not count itself a member asks for    *)
(* no votes (Replica::counted, since the follow-up to #551).               *)
(*                                                                         *)
(* A call is one atomic step that may or may not happen (a call lost is a  *)
(* step not taken), and a Replicate's answer may be lost on its way back.  *)
(* Time is not modelled: the sticky delay (a replica refuses votes while   *)
(* its primary called within it) and the lease are left out. They only    *)
(* refuse more votes, so a safety property that holds here holds with     *)
(* them, and a stall found here lasts once the delays ran out.            *)
(***************************************************************************)
EXTENDS Naturals, Sequences, FiniteSets, TLC

CONSTANTS
    Copies,      \* the shard's copies, e.g. {c0, c1, c2}
    First,       \* the copy started as the shard's primary (--replicas)
    Rule,        \* "pre-ne2" | "ne2" | "narrow" | "fix"
    MaxTerm,     \* terms asked for stay at or below this
    MaxWrites,   \* clients' writes, in all
    MaxCrashes,  \* crashes of a primary, which starts again as a replica
    MaxLost,     \* volumes lost
    MembersCampaign, \* TRUE: a copy that is not a member asks for no votes
    LossGuard    \* when a volume may be lost: "any" time; "sim", as the
                 \* simulator loses one (Sim::may_lose_volume): never the
                 \* first primary's, while every other copy heard from a
                 \* primary and no primary of an older term than one a copy
                 \* knows serves; "quiet", as "sim" and while no election is
                 \* under way and every other copy knows each term the copy
                 \* knew (what it forgets, the others know: the simulator's
                 \* --lost-volume placement)

ASSUME Rule \in {"pre-ne2", "ne2", "narrow", "fix"}
ASSUME First \in Copies
ASSUME LossGuard \in {"any", "sim", "quiet"}

Nil == [none |-> TRUE]
Maj == Cardinality(Copies) \div 2 + 1
Max(a, b) == IF a >= b THEN a ELSE b

(* Kept.config: 0 none; 1 Some((0, 0)), which the "pre-ne2" rule gives a  *)
(* first copy so that it waits for no call; 2 the shard's membership,     *)
(* version 1 (the model never changes the copies).                        *)
CfgV(cfg) == IF cfg = 2 THEN 1 ELSE 0

VARIABLES
    log,        \* log[c]: the terms of the transactions c holds, in order
    dbTerm,     \* the database's term (Numbering::term; 1 when empty)
    role,       \* "primary" | "replica"
    follows,    \* [p |-> primary, t |-> term] or Nil  (Kept.follows)
    replacedBy, \* 0, or the newer term that fenced it (Kept.replaced_by)
    voted,      \* [t |-> term, c |-> candidate] or Nil (Kept.voted)
    member,     \* Kept.member
    calledFirst,\* Kept.called_first: the primary that called it first as a
                \* first copy, until a call found it caught up (or Nil)
    config,     \* Kept.config, as above
    acked,      \* acked[p][r]: what primary p last heard r holds (Peer.acked)
    termStart,  \* termStart[p]: its term's first transaction (0: none)
    campaign,   \* campaign[c]: [t, granted, handover] or Nil
    committed,  \* history: <<number, term, term of the primary that answered>>
                \* of each write answered as acknowledged
    began,      \* history: <<term, copy>> of each term begun
    extraWins,  \* history: the terms of began won with a vote of a copy that
                \* did not count itself a member (the "fix" rule's exception)
    writes, crashes, lost

vars == <<log, dbTerm, role, follows, replacedBy, voted, member, calledFirst, config,
          acked, termStart, campaign, committed, began, extraWins, writes, crashes, lost>>

-----------------------------------------------------------------------------
(* What a copy knows and holds.                                            *)

(* Kept::newest *)
Newest(c) ==
    LET f == IF follows[c] = Nil THEN 0 ELSE follows[c].t
        v == IF voted[c] = Nil THEN 0 ELSE voted[c].t
    IN Max(Max(dbTerm[c], f), Max(replacedBy[c], v))

LastTerm(c) == IF log[c] = <<>> THEN 0 ELSE log[c][Len(log[c])]
Last(c) == <<LastTerm(c), Len(log[c])>>
GEQ(a, b) == a[1] > b[1] \/ (a[1] = b[1] /\ a[2] >= b[2])

(* Kept::first_only. 0.3.3 gave a first copy a membership of version 0,   *)
(* which made it no first copy that waits for a call.                      *)
FirstOnly(c) ==
    /\ member[c] /\ role[c] = "replica" /\ follows[c] = Nil
    /\ replacedBy[c] = 0 /\ config[c] = 0

(* A vote request (Campaign::ask): the candidate, the term, and what the  *)
(* candidate held and knew of the membership when it asked.               *)
Req(c, t) == [c |-> c, t |-> t, last |-> Last(c), cfg |-> CfgV(config[c])]

(* Whether voter v grants request q (decide). *)
Grants(v, q) ==
    /\ \/ voted[v] = [t |-> q.t, c |-> q.c]
       \/ q.t > Newest(v)
    /\ GEQ(q.last, Last(v))
    /\ \/ member[v]
       \/ Rule = "fix" /\ calledFirst[v] = q.c
    /\ ~ (q.cfg = 0 /\ q.last = <<0, 0>> /\ CfgV(config[v]) > 0)
    /\ ~ (FirstOnly(v) /\ (q.cfg > 0 \/ q.last # Last(v)))

(* The pre-vote round (Campaign::pre_voted): a majority would grant it,   *)
(* none of them knowing its term or a newer one; a serving primary grants *)
(* no pre-vote but a handover's. Copies that do not answer are left out.  *)
PreVoted(c, t) ==
    Cardinality({c} \cup {v \in Copies \ {c} :
                    role[v] = "replica" /\ t > Newest(v) /\ Grants(v, Req(c, t))}) >= Maj

(* Longest common prefix of two logs. *)
LCP(a, b) ==
    LET n == IF Len(a) < Len(b) THEN Len(a) ELSE Len(b)
        same == {i \in 0..n : \A j \in 1..i : a[j] = b[j]}
    IN CHOOSE i \in same : \A k \in same : k <= i

(* The last write primary p may answer as acknowledged, heard `a` of its   *)
(* replicas (Primary::holding): every one up to the last a majority holds, *)
(* once a majority holds its term's first transaction too.                *)
CommitIndex(p, a) ==
    LET H(j) == Cardinality({p} \cup {q \in Copies \ {p} : a[q] >= j})
        ok   == {j \in 0..Len(log[p]) : j = 0 \/ H(Max(j, termStart[p])) >= Maj}
    IN CHOOSE j \in ok : \A x \in ok : x <= j

-----------------------------------------------------------------------------
Init ==
    /\ log        = [c \in Copies |-> <<>>]
    /\ dbTerm     = [c \in Copies |-> 1]
    /\ role       = [c \in Copies |-> IF c = First THEN "primary" ELSE "replica"]
    /\ follows    = [c \in Copies |-> Nil]
    /\ replacedBy = [c \in Copies |-> 0]
    /\ voted      = [c \in Copies |-> Nil]
    \* Replica::open with --replica on a database with no part: a member.
    /\ member     = [c \in Copies |-> TRUE]
    /\ calledFirst = [c \in Copies |-> Nil]
    \* Primary::new numbers the first membership; Sim::become_replica
    \* gives a first copy version 0 under the bug.
    /\ config     = [c \in Copies |-> IF c = First THEN 2
                                      ELSE IF Rule = "pre-ne2" THEN 1 ELSE 0]
    /\ acked      = [p \in Copies |-> [r \in Copies |-> 0]]
    /\ termStart  = [c \in Copies |-> 0]
    /\ campaign   = [c \in Copies |-> Nil]
    /\ committed  = {}
    /\ began      = {<<1, First>>}
    /\ extraWins  = {}
    /\ writes = 0 /\ crashes = 0 /\ lost = 0

(* A client's write, which the primary commits to its log. Primary::admit *)
(* is left out: it only refuses writes.                                    *)
Write(p) ==
    /\ role[p] = "primary"
    /\ writes < MaxWrites
    /\ log' = [log EXCEPT ![p] = Append(@, dbTerm[p])]
    /\ writes' = writes + 1
    /\ UNCHANGED <<dbTerm, role, follows, replacedBy, voted, member, calledFirst,
                   config, acked, termStart, campaign, committed, began, extraWins,
                   crashes, lost>>

(* Primary p's term is older than one replica r knows: r answers with its *)
(* term (Replica::take), and p steps down (Primary::outcome, fence).      *)
ShipFenced(p, r) ==
    /\ role[p] = "primary" /\ role[r] = "replica" /\ p # r
    /\ dbTerm[p] < Newest(r)
    /\ role'       = [role EXCEPT ![p] = "replica"]
    /\ replacedBy' = [replacedBy EXCEPT ![p] = Newest(r)]
    /\ follows'    = [follows EXCEPT ![p] = Nil]
    /\ member'     = [member EXCEPT ![p] = TRUE]
    /\ calledFirst' = [calledFirst EXCEPT ![p] = Nil]
    /\ UNCHANGED <<log, dbTerm, voted, config, acked, termStart, campaign,
                   committed, began, extraWins, writes, crashes, lost>>

(* Primary p calls replica r (Replicate) and r takes its log up to number *)
(* k (k below p's last: the call ships at most ship_max, or follows what  *)
(* p last heard). r keeps what it holds of p's log and drops what differs.*)
(* Its answer may be lost (answered = FALSE).                              *)
Ship(p, r) ==
    /\ role[p] = "primary" /\ role[r] = "replica" /\ p # r
    /\ dbTerm[p] >= Newest(r)
    \* a second primary of one term is refused, nothing changes
    /\ ~ (follows[r] # Nil /\ follows[r].t = dbTerm[p] /\ follows[r].p # p)
    /\ \E k \in 0..Len(log[p]), answered \in BOOLEAN :
         LET pt     == dbTerm[p]
             n      == Max(k, LCP(log[r], log[p]))
             behind == n < Len(log[p])
             \* take: a first copy no primary called stops counting itself
             m1     == IF FirstOnly(r) /\ Rule \notin {"narrow", "pre-ne2"}
                       THEN FALSE ELSE member[r]
             \* 0.3.3: a member at the first call, caught up or not
             m2     == IF Rule = "pre-ne2" THEN TRUE ELSE m1
             \* joined: a member once a call finds it caught up
             m3     == IF behind THEN m2 ELSE TRUE
             row    == [acked[p] EXCEPT ![r] = n]
         IN
         /\ log'        = [log EXCEPT ![r] = IF n > LCP(log[r], log[p])
                                            THEN SubSeq(log[p], 1, n) ELSE @]
         /\ dbTerm'     = [dbTerm EXCEPT ![r] = Max(@, pt)]
         /\ follows'    = [follows EXCEPT ![r] = [p |-> p, t |-> pt]]
         /\ replacedBy' = [replacedBy EXCEPT ![r] = 0]
         /\ member'     = [member EXCEPT ![r] = m3]
         \* take keeps the primary that called a first copy; joined drops it
         /\ calledFirst' = [calledFirst EXCEPT ![r] =
                              IF m3 /\ ~ member[r] THEN Nil
                              ELSE IF FirstOnly(r) /\ Rule = "fix" /\ ~ m3 THEN p
                              ELSE @]
         /\ config'     = [config EXCEPT ![r] =
                              IF CfgV(config[p]) > CfgV(@) THEN 2 ELSE @]
         /\ IF answered
            THEN /\ acked' = [acked EXCEPT ![p] = row]
                 /\ committed' = committed \cup
                      {<<i, log[p][i], pt>> : i \in 1..CommitIndex(p, row)}
            ELSE UNCHANGED <<acked, committed>>
         /\ UNCHANGED <<role, voted, termStart, campaign, began, extraWins,
                        writes, crashes, lost>>

(* Replica c asks to be elected in term t: after the pre-vote round, or,  *)
(* promoted by hand (a handover, which a serving primary may grant), with *)
(* none (promote_check is left out: it asks only the copies that answer), *)
(* keeping its own vote first (Campaign::vote_self). The request carries  *)
(* what c holds as it asks.                                               *)
StartCampaign(c, t, handover) ==
    /\ role[c] = "replica"
    /\ MembersCampaign => member[c]
    /\ t > Newest(c) /\ t <= MaxTerm
    /\ handover \/ PreVoted(c, t)
    /\ voted'    = [voted EXCEPT ![c] = [t |-> t, c |-> c]]
    /\ campaign' = [campaign EXCEPT ![c] = [req |-> Req(c, t), granted |-> {c},
                                             handover |-> handover, extra |-> FALSE]]
    /\ UNCHANGED <<log, dbTerm, role, follows, replacedBy, member, calledFirst,
                   config, acked, termStart, committed, began, extraWins, writes,
                   crashes, lost>>

(* Copy v grants c's vote (Replica::vote, Primary::vote). A refusal       *)
(* changes nothing, so it is no step. A serving primary grants only a     *)
(* handover, and steps down once it did (Primary::gave_vote).             *)
Vote(v, c) ==
    /\ campaign[c] # Nil /\ v # c /\ v \notin campaign[c].granted
    /\ LET t == campaign[c].req.t IN
       /\ role[v] = "primary" => campaign[c].handover
       /\ Grants(v, campaign[c].req)
       /\ voted'    = [voted EXCEPT ![v] = [t |-> t, c |-> c]]
       /\ campaign' = [campaign EXCEPT ![c].granted = @ \cup {v},
                                         \* granted only by the "fix" rule's exception
                                         ![c].extra = @ \/ ~ member[v]]
       /\ IF role[v] = "primary"
          THEN /\ role'       = [role EXCEPT ![v] = "replica"]
               /\ replacedBy' = [replacedBy EXCEPT ![v] = t]
               /\ follows'    = [follows EXCEPT ![v] = Nil]
               /\ calledFirst' = [calledFirst EXCEPT ![v] = Nil]
          ELSE UNCHANGED <<role, replacedBy, follows, calledFirst>>
       /\ UNCHANGED <<log, dbTerm, member, config, acked, termStart,
                      committed, began, extraWins, writes, crashes, lost>>

(* c won term t with a majority and begins it (Replica::take_over,        *)
(* term_opened commits the term's first transaction, Primary::new).       *)
Win(c) ==
    /\ campaign[c] # Nil /\ role[c] = "replica"
    /\ LET t == campaign[c].req.t IN
       /\ Cardinality(campaign[c].granted) >= Maj
       /\ voted[c] = [t |-> t, c |-> c] /\ Newest(c) = t
       /\ role'       = [role EXCEPT ![c] = "primary"]
       /\ dbTerm'     = [dbTerm EXCEPT ![c] = t]
       /\ log'        = [log EXCEPT ![c] = Append(@, t)]
       /\ termStart'  = [termStart EXCEPT ![c] = Len(log[c]) + 1]
       /\ follows'    = [follows EXCEPT ![c] = Nil]
       /\ replacedBy' = [replacedBy EXCEPT ![c] = 0]
       /\ member'     = [member EXCEPT ![c] = TRUE]
       /\ calledFirst' = [calledFirst EXCEPT ![c] = Nil]
       /\ config'     = [config EXCEPT ![c] = IF @ = 0 THEN 2 ELSE @]
       /\ acked'      = [acked EXCEPT ![c] = [r \in Copies |-> 0]]
       /\ campaign'   = [campaign EXCEPT ![c] = Nil]
       /\ began'      = began \cup {<<t, c>>}
       /\ extraWins'  = IF campaign[c].extra THEN extraWins \cup {<<t, c>>} ELSE extraWins
       /\ UNCHANGED <<voted, committed, writes, crashes, lost>>

(* An election that did not win in time gives up (Campaign::decided):     *)
(* the votes it got stay kept.                                             *)
GiveUp(c) ==
    /\ campaign[c] # Nil
    /\ campaign' = [campaign EXCEPT ![c] = Nil]
    /\ UNCHANGED <<log, dbTerm, role, follows, replacedBy, voted, member, calledFirst,
                   config, acked, termStart, committed, began, extraWins, writes,
                   crashes, lost>>

(* A primary crashes and, with failover, starts again as a replica        *)
(* (Sim::boot, shard::restart).                                            *)
CrashPrimary(p) ==
    /\ role[p] = "primary" /\ crashes < MaxCrashes
    /\ role'     = [role EXCEPT ![p] = "replica"]
    /\ crashes'  = crashes + 1
    /\ UNCHANGED <<log, dbTerm, follows, replacedBy, voted, member, calledFirst,
                   config, acked, termStart, campaign, committed, began, extraWins,
                   writes, lost>>

(* Sim::may_lose_volume *)
MayLose(r) ==
    LET others == Copies \ {r}
        newest == CHOOSE x \in {Newest(o) : o \in others} :
                    \A o \in others : Newest(o) <= x
    IN /\ r # First
       /\ \A o \in others : config[o] # 0
       /\ ~ \E o \in others : role[o] = "primary" /\ dbTerm[o] < newest

(* Copy r loses its volume, database and part together, and starts again  *)
(* with the same --replica on an empty database (Sim::lose_volume).       *)
LoseVolume(r) ==
    /\ lost < MaxLost
    /\ LossGuard # "any" => MayLose(r)
    /\ LossGuard = "quiet" =>
         /\ \A o \in Copies : campaign[o] = Nil
         /\ \A o \in Copies \ {r} : Newest(o) >= Newest(r)
    /\ log'        = [log EXCEPT ![r] = <<>>]
    /\ dbTerm'     = [dbTerm EXCEPT ![r] = 1]
    /\ role'       = [role EXCEPT ![r] = "replica"]
    /\ follows'    = [follows EXCEPT ![r] = Nil]
    /\ replacedBy' = [replacedBy EXCEPT ![r] = 0]
    /\ voted'      = [voted EXCEPT ![r] = Nil]
    /\ member'     = [member EXCEPT ![r] = TRUE]
    /\ calledFirst' = [calledFirst EXCEPT ![r] = Nil]
    /\ config'     = [config EXCEPT ![r] = IF Rule = "pre-ne2" THEN 1 ELSE 0]
    /\ acked'      = [acked EXCEPT ![r] = [q \in Copies |-> 0]]
    /\ termStart'  = [termStart EXCEPT ![r] = 0]
    /\ campaign'   = [campaign EXCEPT ![r] = Nil]
    /\ lost'       = lost + 1
    /\ UNCHANGED <<committed, began, extraWins, writes, crashes>>

Next ==
    \/ \E p \in Copies : Write(p) \/ CrashPrimary(p) \/ Win(p) \/ GiveUp(p)
    \/ \E p, r \in Copies : ShipFenced(p, r) \/ Ship(p, r) \/ Vote(p, r)
    \/ \E c \in Copies, t \in 1..MaxTerm, h \in BOOLEAN : StartCampaign(c, t, h)
    \/ \E r \in Copies : LoseVolume(r)

Spec == Init /\ [][Next]_vars

-----------------------------------------------------------------------------
(* Properties.                                                             *)

TypeOK ==
    /\ \A c \in Copies : role[c] \in {"primary", "replica"}
    /\ \A c \in Copies : member[c] \in BOOLEAN

(* One copy began each term: a term has one primary. *)
OneCopyATerm ==
    \A a, b \in began : a[1] = b[1] => a[2] = b[2]

(* No write answered as acknowledged is lost: every primary of a newer    *)
(* term than the primary that answered it holds it (Raft's leader         *)
(* completeness; the simulator's second check).                            *)
NoLostWrite ==
    \A e \in committed : \A c \in Copies :
        (role[c] = "primary" /\ dbTerm[c] > e[3])
            => (Len(log[c]) >= e[1] /\ log[c][e[1]] = e[2])

(* Two copies holding a transaction of one number and term hold the same *)
(* history up to it.                                                       *)
LogMatching ==
    \A a, b \in Copies : \A i \in 1..Len(log[a]) :
        (i <= Len(log[b]) /\ log[a][i] = log[b][i])
            => SubSeq(log[a], 1, i) = SubSeq(log[b], 1, i)

(* The shard has no primary and none can ever be elected again: each      *)
(* copy, asking for a term newer than any copy knows, gets fewer than a    *)
(* majority. With no primary nothing but votes changes a copy, and a vote *)
(* changes none of what Grants reads for such a term, so the state is      *)
(* final but for another lost volume. (The simulator's last check: the    *)
(* shard settles.)                                                         *)
Top == CHOOSE x \in {Newest(c) + 1 : c \in Copies} :
         \A c \in Copies : Newest(c) + 1 <= x
CanWin(c) == Cardinality({c} \cup {v \in Copies \ {c} : Grants(v, Req(c, Top))}) >= Maj
Stalled == (\A c \in Copies : role[c] = "replica") /\ (\A c \in Copies : ~ CanWin(c))
NeverStalled == ~ Stalled

(* The "fix" rule's exception never elects a copy without a write        *)
(* answered as acknowledged: NoLostWrite, for the primaries won with a    *)
(* vote "ne2" refuses.                                                     *)
NoLostWriteByExtraVote ==
    \A e \in committed : \A c \in Copies :
        (role[c] = "primary" /\ dbTerm[c] > e[3] /\ <<dbTerm[c], c>> \in extraWins)
            => (Len(log[c]) >= e[1] /\ log[c][e[1]] = e[2])

=============================================================================
