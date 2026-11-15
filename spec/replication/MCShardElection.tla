------------------------- MODULE MCShardElection -------------------------
EXTENDS ShardElection
CONSTANTS c0, c1, c2
MCCopies == {c0, c1, c2}
MCSymmetry == Permutations({c1, c2})
=============================================================================
