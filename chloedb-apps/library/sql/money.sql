-- title: Money
-- description: Money arithmetic in SQL, to the cent: prices with tax, discounts and tips, invoice lines, loan payments, budgets left and their state. Call them in any query: SELECT name, with_tax(price, 8.5) AS total FROM items.

CREATE OR REPLACE FUNCTION cents(amount FLOAT) RETURNS FLOAT
  OPTIONS (description = 'The amount rounded to the cent')
  AS $$ round(:amount * 100) / 100 $$;

CREATE OR REPLACE FUNCTION with_tax(amount FLOAT, rate_pct FLOAT) RETURNS FLOAT
  OPTIONS (description = 'The amount with rate_pct percent of tax added, to the cent')
  AS $$ round(:amount * (100 + coalesce(:rate_pct, 0))) / 100 $$;

CREATE OR REPLACE FUNCTION tax_of(amount FLOAT, rate_pct FLOAT) RETURNS FLOAT
  OPTIONS (description = 'The tax on the amount at rate_pct percent, to the cent')
  AS $$ round(:amount * coalesce(:rate_pct, 0)) / 100 $$;

CREATE OR REPLACE FUNCTION line_total(qty FLOAT, price FLOAT, discount_pct FLOAT DEFAULT 0) RETURNS FLOAT
  OPTIONS (description = 'qty times price less discount_pct percent, to the cent (an invoice or order line)')
  AS $$ round(coalesce(:qty, 0) * coalesce(:price, 0) * (100 - coalesce(:discount_pct, 0))) / 100 $$;

CREATE OR REPLACE FUNCTION tip(amount FLOAT, pct FLOAT) RETURNS FLOAT
  OPTIONS (description = 'A tip of pct percent on the amount, to the cent')
  AS $$ round(:amount * :pct) / 100 $$;

CREATE OR REPLACE FUNCTION loan_payment(principal FLOAT, annual_rate_pct FLOAT, months INT) RETURNS FLOAT
  OPTIONS (description = 'The monthly payment on a loan or mortgage, to the cent (principal / months at 0%)')
  AS $$ CASE WHEN :months IS NULL OR :months <= 0 OR :principal IS NULL THEN NULL
        WHEN coalesce(:annual_rate_pct, 0) = 0 THEN round(:principal * 100 / :months) / 100
        ELSE round(100 * :principal * (:annual_rate_pct / 1200) / (1 - pow(1 + :annual_rate_pct / 1200, -:months))) / 100 END $$;

CREATE OR REPLACE FUNCTION loan_balance(principal FLOAT, annual_rate_pct FLOAT, months INT, paid INT) RETURNS FLOAT
  OPTIONS (description = 'What is left to pay on a loan after paid monthly payments, to the cent')
  AS $$ CASE WHEN coalesce(:annual_rate_pct, 0) = 0 THEN round(100 * :principal * (1 - :paid / CAST(:months AS FLOAT))) / 100
        ELSE round(100 * (:principal * pow(1 + :annual_rate_pct / 1200, :paid)
          - loan_payment(:principal, :annual_rate_pct, :months) * (pow(1 + :annual_rate_pct / 1200, :paid) - 1) / (:annual_rate_pct / 1200))) / 100 END $$;

CREATE OR REPLACE FUNCTION budget_left(budget FLOAT, spent FLOAT) RETURNS FLOAT
  OPTIONS (description = 'budget - spent to the cent (negative when over); a missing spent counts as 0')
  AS $$ round(100 * (:budget - coalesce(:spent, 0))) / 100 $$;

CREATE OR REPLACE FUNCTION budget_state(budget FLOAT, spent FLOAT, near FLOAT DEFAULT 0.8) RETURNS KEYWORD
  OPTIONS (description = '''over'' when spent > budget, ''near'' when spent >= near * budget, else ''ok''; NULL without a budget')
  AS $$ CASE WHEN :budget IS NULL THEN NULL WHEN coalesce(:spent, 0) > :budget THEN 'over'
        WHEN coalesce(:spent, 0) >= :near * :budget THEN 'near' ELSE 'ok' END $$;

CREATE OR REPLACE FUNCTION pct_of(part FLOAT, whole FLOAT) RETURNS FLOAT
  OPTIONS (description = 'part as a percent of whole, one decimal; NULL when whole is 0 or missing')
  AS $$ CASE WHEN :whole IS NULL OR :whole = 0 THEN NULL ELSE round(1000 * coalesce(:part, 0) / :whole) / 10 END $$;

CREATE OR REPLACE FUNCTION amortization(principal FLOAT, annual_rate_pct FLOAT, months INT) RETURNS TABLE
  OPTIONS (description = 'A loan''s schedule, one row per month: month, payment, interest, principal_paid and balance after it')
  AS $$ SELECT id AS month, loan_payment(:principal, :annual_rate_pct, :months) AS payment,
    round(100 * loan_balance(:principal, :annual_rate_pct, :months, id - 1) * coalesce(:annual_rate_pct, 0) / 1200) / 100 AS interest,
    round(100 * (loan_payment(:principal, :annual_rate_pct, :months) - loan_balance(:principal, :annual_rate_pct, :months, id - 1) * coalesce(:annual_rate_pct, 0) / 1200)) / 100 AS principal_paid,
    CASE WHEN id = :months THEN 0 ELSE greatest(0, loan_balance(:principal, :annual_rate_pct, :months, id)) END AS balance
    FROM sa_numbers WHERE id >= 1 AND id <= :months ORDER BY id LIMIT 2000 $$;
