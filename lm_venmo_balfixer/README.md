# Lunch Money Venmo Balance Fixer (`lm-venmo-balfixer`)

A tool to ensure Plaid-synced Venmo accounts in Lunch Money follow proper double-entry accounting principles by automatically identifying and generating synthetic inflow transactions representing implicit funding events.

## Context

When your Venmo balance is insufficient to cover a payment, Venmo initiates an ACH debit from your linked bank account. Plaid records the payment transaction in your Venmo history, but completely omits the matching transfer transaction that moved the cash from your bank into Venmo.

As a result, the computed balance of the Venmo account in Lunch Money drifts over time.

This tool scans your transaction histories across both accounts, identifies unmatched debit transfers on your bank checking account, and automatically creates a synthetic matching inflow (`Venmo Transfer (Synthetic)`) on the Venmo side in Lunch Money.

## Setup & Configuration

You can easily set up the configuration using the interactive setup wizard:

```bash
lm-utils venmo-balfixer init
```

The wizard will:
1. Retrieve your Lunch Money developer API key interactively.
2. Connect to the Lunch Money API and query all active Plaid accounts.
3. Guide you to select the correct Bank checking account and Venmo account.
4. Upsert the `[venmo]` section (and the shared `[common].lm_api_key`) into `lm_utils.toml`, creating the file if needed and leaving any other tools' sections intact. The relevant sections look like:

```toml
# Shared settings for every Lunch Money utility tool.
[common]
# Your Lunch Money developer API key (shared by every tool).
lm_api_key = "your_lunch_money_api_key_here"

[venmo]
venmo_acct = "Venmo"
bank_acct = "Bank Checking"
```



## Running

The tool exposes the `reconcile` command, which takes a scan duration window
(e.g. `30d`, `2w`, `3months`):

```bash
# Dry run: display what would be created without inserting any transactions
lm-utils venmo-balfixer reconcile 30d --dry-run

# Reconcile and insert synthetic transactions for the last 30 days
lm-utils venmo-balfixer reconcile 30d
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

---

## ⏰ Automated Scheduling (Systemd Timers)

To keep Lunch Money up-to-date automatically, you can schedule the reconciliation task using a `systemd` user timer. Running this task as a user service is highly recommended: it requires no root (`sudo`) privileges, runs under your local user session, and logs directly to `journald`.

I suggest setting up a daily timer with a `30d` (30 days) rolling window to capture settled bank transfers and insert synthetic Venmo transactions.

### Step-by-Step Installation

#### 1. Create the Systemd User Directory
Ensure your systemd user configuration directory exists:
```bash
mkdir -p ~/.config/systemd/user
```

#### 2. Define the Service
Create `~/.config/systemd/user/venmo-balfixer.service`:
```ini
[Unit]
Description=Reconcile Venmo Balances to Lunch Money (30 day window)
After=network.target

[Service]
Type=oneshot
WorkingDirectory=/path/to/lm-utils
ExecStart=/path/to/lm-utils/target/release/lm-utils venmo-balfixer reconcile 30d
```
> [!NOTE]
> Replace `/path/to/lm-utils` with the absolute path to your cloned repository (where `lm_utils.toml` and the compiled `lm-utils` binary are located). Do not use `~` in unit files as systemd does not perform shell expansion (though systemd specifiers like `%h` can be used to refer to your home directory).

#### 3. Define the Timer
Create `~/.config/systemd/user/venmo-balfixer.timer`:
```ini
[Unit]
Description=Run venmo-balfixer daily

[Timer]
OnCalendar=daily
Persistent=true

[Install]
WantedBy=timers.target
```

#### 4. Load and Enable the Timer
Tell systemd to reload its configuration, then enable and start the timer:
```bash
# Reload the systemd user daemon to register the new units
systemctl --user daemon-reload

# Enable and start the timer immediately
systemctl --user enable --now venmo-balfixer.timer
```

#### 5. Verify the Installation
You can verify that the timer is active and see when it is next scheduled to run:
```bash
systemctl --user list-timers
```

#### 6. View Logs and Debugging
To view the output/logs of the reconciliation execution, use `journald`'s query tool:
```bash
# View logs for the venmo-balfixer service
journalctl --user -u venmo-balfixer.service

# Stream logs in real-time
journalctl --user -u venmo-balfixer.service -f
```

