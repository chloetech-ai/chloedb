-- title: Days and deadlines
-- description: Calendar days in SQL, on 'YYYY-MM-DD' text (or ISO times): days between two days, days left to a due date and its state, a day some days later. Pass today from the screen (:today).

CREATE OR REPLACE FUNCTION days_between(a TEXT, b TEXT) RETURNS INT
  OPTIONS (description = 'Whole days from day a to day b (negative when b is before a); NULL when either is missing')
  AS $$ CASE WHEN :a IS NULL OR :b IS NULL THEN NULL
        ELSE CAST((date_part('epoch', date_trunc('day', :b)) - date_part('epoch', date_trunc('day', :a))) / 86400 AS INT) END $$;

CREATE OR REPLACE FUNCTION days_left(due TEXT, today TEXT) RETURNS INT
  OPTIONS (description = 'Whole days from today to due: 0 on the day, negative when overdue; NULL without a due date')
  AS $$ days_between(:today, :due) $$;

CREATE OR REPLACE FUNCTION due_state(due TEXT, today TEXT, soon INT DEFAULT 3, done BOOL DEFAULT FALSE) RETURNS KEYWORD
  OPTIONS (description = '''done'', ''overdue'', ''today'', ''soon'' (within soon days) or ''later''; NULL without a due date')
  AS $$ CASE WHEN :done THEN 'done' WHEN :due IS NULL THEN NULL
        WHEN days_between(:today, :due) < 0 THEN 'overdue' WHEN days_between(:today, :due) = 0 THEN 'today'
        WHEN days_between(:today, :due) <= :soon THEN 'soon' ELSE 'later' END $$;

CREATE OR REPLACE FUNCTION add_days(day TEXT, n INT) RETURNS TEXT
  OPTIONS (description = 'The day n days after day (n negative goes back), as YYYY-MM-DD')
  AS $$ strftime('%Y-%m-%d', date_trunc('day', :day) + :n * 86400000000) $$;

CREATE OR REPLACE FUNCTION days_from(start TEXT DEFAULT '2026-01-01', n INT DEFAULT 7) RETURNS TABLE
  OPTIONS (description = 'n days in a row from start, one row each: day (YYYY-MM-DD), dow (0 Sunday to 6 Saturday) and i (0, 1, ...): join them to rows by day for a calendar or a chart with no gaps')
  AS $$ SELECT id AS i, strftime('%Y-%m-%d', date_trunc('day', :start) + id * 86400000000) AS day,
    date_part('dow', date_trunc('day', :start) + id * 86400000000) AS dow
    FROM sa_numbers WHERE id < :n ORDER BY id LIMIT 2000 $$;
