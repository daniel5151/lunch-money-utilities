# Payroll Domain Notes

A field guide to the payroll mechanics this importer has to model: how each
backend lays out a payslip, how the tricky line items (imputed income, RSU
vests, gross-ups) actually behave, and how to set up categorization so a
paycheck reconciles cleanly. This is the *why* behind the code; the
[`README.md`](./README.md) is the *how* (commands, config keys, CLI).

> All examples use line-item **descriptions only** — no real dollar amounts.
> Everything here was inferred from real payslip corpora across three payroll
> providers and cross-checked against the per-page reconciliation tests.

---

## 1. The one invariant that governs everything

Every payslip page must satisfy:

```
earnings − employee_taxes − pre_tax_deductions − post_tax_deductions == net_pay
```

to within one cent. This is the single source of truth. The importer refuses to
write a split that doesn't reconcile, and the corpus tests assert it page by
page. Almost every "weird" rule below exists because some line item *appears* to
break this equation but actually doesn't once you understand what it represents.

Two recurring reasons a naive sum is wrong:

1. **Non-cash (imputed) income** inflates the earnings side without being paid
   in cash. Either the payslip prints an offsetting line (self-balancing), or it
   doesn't and the importer must inject one.
2. **RSU vests** show up as their own $0.00 paycheck where the entire gross is
   immediately consumed by taxes and offsets. These don't reconcile as a normal
   4-section paycheck and are handled by a separate reconstruction path.

---

## 2. Imputed income — the core tricky concept

**Imputed income** is the taxable *value* of a non-cash benefit. The classic
examples seen in the corpora:

- **Group-term life insurance** over the IRS exclusion threshold (the imputed
  cost of employer-paid coverage). Workday: `*Imp GTL`.
- **Relocation benefits** and their **tax gross-ups** — the employer pays moving
  costs and then pays *extra* to cover the tax on that benefit, and the whole
  thing is taxable wages. Workday: `*Relo Qualified`, `Relocation Tax Ben`.
- **Stock award income** — the fair-market value of vested shares, taxed as
  ordinary income even though no cash changes hands. Microsoft/ADP:
  `STOCK AWARD INCOME`.

The mental model: imputed income **raises your taxable gross and your withheld
taxes, but never lands in your bank account.** So on the paycheck it has to be
cancelled out somewhere, or the math won't tie to net pay.

There are exactly two ways a provider handles this, and **which one a provider
uses is the single most important fact for configuring it correctly.**

### Pattern A — provider prints both halves (self-reconciling)

The payslip shows the imputed item *and* an explicit equal-and-opposite offset.
The two cancel inline, so the page already reconciles. **Do not** add anything;
injecting an offset would double-count.

This is how **Microsoft** and **ADP-Microsoft** work:

```
STOCK AWARD INCOME            +X.XX     ← the imputed add-back
STOCK AWARD INCOME            −X.XX     ← the offset, printed right alongside
```

ADP does the same with its `… Offset` companion lines:

```
Stock Award Income            +X.XX
Stock Award Income Offset     −X.XX
Long Term Dis Imputed Inc     +X.XX
Long Term Dis Offset          −X.XX
```

### Pattern B — provider prints a one-sided add-back (needs injection)

The payslip lists the imputed item as a bare earnings line with **no** offset.
The page will *not* reconcile until the importer injects an equal opposite
offset for each such line. This is how **Workday** works, and it's the only
backend that needs offset injection.

```
*Imp GTL                       +X.XX    ← inflates gross, no cash, no offset printed
                                         → importer injects a −X.XX offset
```

### Why this maps to a per-backend flag

In code this is the `PayslipKind::injects_imputed_offsets()` seam: it returns
`true` only for Workday. The companion `is_imputed_income(description, extra)`
decides *which lines* are imputed, and is also a no-op (`false`) for the
self-reconciling providers — so even if a Microsoft line looked imputed, no
offset is ever injected for it. The config validator enforces this: setting
`imputed_income.descriptions` on a non-injecting backend is rejected, because it
would be silently ignored and give a false sense of configuration.

---

## 3. Per-backend reference

### 3a. Workday (e.g. Meta)

- **Extraction:** clean plain text; line-by-line state machine.
- **Imputed income:** Pattern B (one-sided add-backs → importer injects
  offsets).
- **Detection rule:** any description with a **leading `*`** is imputed
  (`*Imp GTL`, `*Relo Qualified`). Some imputed companions carry **no marker**
  and must be listed explicitly in `imputed_income.descriptions` (the relocation
  tax-benefit gross-ups, e.g. `Relocation Tax Ben`).
- **RSU vests:** Pattern — separate $0.00 paychecks (see §4). Requires
  `rsu_account` + `rsu_payee_match`.

**The relocation tangle (worked example of why care is needed).** A relocation
paycheck can carry three different-looking lines, and only *some* are imputed:

| Line | What it is | Imputed? |
|------|-----------|----------|
| `*Relo Qualified` | Taxable value of the qualified relocation benefit | **Yes** (marked `*`) |
| `Relocation Tax Ben` | The tax-benefit gross-up companion | **Yes** (unmarked → must be configured) |
| `Relocation Gross Up` | **Real cash** added to the check to fund the tax | **No** — it's actually paid |

The decisive corpus proof was a `$0.00` net-pay relocation page: a naive
`earnings − taxes − pre − post` came out off by *exactly* `*Relo Qualified` +
`Relocation Tax Ben`. `Relocation Gross Up` was **not** part of the discrepancy —
it's genuine cash funding the withholding, so it must stay on the earnings side.
Getting this wrong in either direction breaks reconciliation.

> **Naming gotcha (fixed):** the config field was once called `exceptions` with
> a comment saying those lines should *not* be treated as imputed — but the code
> (correctly) treated them as *additional* imputed lines. The behavior was
> right; the name and docs were backwards. It's now `descriptions`, meaning
> "extra imputed lines beyond the `*`-marked ones."

Other Workday line families seen: `Salary`, `Performance Bonus`,
`OneTime SignOn Payment`, `Dividend Equivalent` (on RSU dividend equivalents),
`Restricted Stock Units` (vest pages), `Pretax Medical/Dental/Vision`,
`Medical FSA`, `Life@ Choice`, and NY-specific taxes (`NY SDI / NYSDI`,
`New York Paid Family Leave / NYPFL`, `City Tax NY`, `State Tax NY`).

### 3b. Microsoft Corporation ("Official Copy")

- **Extraction:** single running column, CURRENT vs YEAR-TO-DATE inline;
  line items are bucketed by their **printed sign**.
- **Imputed income:** Pattern A (self-reconciling). Every `STOCK AWARD INCOME`
  appears as a matched `+X / −X` pair. **No injection, no config needed.**
- **RSU vests:** folded inline as offsetting line items — *not* separate $0
  paychecks. So **no** `rsu_account` / `rsu_payee_match`, and no RSU
  reconstruction path.
- **Quirk:** `pdf-extract` intermittently drops the entire earnings section on
  some pages; the backend has handling for that. Large multi-page Official Copy
  PDFs are **slow to parse in debug builds** — see §6.

### 3c. ADP-Microsoft (ADP-generated Microsoft statements)

- **Extraction:** two *physical* columns that `pdf-extract` interleaves into one
  text stream; the backend de-interleaves by **leading indentation** (right
  column starts with whitespace).
- **Imputed income:** Pattern A (self-reconciling) via explicit `… Offset`
  companion lines. **No injection, no config needed.**
- **The memo-block subtlety (important):** ADP prints some figures in an
  **"Other Benefits and Information"** block on the right that are *informational
  only* and must **not** enter the four reconciliation sections. The parser hard-
  stops the right column at `Other Benefits`, `Personal Time`, and
  `Federal taxable …`.
  - `Imputed Life Ins` lives **in that memo block** → it's genuinely one-sided
    but is structurally excluded from parsing, so it can't inflate gross. This
    is *why* ADP needs no imputed handling despite having a one-sided item on the
    page: the item simply isn't in the parsed sections.
  - Contrast with `Long Term Dis Imputed Inc`, which *is* in the parsed Benefits
    section but prints its `Long Term Dis Offset` companion → self-cancels.
- **`*401K` marker:** ADP prefixes some retirement lines with `*` ("Excluded
  from Federal Taxable Wages"). That asterisk is **stripped during parsing** to
  keep descriptions clean. Note this `*` means something completely different
  from Workday's `*` (imputed) — another reason imputed detection is per-backend,
  not global.
- **Detection vs Microsoft:** both say "MICROSOFT CORPORATION"; the ADP variant
  is told apart by the ADP footer or the `Period Beg/End:` header band.

---

## 4. RSU vests — the other reconstruction path

Workday (and Workday-like providers) book an RSU vest as a **separate $0.00
paycheck**: the full fair-market value lands as `Restricted Stock Units`
earnings and is immediately consumed by tax withholding and offsets, netting to
zero cash. This does **not** reconcile as a normal 4-section paycheck, so it's
routed through a dedicated *gross-comp-minus-taxes* reconstruction that matches
against the auto-imported $0.00 vest transaction in the linked `rsu_account`.

Detection: a page is an RSU vest if it has a `restricted stock`-style earning
with a **non-zero** current amount. Such pages are deliberately **skipped** by
the standard reconciliation invariant (§1) — they're expected not to satisfy it.

Microsoft/ADP do **not** use this path — they fold stock comp inline (Pattern A),
which is why those backends require no `rsu_account`.

### Suggested category setup for the RSU lifecycle

To keep the vest→sale story coherent in Lunch Money:

- **At vest:** recognize the share value as income and the withheld shares as
  tax; the net is a transfer of *shares* into a brokerage/asset account, not
  cash spending.
- **At sale:** treat it as an asset-to-cash conversion, not new income (the
  income was already recognized at vest). Only the gain/loss since vest is new.

Mapping every RSU-related line (`Restricted Stock Units`, `… RSU Tax Offset`,
`… RSU Excess Refund`, `Dividend Equivalent`) to deliberate categories avoids
double-counting equity comp as both income and spending.

---

## 5. Categorization setup — practical rules

- **Use the full, un-truncated description exactly as the parser emits it.** The
  `mapping` table keys on the literal line description; a near-miss silently
  falls through to the default.
- **Map by line family, not by paycheck.** Salary, bonus, sign-on, and dividend-
  equivalent lines often want different categories even though they're all
  "income."
- **Pre-tax vs post-tax matters for the math, not just the label.** Pre-tax
  deductions (`Pretax Medical/Dental/Vision`, `Medical FSA`, 401k) reduce taxable
  wages; they sit in a different reconciliation bucket than post-tax items.
- **Imputed lines and their offsets should share a category.** The injected
  offset carries the same category as the line it cancels, so net category impact
  is zero — which is correct, since no real money moved.
- **Don't configure `imputed_income.descriptions` on Microsoft/ADP.** It's
  rejected by validation on purpose (they self-reconcile). Only Workday uses it,
  and only for the *unmarked* imputed lines.

---

## 6. Operational gotchas

- **Run the corpus/parse tests in `--release`.** `pdf-extract` is dramatically
  slower unoptimized. The 78-page Microsoft Official Copy takes **5+ minutes in
  debug (effectively hangs) vs ~30s in release**; ADP/Workday corpora drop from
  ~8–12s to under a second. For reference, `pdftotext` does the same Microsoft
  file in ~0.3s — the cost is entirely `pdf-extract`'s decoding, not I/O.
- **Trailing-minus money tokens.** Some providers render credits with a
  *trailing* `-` (`1,234.50-`). The shared `clean_decimal` normalizes these to a
  leading sign; a token that still won't parse is surfaced as an error rather
  than silently becoming `0.00` (which would let a botched extraction sail
  through reconciliation undetected).
- **`pdf-extract` glues stray tokens onto rows.** Workday glues a row's period
  *start date* onto the description; ADP appends stray right-column numbers when
  the left column is blank. Each backend has targeted cleanup — be careful when
  touching it, and lean on the reconciliation tests to catch regressions.
- **Detection is best-effort.** `detect_kind` sniffs the first page for a
  provider fingerprint, but the result should still be cross-checked against the
  configured kind; two Microsoft formats share the same company string.

---

## 7. Quick mental checklist for a new provider or line item

1. Does this line represent **cash that hits the bank**, or a **non-cash taxable
   value**? Only cash belongs on the net-pay side.
2. If non-cash: does the payslip **print its own offset** (Pattern A → do
   nothing) or **not** (Pattern B → inject an offset, mark it imputed)?
3. Is there a **machine-readable marker** for imputed lines (Workday's `*`), or
   must some be listed by description?
4. Are RSU vests **separate $0 paychecks** (reconstruction path) or **folded
   inline** (normal reconciliation)?
5. Are there **informational/memo figures** that must be excluded from the four
   sections entirely (ADP's "Other Benefits and Information")?
6. After all that: does **`earnings − taxes − pre − post == net_pay`** hold to a
   cent? If not, one of the above is misclassified.
