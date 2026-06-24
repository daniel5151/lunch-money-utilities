# `lm-payslip-importer`

Imports direct deposits and RSU vests from payslips into granular transaction lines in Lunch Money.

The code may eventually support numerous payslip formats, but at the moment, it's primarily being tested with Workday payslips (specifically, Meta payslips).

---

## ⚡ Key Features

- **PDF Extraction**: Automatically parses payslip PDFs (utilizing `pdf-extract`) and groups lines into Earnings, Pre-Tax Deductions, Employee Taxes, and Post-Tax Deductions.
- **Granular Import Mapping**: Translates payslip items (e.g., "Salary", "Medical FSA", "Federal Withholding") to active categories in your Lunch Money account using a custom configuration map.
- **Interactive Configuration Wizard (`init`)**: Walkthrough wizard that fetches active accounts and categories from the Lunch Money API, lets you select target accounts, parses your payslips to identify unique line descriptions, and writes the configuration file automatically.
- **RSU Vest Matching**: Detects restricted stock earnings from your payslip, automatically creates a synthetic parent transaction in your RSU manual account (e.g., Equity Awards), and splits it between gross stock value (credit) and tax withholdings (debits).
- **Net-Zero Reconciliation**:
  - Automatically matches direct deposit transactions in Lunch Money within a 3-day window of the check date.
  - For zero-dollar checks (e.g., relocation benefits or stock vests where cash flow is $0.00), it creates a synthetic `$0.00` transaction in the configured `net_zero_account` (e.g., Checking account) before executing the split.
- **Imputed Income Offsetting**:
  - Handles non-cash, taxable benefits (e.g., Group Term Life insurance, relocation tax assistance) that do not affect net cash flow.
  - Automatically inserts both the benefit line (negative/credit) and a corresponding "Offset" transaction line (positive/debit) in the same category to preserve mathematical cash flow balance while reporting correct gross income/taxes.
- **Dry-Run Mode**: Use `--dry-run` to preview all parsed values, resolved categories, matching operations, and import transactions without modifying Lunch Money.

---

## ⚙️ Core Domain & Import Rules

- **Direct Deposit Matching**:
  - Queries Lunch Money transactions within a 6-day window around the check date (`check_date - 3 days` to `check_date + 3 days`).
  - Matches transactions where the net amount matches the payslip's Net Pay exactly (within 1 cent).
- **Synthetic Transactions**:
  - For zero-dollar checks or RSU vests where there isn't a corresponding bank transaction, the tool inserts a synthetic parent transaction on the check date before splitting it.
- **Imputed Income Handling**:
  - Imputed income represents non-cash compensation that increases taxable income.
  - Any description starting with an asterisk `*` (e.g., `*Imp GTL`) or exactly matching (case-insensitive) an entry in the `exceptions` list of `[imputed_income]` is recognized as imputed income.
  - To prevent altering the transaction's net cash flow, two split lines are generated: the main benefit line and a matching "Offset" line with the opposite sign.
- **RSU Vest Processing**:
  - Identifies pages containing "restricted stock" in the earnings section.
  - Creates a synthetic transaction with an amount equal to `-(gross_rsu_value - total_taxes)`.
  - Splits it between `Restricted Stock` (gross amount) and each tax withholding component.

---

## 🔧 Commands & Subcommands

The CLI entrypoint and subcommand dispatching are implemented in [main.rs](./src/main.rs), with the argument definitions in [cli.rs](./src/cli.rs) and the subcommand implementations under [src/commands/](./src/commands/).

### 1. Configuration Wizard (`init`)

Run the setup wizard to create your config file. Pass one or more payslip PDFs to automatically scan and extract unique payslip line item descriptions:

```bash
cargo run --package lm-payslip-importer -- init [PDF_PATHS...]
```

This command executes [run_init](./src/commands/init.rs#L28) in [init.rs](./src/commands/init.rs):
1. Prompts for your Lunch Money developer API key.
2. Connects to the Lunch Money API and fetches your active manual and Plaid accounts, as well as categories.
3. Asks you to select a "Net Zero Account" and an "RSU Account".
4. Parses the provided PDFs to gather the unique earnings, taxes, and deduction descriptions that appeared with a non-zero current amount (YTD-only rows are skipped).
5. Optionally prints a copy-pasteable LLM prompt to help you map each extracted line item to a Lunch Money category.
6. Writes the finished configuration to `lm_payslip_importer.toml`.

Options:
- `--file`, `-f`: Specify a custom output path for the configuration file (defaults to `lm_payslip_importer.toml`).

### 2. Parse and Import Payslips (`import`)

Processes a payslip PDF (e.g. Workday) and synchronizes the splits/transactions to Lunch Money. This logic is handled by [run_import](./src/commands/import.rs#L39) in [import.rs](./src/commands/import.rs).

```bash
cargo run --package lm-payslip-importer -- import <PAYSLIP_PDF> [FLAGS]
```

Flags:
- `--dry-run`: Do not modify anything in Lunch Money. Instead, output the visual plan detailing matched transactions, synthetic transactions to create, and split components.
- `--page <PAGE>`: Specify a page number to process. If omitted, all pages are processed. Can be passed multiple times (e.g., `--page 1 --page 3`).

---

## ⚙️ Configuration File (`lm_payslip_importer.toml`)

The configuration structure is defined by [Config](./src/config.rs#L5-L10) in [config.rs](./src/config.rs). Below is an annotated example of `lm_payslip_importer.toml`:

```toml
[lunch_money]
# Your Lunch Money developer API key
api_key = "..."
# Account where zero-dollar check matches or direct deposit splits will be posted
net_zero_account = "Bank of America Checking"
# Manual account used to track RSU vests
rsu_account = "Equity Awards"
# Payee for newly created direct deposit / net-zero transactions
payslip_payee = "Meta Payslip"
# Payee of the auto-imported $0.00 RSU vest transaction to match against
# (case-insensitive). This is the payee Workday/Plaid assigns to the vest event.
rsu_payee_match = "$META Vest"

[mapping]
# Maps payslip line descriptions to Lunch Money category names (or category IDs).
# Use the full, un-truncated description exactly as the parser emits it.
"Salary" = "Salary"
"Performance Bonus" = "Salary"
"Restricted Stock Units" = "Salary"
"One-Time Sign-On Payment" = "Salary"
"*Imp GTL" = "Salary"
"Medical FSA" = "Medical Insurance"
"Pretax Dental" = "Medical Insurance"
"Pretax Medical" = "Medical Insurance"
"Federal Withholding" = "Taxes"
"Medicare" = "Taxes"
"OASDI" = "Taxes"
"State Tax - NY" = "Taxes"
"City Tax - NY" = "Taxes"
"401k Salary" = "Payment, Transfer"

[imputed_income]
# Full descriptions (exact, case-insensitive match) that represent imputed
# income exceptions. Any description starting with an asterisk '*' is always
# treated as imputed income regardless of this list.
exceptions = [
    "Relocation Tax Ben",
]
```

---

## 🔍 How Import Calculations are Handled

The importer parses the text tables inside [parse_page_tables](./src/payslip.rs#L196) and returns a [ParsedPage](./src/payslip.rs#L38-L49).

When importing and splitting, the net total of the transaction must match the Lunch Money target transaction's amount. The signs are aligned as follows:
- **Earnings (Credits)**: Mapped as negative split amounts (credit/inflow in Lunch Money).
- **Pre-Tax & Post-Tax Deductions (Debits)**: Mapped as positive split amounts (debit/outflow in Lunch Money).
- **Employee Taxes (Debits)**: Mapped as positive split amounts (debit/outflow in Lunch Money).

### Imputed Income Offsetting example
If a payslip contains `*Imp GTL` of `$50.00`, two split lines are created:
1. `Meta Payslip - *Imp GTL` with an amount of `-$50.00` (Credit/Earnings).
2. `Meta Payslip - *Imp GTL Offset` with an amount of `$50.00` (Debit/Deduction).
Both lines are assigned to the category resolved for `*Imp GTL` in `[mapping]`. They cancel each other out mathematically, keeping the net transaction amount intact.

---

## 📈 RSU Vest & Sale Accounting Lifecycle

When your brokerage account receives shares from stock vests, Plaid syncs these events as `$0.00` transactions in your brokerage transaction ledger because no cash was exchanged. However, this leaves your budget blind to your true W-2 gross income and tax withholding history.

`lm-payslip-importer` solves this by injecting a synthetic parent transaction alongside the `$0.00` Plaid transaction. For this system to function cleanly without double-counting your budget or miscalculating your net worth, you should configure your Lunch Money categories as follows.

### Recommended Category Setup

1. **`Stock Vest`** (nested under the **Transfers** group):
   * Mark this category as a **Transfer** (excluded from income/expenses).
   * The auto-imported `$0.00` Plaid transaction should be assigned to this category.
2. **`Stock Sale`** (nested under the **Transfers** group):
   * Mark this category as a **Transfer** (excluded from income/expenses).
   * Any future transactions representing the sale of these shares (converting stock to cash) should be assigned here.
3. **`Salary`** (nested under the **Income** group):
   * Include gross earnings from stock vests in this category so it counts towards your W-2 income.
4. **`Taxes`** (nested under the **Expenses** group):
   * Tax withholdings from the vest split should be mapped to your standard tax expense categories.

### Real-World Walkthrough (Example)

Consider a vesting event where you receive gross stock value of **`$10,000.00`**, but **`$4,000.00`** is withheld for taxes, resulting in net shares worth **`$6,000.00`** deposited into your brokerage account.

#### 1. At Vest (Income & Taxes Recognized)
* Plaid imports a transaction representing the share deposit:
  * **Payee**: `$META Vest` (Amount: `$0.00`, Category: `Stock Vest` / Transfer).
* The importer identifies this event, matches the `$0.00` transaction, and injects a **synthetic parent transaction** into your RSU account for the net value:
  * **Payee**: `Meta Payslip` (the configured `payslip_payee`) (Amount: `-$6,000.00` / credit).
* The importer automatically splits the synthetic parent into:
  * **`-$10,000.00`** under **`Salary`** (Gross W-2 income recorded).
  * **`+$4,000.00`** under **`Taxes`** (Tax withholding expenses recorded).
* **Result**: Your W-2 income increases by `$10,000.00`, your tax expenses increase by `$4,000.00`, and your net worth/budget shows a correct net gain of `$6,000.00`.

#### 2. At Sale (Asset-to-Cash Conversion)
Later, you decide to liquidate that stock:
* Plaid imports a transaction when the stock is sold:
  * **Payee**: `$META Sell` (Amount: `-$6,000.00` / credit to Schwab, Category: `Stock Sale` / Transfer).
* You transfer that cash to your checking account:
  * **Schwab ledger**: `+$6,000.00` (debit, Category: `Payment, Transfer` / Transfer).
  * **Checking ledger**: `-$6,000.00` (credit, Category: `Payment, Transfer` / Transfer).
* **Result**: The sale inflow (`-$6,000.00`) and transfer outflow (`+$6,000.00`) in Schwab cancel each other out. The transfer inflow in your checking account (`-$6,000.00`) is recognized. **No new income is recorded** during the sale because it was already fully accounted for at the vest!

---

## 🛠️ Code Structure

- [main.rs](./src/main.rs): Binary entrypoint; parses CLI args and dispatches to the subcommand implementations.
- [cli.rs](./src/cli.rs): Command-line argument definitions ([Cli](./src/cli.rs#L34-L37) / [Commands](./src/cli.rs#L40-L45) / [ImportArgs](./src/cli.rs#L48) / [InitArgs](./src/cli.rs#L67)).
- [payslip.rs](./src/payslip.rs): PDF page text extraction ([convert_pdf_to_pages](./src/payslip.rs#L319)), table token parsing ([parse_page_tables](./src/payslip.rs#L196)), and the [ParsedPage](./src/payslip.rs#L38-L49) / `RowData` data model.
- [commands/import.rs](./src/commands/import.rs): Lunch Money transaction query/matching, pre-flight validation, and import execution logic ([run_import](./src/commands/import.rs#L39)).
- [commands/init.rs](./src/commands/init.rs): The interactive configuration setup wizard ([run_init](./src/commands/init.rs#L28)).
- [config.rs](./src/config.rs): Handles loading the `lm_payslip_importer.toml` configuration (with backwards-compatible loading of `workday_payslip_splitter.toml`) and defines the config structs.
- [style.rs](./src/style.rs): Constants for stylized shell logs (e.g., `STYLE_HEADER`, `STYLE_SUCCESS`, `STYLE_WARNING`, `STYLE_ERROR`).
