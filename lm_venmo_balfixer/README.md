# Lunch Money Venmo Balance Fixer (`lm-venmo-balfixer`)

A tool to ensure Plaid-synced Venmo accounts in Lunch Money follow proper double-entry accounting principles by automatically identifying and generating synthetic inflow transactions representing implicit funding events.

## Context

When your Venmo balance is insufficient to cover a payment, Venmo initiates an ACH debit from your linked bank account. Plaid records the payment transaction in your Venmo history, but completely omits the matching transfer transaction that moved the cash from your bank into Venmo.

As a result, the computed balance of the Venmo account in Lunch Money drifts over time.

This tool scans your transaction histories across both accounts, identifies unmatched debit transfers on your bank checking account, and automatically creates a synthetic matching inflow (`Venmo Transfer (Synthetic)`) on the Venmo side in Lunch Money.

## Setup & Configuration

You can easily set up the configuration using the interactive setup wizard:

```bash
cargo run -p lm-venmo-balfixer -- init
```

The wizard will:
1. Retrieve your Lunch Money developer API key interactively.
2. Connect to the Lunch Money API and query all active Plaid accounts.
3. Guide you to select the correct Bank checking account and Venmo account.
4. Save the configuration to `lm_venmo_balfixer.toml` in the following format:

```toml
[lunch_money]
api_key = "your_lunch_money_api_key_here"

[accounts]
venmo_acct = "Venmo"
bank_acct = "Bank Checking"
```



## Running

The tool exposes the `reconcile` command, which takes a scan duration window
(e.g. `30d`, `2w`, `3months`):

```bash
# Dry run: display what would be created without inserting any transactions
cargo run -p lm-venmo-balfixer -- reconcile 30d --dry-run

# Reconcile and insert synthetic transactions for the last 30 days
cargo run -p lm-venmo-balfixer -- reconcile 30d
```

### Behavior notes

- Synthetic transactions are inserted as **unreviewed**, so they show up in your
  Lunch Money review queue for you to eyeball rather than landing pre-cleared.
- **Pending** transactions are ignored on both accounts. A pending transfer can
  later re-post with a changed amount/date or disappear entirely, so the tool
  only reconciles settled transactions to avoid orphaning a synthetic.
- Each synthetic carries a stable `external_id` (`synthetic-venmo-<bank-tx-id>`),
  so re-running over an overlapping window won't create duplicates — the API
  reports those as skipped duplicates.
