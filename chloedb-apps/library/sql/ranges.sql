-- title: Bands and overlaps
-- description: Numbers put in bands, and spans that overlap, in SQL: how many days old something is ('1-30', '31-60', '61-90', '90+'), a score or an age in its range, and whether two bookings of the same thing take the same days or the same hour. Bands sort with band_rank; a booking rule refuses a clash with a REQUIRE on days_overlap or times_overlap.

CREATE OR REPLACE FUNCTION band_of(n FLOAT, a FLOAT DEFAULT 30, b FLOAT DEFAULT 60, c FLOAT DEFAULT 90) RETURNS KEYWORD
  OPTIONS (description = 'The band n falls in: ''current'' at 0 or below, then ''1-a'', ''a+1-b'', ''b+1-c'' and ''c+'' (with the defaults ''1-30'', ''31-60'', ''61-90'', ''90+''): days open or late, ages, scores; NULL when n is missing. GROUP BY it with SUM for a total per band')
  AS $$ CASE WHEN :n IS NULL THEN NULL WHEN :n <= 0 THEN 'current'
        WHEN :n <= :a THEN '1-' || CAST(CAST(:a AS INT) AS TEXT)
        WHEN :n <= :b THEN CAST(CAST(:a AS INT) + 1 AS TEXT) || '-' || CAST(CAST(:b AS INT) AS TEXT)
        WHEN :n <= :c THEN CAST(CAST(:b AS INT) + 1 AS TEXT) || '-' || CAST(CAST(:c AS INT) AS TEXT)
        ELSE CAST(CAST(:c AS INT) AS TEXT) || '+' END $$;

CREATE OR REPLACE FUNCTION band_rank(n FLOAT, a FLOAT DEFAULT 30, b FLOAT DEFAULT 60, c FLOAT DEFAULT 90) RETURNS INT
  OPTIONS (description = 'The order of band_of(n, a, b, c): 0 current, 1, 2, 3, 4 for the last band; ORDER BY it (MIN(band_rank(...)) in a GROUP BY) so the bands read in order')
  AS $$ CASE WHEN :n IS NULL THEN NULL WHEN :n <= 0 THEN 0 WHEN :n <= :a THEN 1 WHEN :n <= :b THEN 2 WHEN :n <= :c THEN 3 ELSE 4 END $$;

CREATE OR REPLACE FUNCTION days_overlap(a_start TEXT, a_end TEXT, b_start TEXT, b_end TEXT) RETURNS BOOL
  OPTIONS (description = 'Whether two spans of days overlap (YYYY-MM-DD; each end is the day after the last one, as a checkout day: a stay ending on the 5th and one starting on the 5th do not clash); a missing end is open: a span with no end yet still runs')
  AS $$ (days_between(:b_start, COALESCE(:a_end, '2999-12-31')) > 0) AND (days_between(:a_start, COALESCE(:b_end, '2999-12-31')) > 0) $$;

CREATE OR REPLACE FUNCTION times_overlap(a_start TEXT, a_end TEXT, b_start TEXT, b_end TEXT) RETURNS BOOL
  OPTIONS (description = 'Whether two spans of time overlap, to the minute (ISO times such as 2026-11-20T09:30; one ending at 10:00 and one starting at 10:00 do not clash): a court, a van, a tutor or a person booked twice')
  AS $$ (date_part('epoch', date_trunc('minute', :a_start)) < date_part('epoch', date_trunc('minute', :b_end)))
        AND (date_part('epoch', date_trunc('minute', :b_start)) < date_part('epoch', date_trunc('minute', :a_end))) $$;
