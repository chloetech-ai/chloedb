// title: Loans and savings
// description: Money over time, to the cent: the monthly payment on a loan or mortgage, its amortization schedule with optional extra payments, how savings grow with compound interest and deposits, how long it takes to reach a savings goal, and APR and effective annual rates.
// api: payment(principal, annualRatePct, months) -> the monthly payment to the cent (principal / months at 0%), or null for bad input;
//   schedule(principal, annualRatePct, months, {extra = 0}) -> {payment, months, totalPaid, totalInterest, rows: [{month, payment, interest, principal, balance}]}
//   or null; extra is added to every payment, so the loan may end early; the last row pays off what is left, so balance ends at 0;
//   compound(principal, annualRatePct, years, {perYear = 12, deposit = 0}) -> the future value to the cent, with interest compounded perYear times a year
//   and deposit added at the end of each compounding period;
//   monthsToGoal(goal, monthly, annualRatePct = 0, start = 0) -> whole months until savings starting at start, with monthly added each month and
//   interest credited monthly, reach goal (0 when start already does), or null if they never do;
//   apr(principal, payment, months, {fees = 0}) -> the annual percentage rate, rounded to 2 places, at which months payments repay principal less fees
//   (0 when they add up to it exactly, null when they add up to less or input is bad);
//   effectiveRate(annualRatePct, perYear = 12) -> the effective annual rate (APY) in percent, rounded to 2 places;
//   rates are yearly percents (6 means 6%); months are rounded to whole months
function helper() {
  const ok = (x) => typeof x === 'number' && isFinite(x);
  const cents = (x) => Math.round((x + (x >= 0 ? 1e-9 : -1e-9)) * 100) / 100 + 0;
  const raw = (p, r, n) => (r === 0 ? p / n : (p * r) / (1 - Math.pow(1 + r, -n)));

  function payment(principal, annualRatePct, months) {
    if (!ok(principal) || !ok(annualRatePct) || !ok(months) || principal < 0) return null;
    const n = Math.round(months);
    const r = annualRatePct / 1200;
    if (n < 1 || r <= -1) return null;
    return cents(raw(principal, r, n));
  }

  function schedule(principal, annualRatePct, months, opts) {
    const pay = payment(principal, annualRatePct, months);
    if (pay === null) return null;
    const extra = opts && ok(opts.extra) && opts.extra > 0 ? opts.extra : 0;
    const n = Math.round(months);
    const r = annualRatePct / 1200;
    const rows = [];
    let balance = cents(principal);
    let paid = 0;
    let interestSum = 0;
    for (let month = 1; balance > 0 && month <= n; month++) {
      const interest = cents(balance * r);
      let part = cents(pay + extra - interest);
      if (month === n || part >= balance) part = balance;
      const total = cents(interest + part);
      balance = cents(balance - part);
      rows.push({ month, payment: total, interest, principal: part, balance });
      paid += total;
      interestSum += interest;
    }
    return { payment: pay, months: rows.length, totalPaid: cents(paid), totalInterest: cents(interestSum), rows };
  }

  function compound(principal, annualRatePct, years, opts) {
    const perYear = opts && opts.perYear !== undefined ? opts.perYear : 12;
    const deposit = opts && opts.deposit !== undefined ? opts.deposit : 0;
    if (![principal, annualRatePct, years, perYear, deposit].every(ok) || years < 0 || perYear < 1) return null;
    const k = Math.round(perYear);
    const i = annualRatePct / 100 / k;
    if (i <= -1) return null;
    const n = years * k;
    const g = Math.pow(1 + i, n);
    return cents(principal * g + (i === 0 ? deposit * n : (deposit * (g - 1)) / i));
  }

  function monthsToGoal(goal, monthly, annualRatePct = 0, start = 0) {
    if (![goal, monthly, annualRatePct, start].every(ok)) return null;
    if (start >= goal) return 0;
    const r = annualRatePct / 1200;
    let balance = start;
    for (let m = 1; m <= 12000; m++) {
      const next = cents(balance + cents(balance * r) + monthly);
      if (next >= goal) return m;
      if (next <= balance) return null; // not growing: it never will
      balance = next;
    }
    return null;
  }

  function apr(principal, pay, months, opts) {
    const fees = opts && ok(opts.fees) ? opts.fees : 0;
    if (!ok(principal) || !ok(pay) || !ok(months)) return null;
    const n = Math.round(months);
    const amount = principal - fees;
    if (n < 1 || amount <= 0 || pay <= 0) return null;
    const total = pay * n;
    if (Math.abs(total - amount) < 1e-9) return 0;
    if (total < amount) return null;
    const pv = (r) => (pay * (1 - Math.pow(1 + r, -n))) / r;
    let lo = 0;
    let hi = 1;
    while (pv(hi) > amount && hi < 1e6) hi *= 2;
    if (pv(hi) > amount) return null;
    for (let k = 0; k < 200; k++) {
      const mid = (lo + hi) / 2;
      if (pv(mid) > amount) lo = mid; else hi = mid;
    }
    return cents(((lo + hi) / 2) * 1200);
  }

  function effectiveRate(annualRatePct, perYear = 12) {
    if (!ok(annualRatePct) || !ok(perYear) || perYear < 1) return null;
    const k = Math.round(perYear);
    const i = annualRatePct / 100 / k;
    if (i <= -1) return null;
    return cents((Math.pow(1 + i, k) - 1) * 100);
  }

  return { payment, schedule, compound, monthsToGoal, apr, effectiveRate };
}
