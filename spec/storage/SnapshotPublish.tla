-------------------------- MODULE SnapshotPublish --------------------------
(***************************************************************************)
(* Published snapshots (DbOptions::publish_snapshots, chloedb/src/storage/ *)
(* db.rs): the writer, under its lock, makes version v+1 current, then     *)
(* republishes: if a reader took a snapshot since it last published        *)
(* (`read`, swapped to false), it builds the new version's snapshot and    *)
(* hands it to readers; if none did, it publishes none, and readers build  *)
(* the current version themselves. A reader sets `read`, returns the       *)
(* published snapshot if there is one, else builds the current version     *)
(* (snapshot_inner) and, since 5248809 (#339), looks at the published one  *)
(* again and returns that instead if one appeared.                         *)
(*                                                                         *)
(* Recheck = FALSE is the code before 5248809: TLC finds a reader whose    *)
(* next snapshot is older than its last ("970 after 980").                 *)
(***************************************************************************)
EXTENDS Naturals, Sequences, FiniteSets

CONSTANTS Readers, MaxVersion, Recheck

None == 0  \* versions start at 1

VARIABLES
    current,    \* the writer's current version (State)
    published,  \* the published snapshot's version, or None
    read,       \* whether a snapshot was taken since the last publish
    wpc,        \* the writer: "idle" | "applied" | "built"
    wbuilt,     \* the version the writer built to publish (or None)
    returned,   \* the newest version whose write returned to its client
    rpc,        \* each reader: "idle" | "flagged" | "built"
    rbuilt,     \* the version a reader built
    rstart,     \* `returned` when the reader's snapshot call began
    last,       \* each reader's last snapshot (None: none yet)
    back,       \* history: a reader got an older snapshot than its last
    stale       \* history: a snapshot lacked a write that had returned

vars == <<current, published, read, wpc, wbuilt, returned, rpc, rbuilt, rstart,
          last, back, stale>>

(* Reader r takes a snapshot of version v, asked for when `from` had      *)
(* returned.                                                               *)
Took(r, v, from) ==
    /\ last' = [last EXCEPT ![r] = v]
    /\ back' = (back \/ (last[r] # None /\ v < last[r]))
    /\ stale' = (stale \/ v < from)

Init ==
    /\ current = 1 /\ published = None /\ read = FALSE
    /\ wpc = "idle" /\ wbuilt = None /\ returned = 1
    /\ rpc = [r \in Readers |-> "idle"]
    /\ rbuilt = [r \in Readers |-> None]
    /\ rstart = [r \in Readers |-> 0]
    /\ last = [r \in Readers |-> None]
    /\ back = FALSE /\ stale = FALSE

(* The writer applies a write: a new current version. *)
Apply ==
    /\ wpc = "idle" /\ current < MaxVersion
    /\ current' = current + 1
    /\ wpc' = "applied"
    /\ UNCHANGED <<published, read, wbuilt, returned, rpc, rbuilt, rstart, last, back, stale>>

(* republish: swap `read`, and build the current version if it was set. *)
Swap ==
    /\ wpc = "applied"
    /\ read' = FALSE
    /\ wbuilt' = IF read THEN current ELSE None
    /\ wpc' = "built"
    /\ UNCHANGED <<current, published, returned, rpc, rbuilt, rstart, last, back, stale>>

(* ... and store it (none when nobody read); the write returns. *)
Store ==
    /\ wpc = "built"
    /\ published' = wbuilt
    /\ returned' = current
    /\ wpc' = "idle"
    /\ UNCHANGED <<current, read, wbuilt, rpc, rbuilt, rstart, last, back, stale>>

(* Database::snapshot *)
Flag(r) ==
    /\ rpc[r] = "idle"
    /\ read' = TRUE
    /\ rstart' = [rstart EXCEPT ![r] = returned]
    /\ IF published # None
       THEN /\ Took(r, published, returned)
            /\ UNCHANGED <<rpc, rbuilt>>
       ELSE /\ rpc' = [rpc EXCEPT ![r] = "flagged"]
            /\ UNCHANGED <<rbuilt, last, back, stale>>
    /\ UNCHANGED <<current, published, wpc, wbuilt, returned>>

Build(r) ==
    /\ rpc[r] = "flagged"
    /\ rbuilt' = [rbuilt EXCEPT ![r] = current]
    /\ rpc' = [rpc EXCEPT ![r] = "built"]
    /\ UNCHANGED <<current, published, read, wpc, wbuilt, returned, rstart, last, back, stale>>

Return(r) ==
    /\ rpc[r] = "built"
    /\ Took(r, IF Recheck /\ published # None THEN published ELSE rbuilt[r], rstart[r])
    /\ rpc' = [rpc EXCEPT ![r] = "idle"]
    /\ UNCHANGED <<current, published, read, wpc, wbuilt, returned, rbuilt, rstart>>

Next ==
    \/ Apply \/ Swap \/ Store
    \/ \E r \in Readers : Flag(r) \/ Build(r) \/ Return(r)

Spec == Init /\ [][Next]_vars

-----------------------------------------------------------------------------
(* A reader's snapshot never goes back: each holds at least what the one   *)
(* before it held (published_snapshots_never_go_back).                     *)
NeverBack == ~ back

(* A snapshot holds every write that returned before it was asked for.     *)
SeesReturnedWrites == ~ stale

=============================================================================
