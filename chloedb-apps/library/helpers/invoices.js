// title: Invoices
// description: Money arithmetic for invoices and quotes: line totals with discounts, subtotal, discount, tax, shipping and total to the cent, the due date from payment terms, and whether an invoice is paid, partly paid, unpaid or overdue.
// api: lineTotal({qty = 1, price, discountPct = 0}) -> qty × price less discountPct percent, rounded to the cent (null when price or qty is not a number);
//   totals(items, {taxRate = 0 (a percent, 8.25 for 8.25%), discount = 0 (an amount off the subtotal, at most the subtotal), shipping = 0 (not taxed)})
//   -> {subtotal, discount, tax, shipping, total}, each to the cent; items are lines as for lineTotal (lines that are not valid count as 0);
//   tax is taxRate percent of subtotal minus discount; total = subtotal - discount + tax + shipping;
//   dueDate(issued 'YYYY-MM-DD', terms = 'net 30') -> 'YYYY-MM-DD': 'net N' (any N, or a plain number N) is N days after issued,
//   'due on receipt' (or 'receipt', 'immediate') is issued, 'end of month' (or 'eom') is the last day of issued's month; null for unknown terms or a bad day;
//   status({due, paid = 0, total}, today = the local date) -> 'paid' when paid covers total (or total is 0), else 'overdue' when today is after due,
//   else 'partly paid' when something was paid, else 'unpaid'; due 'YYYY-MM-DD' or missing (never overdue); null when total is not a number
function helper() {
  const pad = (n) => String(n).padStart(2, '0');
  const num = (v) => (typeof v === 'number' ? v : typeof v === 'string' && v.trim() !== '' ? Number(v) : NaN);
  const isNum = (v) => Number.isFinite(v);
  // round a value already counted in cents to whole cents, halves away from zero
  const roundC = (c) => Math.sign(c) * Math.round(Math.abs(c) + 1e-7);
  // an amount in units of money to whole cents
  const cents = (n) => roundC(n * 100);
  const out = (c) => (c === 0 ? 0 : c / 100);
  const r2 = (n) => out(cents(n));
  function toUTC(d) {
    const y = Number(d.slice(0, 4)), m = Number(d.slice(5, 7)), day = Number(d.slice(8, 10));
    const ms = Date.UTC(y, m - 1, day);
    const t = new Date(ms);
    return t.getUTCMonth() === m - 1 && t.getUTCDate() === day ? ms : NaN;
  }
  const isDay = (d) => typeof d === 'string' && /^\d{4}-\d{2}-\d{2}$/.test(d.slice(0, 10)) && !isNaN(toUTC(d.slice(0, 10)));
  const fromUTC = (ms) => { const d = new Date(ms); return `${d.getUTCFullYear()}-${pad(d.getUTCMonth() + 1)}-${pad(d.getUTCDate())}`; };
  const localToday = () => { const d = new Date(); return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`; };

  function lineTotal(item) {
    if (!item || typeof item !== 'object') return null;
    const qty = item.qty == null ? 1 : num(item.qty);
    const price = num(item.price);
    const pct = item.discountPct == null ? 0 : num(item.discountPct);
    if (!isNum(qty) || !isNum(price) || !isNum(pct)) return null;
    const p = Math.min(Math.max(pct, 0), 100);
    return r2(qty * price * (100 - p) / 100);
  }

  function totals(items, { taxRate = 0, discount = 0, shipping = 0 } = {}) {
    let sub = 0;
    for (const it of Array.isArray(items) ? items : []) {
      const t = lineTotal(it);
      if (t != null) sub += cents(t);
    }
    const rate = isNum(num(taxRate)) ? Math.max(num(taxRate), 0) : 0;
    let off = isNum(num(discount)) ? cents(Math.max(num(discount), 0)) : 0;
    off = Math.min(off, Math.max(sub, 0));
    const ship = isNum(num(shipping)) ? cents(Math.max(num(shipping), 0)) : 0;
    const tax = roundC((sub - off) * rate / 100);
    return { subtotal: out(sub), discount: out(off), tax: out(tax), shipping: out(ship), total: out(sub - off + tax + ship) };
  }

  function dueDate(issued, terms = 'net 30') {
    if (!isDay(issued)) return null;
    const day = issued.slice(0, 10);
    const ms = toUTC(day);
    if (typeof terms === 'number') return isNum(terms) && terms >= 0 ? fromUTC(ms + Math.round(terms) * 86400000) : null;
    const t = String(terms == null ? '' : terms).trim().toLowerCase().replace(/[\s_-]+/g, ' ');
    if (/^(due )?(on|upon) receipt$|^receipt$|^immediate(ly)?$|^net 0$/.test(t)) return day;
    if (/^(end of (the )?month|eom)$/.test(t)) {
      const d = new Date(ms);
      return fromUTC(Date.UTC(d.getUTCFullYear(), d.getUTCMonth() + 1, 0));
    }
    const m = t.match(/^(?:net ?)?(\d{1,4})( days?)?$/);
    if (m) return fromUTC(ms + Number(m[1]) * 86400000);
    return null;
  }

  function status(invoice, today) {
    if (!invoice || typeof invoice !== 'object') return null;
    const total = num(invoice.total);
    if (!isNum(total)) return null;
    const paidN = invoice.paid == null ? 0 : num(invoice.paid);
    const paid = isNum(paidN) ? cents(paidN) : 0;
    const owed = cents(total);
    if (owed <= 0 || paid >= owed) return 'paid';
    const day = isDay(today) ? today.slice(0, 10) : localToday();
    if (isDay(invoice.due) && day > invoice.due.slice(0, 10)) return 'overdue';
    return paid > 0 ? 'partly paid' : 'unpaid';
  }

  return { lineTotal, totals, dueDate, status };
}
