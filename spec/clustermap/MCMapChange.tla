---------------------------- MODULE MCMapChange ----------------------------
EXTENDS MapChange, TLC
CONSTANTS s1, s2, s3
MCServers == {s1, s2, s3}
\* With Move, s1 is the source and only s2 and s3 are alike.
MCSymmetry == Permutations({s2, s3})
MCSymmetryAll == Permutations(MCServers)
=============================================================================
