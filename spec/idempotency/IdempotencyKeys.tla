-------------------------- MODULE IdempotencyKeys --------------------------
(***************************************************************************)
(* A client's write with an idempotency key, retried after its answer was *)
(* lost, on one shard of three copies under failover, as                  *)
(* chloedb-server/src/idem.rs and src/service.rs handle a key:            *)
(*                                                                         *)
(*  - Service::begin_key looks the key up in memory first (Keys::begin:   *)
(*    New claims it, Busy is a 409, Done answers the answer kept), then,  *)
(*    for a key memory does not hold, in the database (idem::recorded:    *)
(*    the key's row of _sources, answered with the rows and the           *)
(*    database's last sequence number, which Keys::finish then keeps);    *)
(*  - the writer applies a new one (apply_keyed_in, commits_in: only in   *)
(*    the term it was taken in) and records the key in the write's own    *)
(*    transaction (idem::record), so the key is in the same log entry as  *)
(*    the write; Queued::answer then keeps the answer in memory           *)
(*    (Keys::finish);                                                      *)
(*  - an answer, a first one or a retry's, waits for the copies           *)
(*    (Service::replicated, Primary::acknowledged, Primary::acked,        *)
(*    Primary::holding: counted at the term's first transaction,          *)
(*    term_start, which replication::term_opened commits as a primary     *)
(*    begins its term, finding DS-5), and is an error once the primary    *)
(*    stepped down;                                                        *)
(*  - memory is lost when a server restarts, and kept when it steps down  *)
(*    (Service::step_down: the server turns replica in its process, and   *)
(*    the new primary sends it a copy, Database::install_copy, that       *)
(*    replaces its tables, _sources too) and when it is elected again     *)
(*    (Service::fail_over, Follow::take_over);                             *)
(*  - Keys::begin forgets the oldest keys, past the TTL or the 100,000    *)
(*    it keeps, whatever their state (Evict).                              *)
(*                                                                         *)
(* The client's retries go through the coordinator (coord.rs:             *)
(* Coordinator::write_round passes the key as key/shard, the same in      *)
(* every retry; Coordinator::write sends it again, to the primary that    *)
(* replaced one that did not answer or refused it unapplied, and hands an *)
(* in-doubt write back to the client, who may send it again with its      *)
(* key): a retry reaches whichever copy is a primary, a replaced one not  *)
(* fenced yet too. The write codes (x-chloedb-write) only decide which    *)
(* of these retries the coordinator makes itself; the client may make any *)
(* of them, so the model lets every retry happen. A retry refused before  *)
(* anything (a replica, a 409 Busy, a 429) changes nothing, so it is no   *)
(* step. A client sees only the answer to its latest attempt (the        *)
(* connections of the others are gone), and may send again after an OK   *)
(* (the coordinator's answer to it was lost).                              *)
(*                                                                         *)
(* Elections are abstracted, as the slots spec does: a replica wins a     *)
(* term above every term a majority of the copies knows, with their votes *)
(* (a primary among them votes in a handover, and steps down), when its   *)
(* log is as up to date as each of theirs (spec/replication checks the    *)
(* vote rules that give this). A primary learns of a newer term from any  *)
(* copy that knows it and steps down (Primary::fence). Shipping and the   *)
(* commit rule are ShardElection's. Time is not modelled: the TTL of a    *)
(* key's row is taken as not reached (a retry later than it is a new      *)
(* write, as documented), and memory's TTL and capacity are Evict. The    *)
(* write writes a row: the key of one that writes none is kept in memory  *)
(* only (idem::record), which the model leaves out.                        *)
(*                                                                         *)
(* ForgetOnStepDown: a fix, the in-memory keys forgotten when the server  *)
(* stops being the primary (as a restart forgets them).                   *)
(* WriterChecks: a fix, the writer looks the key's row up in the write's  *)
(* own transaction and answers a retry it finds there, unapplied.         *)
(* AcksAnOlderTerm: DS-5 put back, as the simulator's                     *)
(* Bug::AcksAnOlderTerm: a primary commits nothing as it begins its term, *)
(* and counts a transaction of an older term as acknowledged once a       *)
(* majority holds it (term_start 0).                                       *)
(***************************************************************************)
EXTENDS Integers, Sequences, FiniteSets, TLC

CONSTANTS
    Copies,           \* the shard's copies, e.g. {c0, c1, c2}
    First,            \* the shard's first primary (term 1)
    MaxTerm,          \* terms begun stay at or below this
    MaxSends,         \* attempts of the keyed write (the first and retries)
    MaxOther,         \* other clients' writes, without a key
    MaxCrashes,       \* restarts of a copy, which forget its memory
    MaxEvict,         \* times memory forgets the key (Keys::begin)
    ForgetOnStepDown, \* fix: memory forgotten when a primary steps down
    WriterChecks,     \* fix: the writer checks the key's row
    AcksAnOlderTerm   \* DS-5 put back

ASSUME First \in Copies

Maj == Cardinality(Copies) \div 2 + 1
Max(a, b) == IF a >= b THEN a ELSE b
Min(a, b) == IF a <= b THEN a ELSE b
MaxOf(S) == CHOOSE x \in S : \A y \in S : y <= x

(* What memory holds of the key (Keys.inner.keys): nothing, a request     *)
(* running (None), or a committed answer, its sequence number (>= 1).     *)
Unknown == -1
Busy == 0

(* Log entries: the term the transaction was numbered in, and what it is: *)
(* "w" the keyed write (its rows and the key's row of _sources), "o"      *)
(* another client's write, "m" a term's first transaction (term_opened).  *)
W == "w"
Entry(t, k) == [t |-> t, k |-> k]

Zero == [c \in Copies |-> 0]
Idle == [phase |-> "idle", at |-> First, t |-> 0, seq |-> 0, how |-> "none"]

VARIABLES
    log,        \* log[c]: c's transactions in order (its database)
    cterm,      \* the newest term c knows (Kept::newest); a primary's term
    role,       \* "primary" | "replica"
    acked,      \* acked[p][r]: what primary p last heard r holds (Peer.acked)
    termStart,  \* termStart[p]: its term's first transaction (0: none)
    mem,        \* mem[c]: what c's memory holds of the key (Unknown, Busy, seq)
    queued,     \* queued[p]: attempts of the key p's writer has to apply
    client,     \* the client's latest attempt: [phase, at, t, seq, how]
    okAns,      \* history: each answer of OK the key's write got
    committed,  \* history: <<number, entry, term>> of each transaction of
                \* the write a primary of that term counted as held by a
                \* majority (the others' are not kept: it keeps the
                \* model small)
    began,      \* history: <<term, copy>> of each term begun
    sends, others, crashes, evicts

vars == <<log, cterm, role, acked, termStart, mem, queued, client, okAns,
          committed, began, sends, others, crashes, evicts>>

-----------------------------------------------------------------------------
LastTerm(c) == IF log[c] = <<>> THEN 0 ELSE log[c][Len(log[c])].t
Last(c) == <<LastTerm(c), Len(log[c])>>
GEQ(a, b) == a[1] > b[1] \/ (a[1] = b[1] /\ a[2] >= b[2])

(* c's database holds the key's row: its log holds the write. *)
Holds(c) == \E i \in 1..Len(log[c]) : log[c][i].k = W
Count(c) == Cardinality({i \in 1..Len(log[c]) : log[c][i].k = W})

(* Longest common prefix of two logs. *)
LCP(a, b) ==
    LET n == Min(Len(a), Len(b))
        same == {i \in 0..n : \A j \in 1..i : a[j] = b[j]}
    IN MaxOf(same)

(* Primary::holding: a majority holds transaction j, counted at the term's *)
(* first transaction when j is older.                                      *)
Majority(p, a, j) ==
    Cardinality({p} \cup {q \in Copies \ {p} : a[q] >= Max(j, termStart[p])}) >= Maj

(* The last transaction p may answer as acknowledged, heard `a`. *)
CommitIndex(p, a) == MaxOf({j \in 0..Len(log[p]) : j = 0 \/ Majority(p, a, j)})

(* What a primary keeps of its memory as it steps down: a request running *)
(* is refused by the writer (commits_in) and forgotten (Keys::finish).     *)
Kept(c) == IF ForgetOnStepDown \/ mem[c] = Busy THEN Unknown ELSE mem[c]

(* The client's attempt at a copy of S, which stops being a primary, gets *)
(* an error (in doubt, or none): it can no longer be answered OK.         *)
Gone(S) == IF client.at \in S /\ client.phase # "idle" THEN Idle ELSE client

-----------------------------------------------------------------------------
Init ==
    /\ log       = [c \in Copies |-> <<>>]
    /\ cterm     = [c \in Copies |-> 1]
    /\ role      = [c \in Copies |-> IF c = First THEN "primary" ELSE "replica"]
    /\ acked     = [p \in Copies |-> Zero]
    /\ termStart = [c \in Copies |-> 0]
    /\ mem       = [c \in Copies |-> Unknown]
    /\ queued    = [c \in Copies |-> 0]
    /\ client    = Idle
    /\ okAns     = {}
    /\ committed = {}
    /\ began     = {<<1, First>>}
    /\ sends = 0 /\ others = 0 /\ crashes = 0 /\ evicts = 0

(* The client's write with key k (a first attempt or a retry) reaches      *)
(* primary p (Service::write_ops_counted, begin_key). Busy in memory is a *)
(* 409 that changes nothing, so it is no step.                             *)
Send(p) ==
    /\ role[p] = "primary" /\ sends < MaxSends
    /\ mem[p] # Busy
    /\ sends' = sends + 1
    /\ IF mem[p] # Unknown
       THEN \* Keys::begin: Done, the answer kept
            /\ client' = [phase |-> "answer", at |-> p, t |-> cterm[p],
                          seq |-> mem[p], how |-> "memory"]
            /\ UNCHANGED <<mem, queued>>
       ELSE IF Holds(p)
       THEN \* idem::recorded: the database's last number, kept in memory
            /\ client' = [phase |-> "answer", at |-> p, t |-> cterm[p],
                          seq |-> Len(log[p]), how |-> "row"]
            /\ mem' = [mem EXCEPT ![p] = Len(log[p])]
            /\ UNCHANGED queued
       ELSE \* New: claimed, and queued for the writer
            /\ client' = [phase |-> "queued", at |-> p, t |-> cterm[p],
                          seq |-> 0, how |-> "applied"]
            /\ mem' = [mem EXCEPT ![p] = Busy]
            /\ queued' = [queued EXCEPT ![p] = @ + 1]
    /\ UNCHANGED <<log, cterm, role, acked, termStart, okAns, committed, began,
                   others, crashes, evicts>>

(* p's writer applies an attempt it took in its term (apply_keyed_in), the *)
(* key's row in the same transaction (idem::record), and keeps the answer *)
(* in memory if memory still holds the key (Queued::answer, Keys::finish). *)
(* With WriterChecks it finds the key's row first and applies nothing.     *)
(* The client's attempt, the latest, is the last one queued.               *)
Apply(p) ==
    /\ role[p] = "primary" /\ queued[p] > 0
    /\ LET found == WriterChecks /\ Holds(p)
           seq   == IF found THEN Len(log[p]) ELSE Len(log[p]) + 1
       IN
       /\ log' = IF found THEN log ELSE [log EXCEPT ![p] = Append(@, Entry(cterm[p], W))]
       /\ queued' = [queued EXCEPT ![p] = @ - 1]
       /\ mem' = [mem EXCEPT ![p] = IF @ = Unknown THEN Unknown ELSE seq]
       /\ client' = IF client.phase = "queued" /\ client.at = p
                       /\ client.t = cterm[p] /\ queued[p] = 1
                    THEN [client EXCEPT !.phase = "answer", !.seq = seq,
                                        !.how = IF found THEN "row" ELSE "applied"]
                    ELSE client
    /\ UNCHANGED <<cterm, role, acked, termStart, okAns, committed, began, sends,
                   others, crashes, evicts>>

(* The client's attempt is answered OK (Service::replicated): its primary *)
(* is the primary of the term it was taken in, and a majority holds its   *)
(* sequence number, or its term's first transaction if that is later      *)
(* (Primary::acked). The answer notes whether the primary's log holds the *)
(* write at or before that number, and at it.                              *)
AnswerOK ==
    /\ client.phase = "answer"
    /\ LET p == client.at
           s == client.seq
       IN
       /\ role[p] = "primary" /\ cterm[p] = client.t
       /\ Majority(p, acked[p], s)
       /\ okAns' = okAns \cup
            {[t |-> client.t, seq |-> s, how |-> client.how,
              sees  |-> \E i \in 1..Min(s, Len(log[p])) : log[p][i].k = W,
              exact |-> s <= Len(log[p]) /\ s >= 1 /\ log[p][s].k = W]}
       /\ client' = Idle
    /\ UNCHANGED <<log, cterm, role, acked, termStart, mem, queued, committed, began,
                   sends, others, crashes, evicts>>

(* Another client's write, without a key. *)
Other(p) ==
    /\ role[p] = "primary" /\ others < MaxOther
    /\ log' = [log EXCEPT ![p] = Append(@, Entry(cterm[p], "o"))]
    /\ others' = others + 1
    /\ UNCHANGED <<cterm, role, acked, termStart, mem, queued, client, okAns,
                   committed, began, sends, crashes, evicts>>

(* Primary p calls replica r (Replicate) and r takes p's log up to number *)
(* n, dropping what differs (a copy of the primary, install_copy, when    *)
(* its log differs), and answers. An answer lost is left out: it only     *)
(* keeps p from answering a write as acknowledged, and an answer OK is    *)
(* never forced, so it removes no behaviour of the logs and adds none of  *)
(* the answers.                                                            *)
Ship(p, r) ==
    /\ role[p] = "primary" /\ role[r] = "replica" /\ p # r
    /\ cterm[p] >= cterm[r]
    /\ LET l == LCP(log[r], log[p]) IN
       \E n \in l..Len(log[p]) :
         LET row == [acked[p] EXCEPT ![r] = n] IN
         /\ log'   = [log EXCEPT ![r] = IF n > l THEN SubSeq(log[p], 1, n) ELSE @]
         /\ cterm' = [cterm EXCEPT ![r] = cterm[p]]
         /\ acked' = [acked EXCEPT ![p] = row]
         /\ committed' = committed \cup
              {<<i, log[p][i], cterm[p]>> :
                 i \in {j \in 1..CommitIndex(p, row) : log[p][j].k = W}}
    /\ UNCHANGED <<role, termStart, mem, queued, client, okAns, began, sends,
                   others, crashes, evicts>>

(* Primary p hears of a newer term from copy r (its call to r, or r's     *)
(* call to it: Primary::shipped_to, fence) and steps down                  *)
(* (Service::step_down), in its process.                                   *)
Fenced(p, r) ==
    /\ role[p] = "primary" /\ r # p /\ cterm[r] > cterm[p]
    /\ role'      = [role EXCEPT ![p] = "replica"]
    /\ cterm'     = [cterm EXCEPT ![p] = cterm[r]]
    /\ mem'       = [mem EXCEPT ![p] = Kept(p)]
    /\ queued'    = [queued EXCEPT ![p] = 0]
    /\ acked'     = [acked EXCEPT ![p] = Zero]
    /\ termStart' = [termStart EXCEPT ![p] = 0]
    /\ client'    = Gone({p})
    /\ UNCHANGED <<log, okAns, committed, began, sends, others, crashes, evicts>>

(* Replica c is elected by the copies Q, a majority with it among them,   *)
(* in the term above every term they know, each holding no transaction c  *)
(* lacks; a primary among them voted in a handover and steps down          *)
(* (Primary::gave_vote, Service::vote, step_down). c begins the term       *)
(* (Follow::take_over, term_opened, Primary::new, term_start).             *)
Elect(c, Q) ==
    /\ role[c] = "replica" /\ c \in Q /\ Cardinality(Q) >= Maj
    /\ LET t     == 1 + MaxOf({cterm[v] : v \in Q})
           downs == {v \in Q : role[v] = "primary"}
       IN
       /\ t <= MaxTerm
       /\ \A v \in Q : GEQ(Last(c), Last(v))
       /\ role'  = [x \in Copies |-> IF x = c THEN "primary"
                                     ELSE IF x \in downs THEN "replica" ELSE role[x]]
       /\ cterm' = [x \in Copies |-> IF x \in Q THEN t ELSE cterm[x]]
       /\ log'   = IF AcksAnOlderTerm THEN log
                   ELSE [log EXCEPT ![c] = Append(@, Entry(t, "m"))]
       /\ termStart' = [x \in Copies |->
                          IF x = c THEN (IF AcksAnOlderTerm THEN 0 ELSE Len(log[c]) + 1)
                          ELSE IF x \in downs THEN 0 ELSE termStart[x]]
       /\ acked'  = [x \in Copies |-> IF x = c \/ x \in downs THEN Zero ELSE acked[x]]
       /\ mem'    = [x \in Copies |-> IF x \in downs THEN Kept(x) ELSE mem[x]]
       /\ queued' = [x \in Copies |-> IF x \in downs THEN 0 ELSE queued[x]]
       /\ began'  = began \cup {<<t, c>>}
       /\ client' = Gone(downs)
    /\ UNCHANGED <<okAns, committed, sends, others, crashes, evicts>>

(* Copy c restarts (a primary, with failover, as a replica): its memory   *)
(* is gone. A replica's restart changes only its memory.                   *)
Crash(c) ==
    /\ crashes < MaxCrashes
    /\ role[c] = "primary" \/ mem[c] # Unknown
    /\ role'      = [role EXCEPT ![c] = "replica"]
    /\ mem'       = [mem EXCEPT ![c] = Unknown]
    /\ queued'    = [queued EXCEPT ![c] = 0]
    /\ acked'     = [acked EXCEPT ![c] = Zero]
    /\ termStart' = [termStart EXCEPT ![c] = 0]
    /\ crashes'   = crashes + 1
    /\ client'    = Gone({c})
    /\ UNCHANGED <<log, cterm, okAns, committed, began, sends, others, evicts>>

(* Keys::begin forgets the key: it is past the TTL, or the oldest of more *)
(* than CAPACITY (or an earlier claim of it that did not commit is, whose *)
(* place in `order` stays after Keys::finish forgot it), whatever its     *)
(* state. Its row stays.                                                   *)
Evict(c) ==
    /\ evicts < MaxEvict /\ mem[c] # Unknown
    /\ mem' = [mem EXCEPT ![c] = Unknown]
    /\ evicts' = evicts + 1
    /\ UNCHANGED <<log, cterm, role, acked, termStart, queued, client, okAns,
                   committed, began, sends, others, crashes>>

Next ==
    \/ AnswerOK
    \/ \E p \in Copies : Send(p) \/ Apply(p) \/ Other(p) \/ Crash(p) \/ Evict(p)
    \/ \E p, r \in Copies : Ship(p, r) \/ Fenced(p, r)
    \/ \E c \in Copies, Q \in SUBSET Copies : Elect(c, Q)

Spec == Init /\ [][Next]_vars

-----------------------------------------------------------------------------
(* Properties.                                                             *)

TypeOK ==
    /\ \A c \in Copies : role[c] \in {"primary", "replica"}
    /\ \A c \in Copies : mem[c] \in {Unknown, Busy} \cup Nat
    /\ client.phase \in {"idle", "queued", "answer"}

(* Sanity of the abstraction: one copy began each term, logs that match,   *)
(* and no transaction of the write counted as held by a majority is lost  *)
(* (Raft's leader completeness, ShardElection's NoLostWrite).              *)
OneCopyATerm == \A a, b \in began : a[1] = b[1] => a[2] = b[2]

LogMatching ==
    \A a, b \in Copies : \A i \in 1..Len(log[a]) :
        (i <= Len(log[b]) /\ log[a][i] = log[b][i])
            => SubSeq(log[a], 1, i) = SubSeq(log[b], 1, i)

NoLostCommit ==
    \A e \in committed : \A c \in Copies :
        (role[c] = "primary" /\ cterm[c] > e[3])
            => (Len(log[c]) >= e[1] /\ log[c][e[1]] = e[2])

(* 1. Applied once: no copy's history holds the write twice, and no two   *)
(* transactions of it were ever counted as held by a majority.            *)
AppliedOnce == \A c \in Copies : Count(c) <= 1

CommittedOnce == Cardinality({<<e[1], e[2]>> : e \in {x \in committed : x[2].k = W}}) <= 1

(* 2. Never lost: a write answered OK, by its first attempt or a retry,   *)
(* is in the log of the primary that answered it and of every primary of  *)
(* a newer term.                                                           *)
NoLostWrite ==
    \A a \in okAns : \A c \in Copies :
        (role[c] = "primary" /\ cterm[c] >= a.t) => Holds(c)

(* 3. A retry answered from its key gets the first attempt's answer: its  *)
(* sequence number names a version that holds the write (a read with that *)
(* min_seq_no sees it), as docs/chloedb/server.md says of a retry answered *)
(* from the row ("the server's last seq_no, which follows the write's").   *)
RetrySeesWrite == \A a \in okAns : a.sees

(* The stricter reading, docs/chloedb/server.md on --seq-no ("a retry     *)
(* with an idempotency key gets the first attempt's"): the number is the   *)
(* write's own. Not what a retry answered from the row gets.               *)
SameSeq == \A a \in okAns : a.exact

=============================================================================
