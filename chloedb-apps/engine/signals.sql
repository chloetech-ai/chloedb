-- title: Use signals
-- description: Patterns in what people do in an app, counted where the events
-- are (sa_events): the engine reads them to suggest the next change, and to
-- measure whether a kept change answered its pattern. Each takes the app and
-- the least count that makes a pattern; a person is their account, or the
-- browser tab's session when nobody signed in.

-- Words searched for again and again: the words someone stopped at, not each
-- pause while typing them ("mi", then "milk" a moment later, counts once).
CREATE OR REPLACE FUNCTION sa_sig_searches(app TEXT DEFAULT 'x', least INT DEFAULT 3) RETURNS TABLE
  OPTIONS (description = 'Searches repeated: q, n, people, sessions, empty (times it found nothing), last')
  AS $$ SELECT q, COUNT(*) AS n, COUNT(DISTINCT who) AS people, COUNT(DISTINCT sess) AS sessions,
      SUM(CASE WHEN results = 0 THEN 1 ELSE 0 END) AS empty, MAX(ts) AS last
    FROM (SELECT q, results, ts, coalesce(person, session, 'someone') AS who, coalesce(session, person, 'someone') AS sess,
        lead(q) OVER (PARTITION BY coalesce(session, person, 'someone') ORDER BY ts) AS nq,
        lead(ts) OVER (PARTITION BY coalesce(session, person, 'someone') ORDER BY ts) AS nts
      FROM sa_events WHERE app = :app AND kind = 'action' AND detail LIKE 'search %' AND q IS NOT NULL) s
    WHERE length(q) >= 2 AND (nq IS NULL OR nq = q OR date_part('epoch', nts) - date_part('epoch', ts) >= 20
      OR NOT (nq LIKE q || '%' OR q LIKE nq || '%'))
    GROUP BY q HAVING COUNT(*) >= :least ORDER BY n DESC LIMIT 20 $$;

-- A filter set by hand again and again (status=done every visit).
CREATE OR REPLACE FUNCTION sa_sig_filters(app TEXT DEFAULT 'x', least INT DEFAULT 4) RETURNS TABLE
  OPTIONS (description = 'Filters set by hand again and again: q (key=value), n, people, sessions, empty, last')
  AS $$ SELECT q, COUNT(*) AS n, COUNT(DISTINCT coalesce(person, session, 'someone')) AS people,
      COUNT(DISTINCT coalesce(session, person, 'someone')) AS sessions, SUM(CASE WHEN results = 0 THEN 1 ELSE 0 END) AS empty, MAX(ts) AS last
    FROM sa_events WHERE app = :app AND kind = 'action' AND detail LIKE 'filter %' AND q IS NOT NULL
      AND NOT (q LIKE '%=' OR q LIKE '%=null' OR q LIKE '%=all' OR q LIKE '%=undefined')
    GROUP BY q HAVING COUNT(*) >= :least ORDER BY n DESC LIMIT 20 $$;

-- Looking for something and finding nothing: the rows are not there, or the
-- search does not look where they are.
CREATE OR REPLACE FUNCTION sa_sig_nothing(app TEXT DEFAULT 'x', least INT DEFAULT 2) RETURNS TABLE
  OPTIONS (description = 'Searches and filters that found nothing: what (search or filter), q, n, people, last')
  AS $$ SELECT CASE WHEN detail LIKE 'search %' THEN 'search' ELSE 'filter' END AS what, q, COUNT(*) AS n,
      COUNT(DISTINCT coalesce(person, session, 'someone')) AS people, MAX(ts) AS last
    FROM sa_events WHERE app = :app AND kind = 'action' AND results = 0 AND q IS NOT NULL AND length(q) >= 2
      AND (detail LIKE 'search %' OR detail LIKE 'filter %')
    GROUP BY what, q HAVING COUNT(*) >= :least ORDER BY n DESC LIMIT 20 $$;

-- The same write again and again in one sitting: the same fields changed on
-- row after row (kind 'update'), rows added one by one ('insert') or deleted
-- one by one ('delete'). A sitting is a browser tab's session.
CREATE OR REPLACE FUNCTION sa_sig_repeats(app TEXT DEFAULT 'x', kind TEXT DEFAULT 'update', least INT DEFAULT 5) RETURNS TABLE
  OPTIONS (description = 'Writes repeated row by row in a sitting: tbl, fields, n (rows), sittings, most (rows in one sitting), people, last')
  AS $$ SELECT tbl, fields, SUM(rows) AS n, COUNT(*) AS sittings, MAX(rows) AS most, COUNT(DISTINCT who) AS people, MAX(last) AS last
    FROM (SELECT tbl, coalesce(fields, '') AS fields, coalesce(session, person, 'someone') AS sess, coalesce(person, session, 'someone') AS who,
        COUNT(DISTINCT rid) AS rows, MAX(ts) AS last
      FROM sa_events WHERE app = :app AND kind = :kind AND tbl IS NOT NULL AND rid IS NOT NULL
      GROUP BY tbl, coalesce(fields, ''), coalesce(session, person, 'someone'), coalesce(person, session, 'someone')
      HAVING COUNT(DISTINCT rid) >= :least) s
    GROUP BY tbl, fields ORDER BY n DESC LIMIT 20 $$;

-- Rows added and deleted again within ten minutes: an add that went wrong.
CREATE OR REPLACE FUNCTION sa_sig_undone(app TEXT DEFAULT 'x', least INT DEFAULT 3) RETURNS TABLE
  OPTIONS (description = 'Rows deleted within ten minutes of being added: tbl, n, people, last')
  AS $$ SELECT i.tbl AS tbl, COUNT(*) AS n, COUNT(DISTINCT coalesce(i.person, i.session, 'someone')) AS people, MAX(d.ts) AS last
    FROM sa_events i JOIN sa_events d ON d.rid = i.rid AND d.tbl = i.tbl
    WHERE i.app = :app AND d.app = :app AND i.kind = 'insert' AND d.kind = 'delete'
      AND date_part('epoch', d.ts) - date_part('epoch', i.ts) >= 0 AND date_part('epoch', d.ts) - date_part('epoch', i.ts) < 600
    GROUP BY i.tbl HAVING COUNT(*) >= :least ORDER BY n DESC LIMIT 20 $$;

-- Forms typed into and closed unsaved.
CREATE OR REPLACE FUNCTION sa_sig_abandoned(app TEXT DEFAULT 'x', least INT DEFAULT 3) RETURNS TABLE
  OPTIONS (description = 'Forms given up: screen, form (table and new or edit), tbl, n, people, ms (average time typing), last')
  AS $$ SELECT coalesce(screen, '') AS screen, detail AS form, tbl, COUNT(*) AS n,
      COUNT(DISTINCT coalesce(person, session, 'someone')) AS people, AVG(ms) AS ms, MAX(ts) AS last
    FROM sa_events WHERE app = :app AND kind = 'abandon'
    GROUP BY coalesce(screen, ''), detail, tbl HAVING COUNT(*) >= :least ORDER BY n DESC LIMIT 20 $$;

-- One screen opened right after another (within 30 seconds), again and again.
CREATE OR REPLACE FUNCTION sa_sig_paths(app TEXT DEFAULT 'x', least INT DEFAULT 4) RETURNS TABLE
  OPTIONS (description = 'Screens opened one right after the other: a, b, n, people, last')
  AS $$ SELECT prev AS a, detail AS b, COUNT(*) AS n, COUNT(DISTINCT who) AS people, MAX(ts) AS last
    FROM (SELECT detail, ts, coalesce(person, session, 'someone') AS who,
        lag(detail) OVER (PARTITION BY coalesce(session, person, 'someone') ORDER BY ts) AS prev,
        lag(ts) OVER (PARTITION BY coalesce(session, person, 'someone') ORDER BY ts) AS pts
      FROM sa_events WHERE app = :app AND kind = 'open') s
    WHERE prev IS NOT NULL AND prev <> detail AND date_part('epoch', ts) - date_part('epoch', pts) < 30
    GROUP BY prev, detail HAVING COUNT(*) >= :least ORDER BY n DESC LIMIT 20 $$;

-- A function run by hand again and again.
CREATE OR REPLACE FUNCTION sa_sig_calls(app TEXT DEFAULT 'x', least INT DEFAULT 5) RETURNS TABLE
  OPTIONS (description = 'Functions run by hand: fn, n, people, last')
  AS $$ SELECT detail AS fn, COUNT(*) AS n, COUNT(DISTINCT coalesce(person, session, 'someone')) AS people, MAX(ts) AS last
    FROM sa_events WHERE app = :app AND kind = 'call'
    GROUP BY detail HAVING COUNT(*) >= :least ORDER BY n DESC LIMIT 20 $$;

-- Where the time goes: each screen's visits and the time spent on it.
CREATE OR REPLACE FUNCTION sa_use_screens(app TEXT DEFAULT 'x') RETURNS TABLE
  OPTIONS (description = 'Time on each screen: screen, visits, people, ms (in all), avg_ms')
  AS $$ SELECT detail AS screen, COUNT(*) AS visits, COUNT(DISTINCT coalesce(person, session, 'someone')) AS people,
      SUM(ms) AS ms, AVG(ms) AS avg_ms
    FROM sa_events WHERE app = :app AND kind = 'leave' AND ms IS NOT NULL
    GROUP BY detail ORDER BY ms DESC LIMIT 30 $$;
