//! The blackboard ([`chloedb::blackboard`]): tasks claimed under leases,
//! findings and the timeline, on a tenant in a directory store.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chloedb::blackboard::{self, Board, BoardError, NewTask, CLAIMED, DONE, FAILED, OPEN};
use chloedb::storage::{DirStore, ObjectStore};
use chloedb::tenant::{Tenant, TenantError, TenantOptions};

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("chloedb-board-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn store(dir: &Path) -> Arc<dyn ObjectStore> {
    Arc::new(DirStore::new(dir.join("store")).unwrap())
}

fn node(dir: &Path, name: &str) -> TenantOptions {
    TenantOptions::new(dir.join(name))
}

fn new(title: &str) -> NewTask {
    NewTask { id: None, title: title.into(), spec: String::new() }
}

fn named(id: &str, title: &str) -> NewTask {
    NewTask { id: Some(id.into()), title: title.into(), spec: String::new() }
}

fn refused<T: std::fmt::Debug>(r: Result<T, BoardError>, part: &str) {
    match r {
        Err(BoardError::Refused(why)) => assert!(why.contains(part), "{why:?} does not say {part:?}"),
        other => panic!("not refused: {other:?}"),
    }
}

const S: i64 = 1_000_000;

#[test]
fn a_task_is_claimed_by_one_agent_and_done_once() {
    let dir = tmp("claims");
    let t = blackboard::create(store(&dir), "job", node(&dir, "a")).unwrap();
    let ann = Board::new(&t, "ann").unwrap().at(100 * S);
    let bob = Board::new(&t, "bob").unwrap().at(100 * S);

    let added = ann.add_tasks(&[new("port the parser"), named("lexer", "port the lexer"), new("port the printer")], None).unwrap();
    assert_eq!(added.added, ["t1", "lexer", "t2"]);
    // The same list again: the task with an id is not taken twice.
    let again = ann.add_tasks(&[named("lexer", "port the lexer")], None).unwrap();
    assert_eq!((again.added.len(), again.existing), (0, vec!["lexer".to_string()]));

    // Two of three to Ann, oldest first; the third to Bob; then nothing.
    let hers = ann.claim(2, 60, None).unwrap();
    assert_eq!(hers.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["t1", "lexer"]);
    assert!(hers.iter().all(|t| t.state == CLAIMED && t.owner.as_deref() == Some("ann") && t.attempts == 1 && t.lease_until == Some(160 * S)));
    let his = bob.claim(5, 60, None).unwrap();
    assert_eq!(his.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["t2"]);
    assert!(bob.claim(5, 60, None).unwrap().is_empty());

    // Each ends its own, not the other's.
    refused(bob.complete("t1", "done by bob", None), "claimed by ann");
    refused(bob.heartbeat("lexer", 60), "claimed by ann");
    refused(bob.complete("nothing", "", None), "no task");
    let done = ann.complete("t1", "parser ported", None).unwrap();
    assert_eq!((done.state.as_str(), done.result.as_deref(), done.lease_until), (DONE, Some("parser ported"), None));
    refused(ann.complete("t1", "again", None), "is done");
    assert_eq!(bob.fail("t2", "no printer here", None).unwrap().state, FAILED);

    let s = ann.status().unwrap();
    assert_eq!((s.tasks[OPEN], s.tasks[CLAIMED], s.tasks[DONE], s.tasks[FAILED], s.lapsed), (0, 1, 1, 1, 0));

    // The timeline, in order, and from a point on.
    let events = ann.timeline(0, 100).unwrap();
    let told: Vec<String> = events.iter().map(|e| format!("{} {} {}", e.agent, e.kind, e.about)).collect();
    assert_eq!(told, ["ann task t1", "ann task lexer", "ann task t2", "ann claim t1", "ann claim lexer", "bob claim t2", "ann done t1", "bob failed t2"]);
    assert_eq!(events.iter().map(|e| e.id).collect::<Vec<_>>(), (1..=8).collect::<Vec<_>>());
    assert_eq!(ann.timeline(6, 100).unwrap().len(), 2);
    assert_eq!(s.events, 8);

    // Another node reads the same board from the store.
    let other = Tenant::open(store(&dir), "job", node(&dir, "b")).unwrap();
    assert!(other.read(|db| Ok(blackboard::is_board(db))).unwrap());
    let carol = Board::new(&other, "carol").unwrap().at(100 * S);
    assert_eq!(carol.task("t1").unwrap().unwrap().state, DONE);
    assert_eq!(carol.timeline(0, 100).unwrap(), events);
}

#[test]
fn a_lease_that_passed_gives_the_task_to_another_and_refuses_the_first() {
    let dir = tmp("leases");
    let t = blackboard::create(store(&dir), "job", node(&dir, "a")).unwrap();
    let at = |agent: &str, s: i64| Board::new(&t, agent).unwrap().at(s * S);

    at("ann", 0).add_tasks(&[new("slow work")], None).unwrap();
    assert_eq!(at("ann", 0).claim(1, 30, None).unwrap()[0].lease_until, Some(30 * S));
    // Within the lease it is hers, and a heartbeat extends it.
    assert!(at("bob", 20).claim(1, 30, None).unwrap().is_empty());
    assert_eq!(at("ann", 20).heartbeat("t1", 30).unwrap().lease_until, Some(50 * S));
    assert!(at("bob", 40).claim(1, 30, None).unwrap().is_empty());
    assert_eq!(at("bob", 60).status().unwrap().lapsed, 1);
    // Past it, while nobody took the task, she may still end or extend it.
    assert_eq!(at("ann", 60).heartbeat("t1", 30).unwrap().lease_until, Some(90 * S));

    // Past it again, Bob claims the task: Ann's work is refused, his counts.
    let his = at("bob", 100).claim(1, 30, None).unwrap();
    assert_eq!((his[0].owner.as_deref(), his[0].attempts), (Some("bob"), 2));
    refused(at("ann", 101).complete("t1", "late", None), "claimed by bob");
    refused(at("ann", 101).heartbeat("t1", 30), "claimed by bob");
    assert_eq!(at("bob", 102).complete("t1", "in time", None).unwrap().result.as_deref(), Some("in time"));
    let last = at("bob", 102).timeline(0, 100).unwrap();
    assert!(last.iter().any(|e| e.kind == "claim" && e.body == "attempt 2, after the lease of ann passed"), "{last:?}");

    refused(at("ann", 0).claim(1, 0, None), "a lease is");
    refused(at("ann", 0).add_tasks(&[], None), "tasks at a time");
    refused(at("ann", 0).add_tasks(&[new(" ")], None), "title");
    assert!(Board::new(&t, "").is_err());
}

#[test]
fn a_call_sent_again_with_its_key_is_applied_once() {
    let dir = tmp("keys");
    let t = blackboard::create(store(&dir), "job", node(&dir, "a")).unwrap();
    let ann = Board::new(&t, "ann").unwrap().at(0);

    let first = ann.add_tasks(&[new("one"), new("two"), new("three")], Some("intake-1")).unwrap();
    assert_eq!(ann.add_tasks(&[new("one"), new("two"), new("three")], Some("intake-1")).unwrap(), first);
    assert_eq!(ann.status().unwrap().tasks[OPEN], 3);

    // A claim sent again answers the tasks it gave, and gives no more.
    let hers = ann.claim(1, 60, Some("c1")).unwrap();
    assert_eq!(ann.claim(1, 60, Some("c1")).unwrap(), hers);
    assert_eq!(ann.status().unwrap().tasks[CLAIMED], 1);
    // The same key under another call is another key.
    let done = ann.complete("t1", "ok", Some("c1")).unwrap();
    assert_eq!(ann.complete("t1", "ok", Some("c1")).unwrap(), done);
    refused(ann.complete("t1", "ok", Some("c2")), "is done");

    let posted = ann.post("the parser needs two passes", Some("t1"), Some("p1")).unwrap();
    assert_eq!(ann.post("the parser needs two passes", Some("t1"), Some("p1")).unwrap(), posted);
    let s = ann.status().unwrap();
    assert_eq!((s.findings, s.duplicates), (1, 0));
    // Without a key it is another finding, which says the same.
    assert_eq!(ann.post("the parser needs two passes", Some("t1"), None).unwrap().dup_of, Some(posted.id.clone()));

    // The keys are in the store with the rows: another node knows them.
    let other = Tenant::open(store(&dir), "job", node(&dir, "b")).unwrap();
    let ann2 = Board::new(&other, "ann").unwrap().at(5 * S);
    assert_eq!(ann2.claim(1, 60, Some("c1")).unwrap(), hers);
    assert_eq!(ann2.post("anything", None, Some("p1")).unwrap(), posted);
}

#[test]
fn findings_that_say_the_same_are_kept_as_duplicates_and_found_once() {
    let dir = tmp("findings");
    let t = blackboard::create(store(&dir), "job", node(&dir, "a")).unwrap();
    let ann = Board::new(&t, "ann").unwrap().at(0);
    let bob = Board::new(&t, "bob").unwrap().at(S);

    let first = ann.post("The build fails with a linker error on ARM64 in release mode.", None, None).unwrap();
    assert_eq!((first.id.as_str(), first.dup_of.clone()), ("f1", None));
    let same = bob.post("the build fails with a linker error on arm64, in release mode", Some("t9"), None).unwrap();
    assert_eq!(same.dup_of.as_deref(), Some("f1"));
    let other = bob.post("The tests pass on ARM64 once the cache is cleared.", None, None).unwrap();
    assert_eq!(other.dup_of, None);
    // A duplicate of a duplicate names the first.
    assert_eq!(ann.post("The build fails with a linker error on ARM64 in release mode!", None, None).unwrap().dup_of.as_deref(), Some("f1"));
    // Never the same, when asked so.
    assert_eq!(Board::new(&t, "ann").unwrap().same_at(1.1).post("The build fails with a linker error on ARM64 in release mode.", None, None).unwrap().dup_of, None);

    ann.add_tasks(&[NewTask { id: None, title: "fix the linker flags".into(), spec: "ARM64 release builds do not link".into() }], None).unwrap();
    let found = ann.search("linker error", 10).unwrap();
    let ids: Vec<&str> = found.findings.iter().map(|f| f.finding.id.as_str()).collect();
    assert_eq!(ids.iter().copied().collect::<BTreeSet<_>>(), BTreeSet::from(["f1", "f5"]), "{found:?}");
    assert_eq!(found.findings.iter().find(|f| f.finding.id == "f1").unwrap().duplicates, 2);
    assert_eq!(found.tasks.iter().map(|t| t.task.id.as_str()).collect::<Vec<_>>(), ["t1"]);
    assert!(found.findings.iter().all(|f| f.score > 0.0) && found.tasks[0].score > 0.0);
    assert!(ann.search("printer", 10).unwrap().findings.is_empty());
    refused(ann.search("?!", 10), "needs a word");
    // Words a query's grammar reads as operators are words here, and
    // words the text index does not keep find nothing.
    ann.search("NOT (linker OR \"error", 10).unwrap();
    let nothing = ann.search("the and of", 10);
    assert!(nothing.as_ref().is_ok_and(|f| f.findings.is_empty() && f.tasks.is_empty()), "{nothing:?}");
    assert_eq!(ann.post("The and of.", None, None).unwrap().dup_of, None);

    let s = ann.status().unwrap();
    assert_eq!((s.findings, s.duplicates), (6, 2));
}

#[test]
fn agents_on_two_nodes_claim_each_task_once() {
    let dir = tmp("two-nodes");
    let s = store(&dir);
    let a = blackboard::create(s.clone(), "job", node(&dir, "a")).unwrap();
    let tasks: Vec<NewTask> = (0..60).map(|i| new(&format!("task {i}"))).collect();
    Board::new(&a, "lead").unwrap().add_tasks(&tasks, None).unwrap();
    let b = Tenant::open(s.clone(), "job", node(&dir, "b")).unwrap();

    // Six agents, three on each node, claim two at a time and complete
    // what they claimed; a call that lost the tenant to the other node is
    // sent again with its key.
    let claimed: Vec<Vec<String>> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..6)
            .map(|w| {
                let tenant = if w % 2 == 0 { &a } else { &b };
                scope.spawn(move || {
                    let me = Board::new(tenant, &format!("w{w}")).unwrap();
                    let again = |call: &dyn Fn() -> Result<Vec<chloedb::blackboard::Task>, BoardError>| loop {
                        match call() {
                            Err(BoardError::Tenant(TenantError::NotWriter)) => continue,
                            other => break other.unwrap(),
                        }
                    };
                    let mut mine = Vec::new();
                    for round in 0.. {
                        let key = format!("w{w}-claim-{round}");
                        let got = again(&|| me.claim(2, 600, Some(&key)));
                        if got.is_empty() {
                            break;
                        }
                        for t in got {
                            let key = format!("w{w}-done-{}", t.id);
                            again(&|| me.complete(&t.id, "ok", Some(&key)).map(|t| vec![t]));
                            mine.push(t.id);
                        }
                    }
                    mine
                })
            })
            .collect();
        workers.into_iter().map(|w| w.join().unwrap()).collect()
    });
    let all: Vec<&String> = claimed.iter().flatten().collect();
    assert_eq!(all.len(), 60, "every task claimed");
    assert_eq!(all.iter().collect::<BTreeSet<_>>().len(), 60, "none of them twice");

    let c = Tenant::open(s, "job", node(&dir, "c")).unwrap();
    let board = Board::new(&c, "reader").unwrap();
    let st = board.status().unwrap();
    assert_eq!((st.tasks[OPEN], st.tasks[CLAIMED], st.tasks[DONE]), (0, 0, 60));
    // Sixty tasks taken in, claimed and done: each event once, in order.
    let mut events = Vec::new();
    while let Some(last) = Some(events.last().map_or(0, |e: &chloedb::blackboard::Event| e.id)) {
        let more = board.timeline(last, 50).unwrap();
        if more.is_empty() {
            break;
        }
        events.extend(more);
    }
    assert_eq!(events.len(), 180);
    assert_eq!(events.iter().map(|e| e.id).collect::<Vec<_>>(), (1..=180).collect::<Vec<_>>());
    for kind in ["claim", "done"] {
        assert_eq!(events.iter().filter(|e| e.kind == kind).map(|e| &e.about).collect::<BTreeSet<_>>().len(), 60, "{kind}");
    }
}
