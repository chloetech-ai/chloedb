---------------------------- MODULE MCHandover ----------------------------
(* Model values for Handover.tla. c0 is the first primary (First), so the *)
(* symmetry swaps the two replicas only. MCSteps bounds a model too large   *)
(* to explore whole to its behaviours of so many steps (a constraint: a     *)
(* state that far along is checked and not followed).                       *)
EXTENDS Handover
CONSTANTS c0, c1, c2
MCThree == {c0, c1, c2}
MCTwo == {c0, c1}
MCSymmetry == Permutations({c1, c2})
MCSteps == TLCGet("level") <= 22
=============================================================================
