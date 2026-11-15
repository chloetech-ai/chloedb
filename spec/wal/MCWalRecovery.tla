--------------------------- MODULE MCWalRecovery ---------------------------
EXTENDS WalRecovery
CONSTANTS w1, w2, k1, k2

MCWriters == {w1, w2}
Put(k) == [kind |-> "put", key |-> k]
Rm(k)  == [kind |-> "del", key |-> k]
New(k) == [kind |-> "ins", key |-> k]

(* w1 upserts k1, w2 deletes it: deletes that race merges and flushes. *)
MenuPutDel == [w \in MCWriters |-> IF w = w1 THEN {Put(k1)} ELSE {Rm(k1)}]
(* Either may upsert or delete k1. *)
MenuAny == [w \in MCWriters |-> {Put(k1), Rm(k1)}]
(* w1 upserts k1, w2 inserts it or upserts k2: groups that fail and run   *)
(* each write alone, and transactions of two keys.                        *)
MenuGroups == [w \in MCWriters |-> IF w = w1 THEN {Put(k1)} ELSE {New(k1), Put(k2)}]

OneEach == [w \in MCWriters |-> 1]
TwoOne  == [w \in MCWriters |-> IF w = w1 THEN 2 ELSE 1]
TwoEach == [w \in MCWriters |-> 2]

Both == {"fg", "bg"}
FgOnly == {"fg"}

SymWriters == Permutations(MCWriters)
=============================================================================
