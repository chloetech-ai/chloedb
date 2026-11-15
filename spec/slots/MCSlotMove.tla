---------------------------- MODULE MCSlotMove ----------------------------
EXTENDS SlotMove, TLC
CONSTANTS a1, a2
MCSA == {a1, a2}
MCSymmetry == Permutations(MCSA)
=============================================================================
