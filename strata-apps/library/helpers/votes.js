// title: Votes
// description: Counts votes and picks winners (polls, team decisions, club elections, choosing a restaurant): plain and weighted tallies, ranked-choice instant runoff, approval voting and quorum checks.
// api: tally(votes) -> [{option, votes, pct}] most first, ties kept in the order they first appeared; votes is [option] or [{option, weight = 1}]
//   (options are non-empty strings, numbers or booleans; others and negative or non-numeric weights are left out); pct is the share of all votes, 0..100 to one decimal;
//   winner(tally) -> the option with the most votes, or null on a tie for first, when nothing has a vote, or for an empty list (a raw votes list is tallied first);
//   ranked(ballots) -> {rounds: [{counts: [{option, votes, pct}], exhausted, eliminated}], winner}: instant runoff over ballots [[first, second, ...]];
//   each round counts every ballot for its highest remaining choice (exhausted: ballots with none left; pct is of the ballots still counting);
//   a choice with more than half wins; otherwise the last one is eliminated (a tie for last goes to the one with fewer votes in the latest earlier round
//   that tells them apart, then the one that appeared later); winner null when every remaining choice ties, or when there are no ballots;
//   approval(ballots) -> [{option, votes, pct}] most first: each ballot [options] approves each option once; pct is the share of ballots approving it;
//   quorum(votes, members, pct = 50) -> true when votes reach at least pct percent of members (votes and members are numbers or arrays, counted by length;
//   false when there are no members)
function helper() {
  const valid = (o) => (typeof o === 'string' && o.trim() !== '') || (typeof o === 'number' && isFinite(o)) || typeof o === 'boolean';
  const share = (n, total) => (total > 0 ? Math.round((n / total) * 1000) / 10 : 0);

  // Map insertion order is first appearance; sort by votes, then by that order
  function rowsOf(counts, order, total) {
    return order
      .map((option, i) => ({ option, votes: counts.get(option), i }))
      .sort((a, b) => b.votes - a.votes || a.i - b.i)
      .map(({ option, votes }) => ({ option, votes, pct: share(votes, total) }));
  }

  function tally(votes) {
    const counts = new Map();
    let total = 0;
    for (const v of Array.isArray(votes) ? votes : []) {
      let option = v;
      let weight = 1;
      if (v && typeof v === 'object') {
        option = v.option;
        if (v.weight !== undefined && v.weight !== null) weight = v.weight;
      }
      if (!valid(option) || typeof weight !== 'number' || !isFinite(weight) || weight < 0) continue;
      counts.set(option, (counts.get(option) || 0) + weight);
      total += weight;
    }
    return rowsOf(counts, [...counts.keys()], total);
  }

  function winner(rows) {
    if (!Array.isArray(rows) || !rows.length) return null;
    const isTally = rows.every((r) => r && typeof r === 'object' && typeof r.votes === 'number');
    const list = isTally ? rows : tally(rows);
    if (!list.length) return null;
    let max = -Infinity;
    for (const r of list) if (r.votes > max) max = r.votes;
    if (!(max > 0)) return null;
    const top = list.filter((r) => r.votes === max);
    return top.length === 1 ? top[0].option : null;
  }

  function cleanBallot(b) {
    const seen = new Set();
    const out = [];
    for (const o of Array.isArray(b) ? b : [b]) {
      if (!valid(o) || seen.has(o)) continue;
      seen.add(o);
      out.push(o);
    }
    return out;
  }

  function ranked(ballots) {
    const list = (Array.isArray(ballots) ? ballots : []).map(cleanBallot).filter((b) => b.length);
    const order = [];
    const known = new Set();
    for (const b of list) for (const o of b) if (!known.has(o)) { known.add(o); order.push(o); }
    const active = new Set(order);
    const rounds = [];
    const history = [];
    let win = null;
    for (let guard = 0; active.size && guard <= order.length; guard++) {
      const counts = new Map();
      const names = order.filter((o) => active.has(o));
      for (const o of names) counts.set(o, 0);
      let exhausted = 0;
      for (const b of list) {
        const first = b.find((o) => active.has(o));
        if (first === undefined) exhausted++;
        else counts.set(first, counts.get(first) + 1);
      }
      const live = list.length - exhausted;
      const rows = rowsOf(counts, names, live);
      const round = { counts: rows, exhausted, eliminated: null };
      rounds.push(round);
      if (!live) break;
      const top = rows[0];
      if (top.votes * 2 > live || rows.length === 1) { win = top.option; break; }
      const low = rows[rows.length - 1].votes;
      if (top.votes === low) break; // every remaining choice ties
      let tied = rows.filter((r) => r.votes === low).map((r) => r.option);
      for (let k = history.length - 1; k >= 0 && tied.length > 1; k--) {
        const past = history[k];
        const min = Math.min(...tied.map((o) => past.get(o) || 0));
        tied = tied.filter((o) => (past.get(o) || 0) === min);
      }
      // still tied: the one that appeared later goes (tied keeps order of appearance)
      const out = tied[tied.length - 1];
      round.eliminated = out;
      active.delete(out);
      history.push(counts);
    }
    return { rounds, winner: win };
  }

  function approval(ballots) {
    const counts = new Map();
    let n = 0;
    for (const b of Array.isArray(ballots) ? ballots : []) {
      if (!Array.isArray(b)) continue;
      n++;
      for (const o of cleanBallot(b)) counts.set(o, (counts.get(o) || 0) + 1);
    }
    return rowsOf(counts, [...counts.keys()], n);
  }

  function quorum(votes, members, pct = 50) {
    const count = (x) => (Array.isArray(x) ? x.length : typeof x === 'number' && isFinite(x) ? x : 0);
    const v = count(votes);
    const m = count(members);
    const p = typeof pct === 'number' && isFinite(pct) ? pct : 50;
    if (!(m > 0)) return false;
    return v * 100 >= p * m;
  }

  return { tally, winner, ranked, approval, quorum };
}
