-- title: Measures
-- description: Everyday scores and measures in SQL: stock state and reorder amounts, letter grades, running pace and predicted race times, one-rep max, heart-rate zones, body mass index, fair star ratings and SM-2 review intervals for flashcards.

CREATE OR REPLACE FUNCTION stock_state(qty FLOAT, reorder_at FLOAT DEFAULT 0, max_qty FLOAT DEFAULT NULL) RETURNS KEYWORD
  OPTIONS (description = '''out'' (qty <= 0), ''low'' (qty <= reorder_at), ''over'' (qty > max_qty) or ''ok''')
  AS $$ CASE WHEN :qty IS NULL THEN NULL WHEN :qty <= 0 THEN 'out' WHEN :qty <= coalesce(:reorder_at, 0) THEN 'low'
        WHEN :max_qty IS NOT NULL AND :qty > :max_qty THEN 'over' ELSE 'ok' END $$;

CREATE OR REPLACE FUNCTION reorder_qty(qty FLOAT, reorder_at FLOAT, target FLOAT, pack FLOAT DEFAULT 1) RETURNS FLOAT
  OPTIONS (description = 'How many to order to bring qty up to target, in whole packs; 0 while qty is above reorder_at')
  AS $$ CASE WHEN :qty > :reorder_at THEN 0 ELSE round((greatest(:target, :reorder_at + 1) - :qty) / :pack + 0.4999999) * :pack END $$;

CREATE OR REPLACE FUNCTION letter_grade(pct FLOAT) RETURNS KEYWORD
  OPTIONS (description = 'A, B, C, D or F from a percent (90, 80, 70, 60)')
  AS $$ CASE WHEN :pct IS NULL THEN NULL WHEN :pct >= 90 THEN 'A' WHEN :pct >= 80 THEN 'B' WHEN :pct >= 70 THEN 'C' WHEN :pct >= 60 THEN 'D' ELSE 'F' END $$;

CREATE OR REPLACE FUNCTION pace_secs(km FLOAT, secs FLOAT) RETURNS INT
  OPTIONS (description = 'Seconds per km for a distance run in secs seconds; NULL when km is not above 0')
  AS $$ CASE WHEN :km IS NULL OR :km <= 0 THEN NULL ELSE CAST(round(:secs / :km) AS INT) END $$;

CREATE OR REPLACE FUNCTION race_time(km FLOAT, secs FLOAT, target_km FLOAT) RETURNS INT
  OPTIONS (description = 'Predicted seconds for target_km from a run of km in secs (Riegel, exponent 1.06)')
  AS $$ CASE WHEN :km IS NULL OR :km <= 0 THEN NULL ELSE CAST(round(:secs * pow(:target_km / :km, 1.06)) AS INT) END $$;

CREATE OR REPLACE FUNCTION one_rep_max(weight FLOAT, reps INT) RETURNS FLOAT
  OPTIONS (description = 'Estimated one-rep max (Epley), to 0.1')
  AS $$ CASE WHEN :reps IS NULL OR :reps < 1 THEN NULL WHEN :reps = 1 THEN :weight ELSE round(10 * :weight * (1 + :reps / 30.0)) / 10 END $$;

CREATE OR REPLACE FUNCTION hr_zone(bpm FLOAT, max_hr FLOAT) RETURNS INT
  OPTIONS (description = 'Heart-rate zone 1 to 5 of bpm against max_hr (below 60, 70, 80, 90 percent, and above); 0 below 50 percent')
  AS $$ CASE WHEN :bpm IS NULL OR :max_hr IS NULL OR :max_hr <= 0 THEN NULL WHEN :bpm < 0.5 * :max_hr THEN 0
        WHEN :bpm < 0.6 * :max_hr THEN 1 WHEN :bpm < 0.7 * :max_hr THEN 2 WHEN :bpm < 0.8 * :max_hr THEN 3 WHEN :bpm < 0.9 * :max_hr THEN 4 ELSE 5 END $$;

CREATE OR REPLACE FUNCTION bmi(kg FLOAT, cm FLOAT) RETURNS FLOAT
  OPTIONS (description = 'Body mass index, one decimal')
  AS $$ CASE WHEN :cm IS NULL OR :cm <= 0 THEN NULL ELSE round(10 * :kg / pow(:cm / 100, 2)) / 10 END $$;

CREATE OR REPLACE FUNCTION fair_rating(avg FLOAT, n INT, prior FLOAT DEFAULT 3.5, weight INT DEFAULT 5) RETURNS FLOAT
  OPTIONS (description = 'A star rating that few votes cannot inflate: the average pulled toward prior as if weight votes of prior were added, two decimals')
  AS $$ round(100 * (coalesce(:avg, 0) * coalesce(:n, 0) + :prior * :weight) / (coalesce(:n, 0) + :weight)) / 100 $$;

CREATE OR REPLACE FUNCTION sm2_ease(ease FLOAT, quality INT) RETURNS FLOAT
  OPTIONS (description = 'The next ease factor by SM-2 from the last one (2.5 for a new card) and how well it was remembered (0 to 5), never below 1.3')
  AS $$ greatest(1.3, round(100 * (coalesce(:ease, 2.5) + 0.1 - (5 - :quality) * (0.08 + (5 - :quality) * 0.02))) / 100) $$;

CREATE OR REPLACE FUNCTION sm2_interval(interval_days INT, reps INT, ease FLOAT, quality INT) RETURNS INT
  OPTIONS (description = 'Days until the next review by SM-2: 1 after a lapse (quality below 3) or a first review, 6 after a second, else interval times ease')
  AS $$ CASE WHEN :quality < 3 OR coalesce(:reps, 0) = 0 THEN 1 WHEN :reps = 1 THEN 6 ELSE CAST(round(:interval_days * coalesce(:ease, 2.5)) AS INT) END $$;
