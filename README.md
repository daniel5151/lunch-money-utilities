# `splitwise-lunchmoney`

Sync Splitwise transactions into Lunch Money.

> [!WARNING]
>
> This is 100% free range Gemini Flash 3.5 vibe code.
>
> The author is confident enough in this code to use it with his live splitwise / lunch-money accounts... but YMMV.

---

## ⚡ Current Capabilities

- **Auto-Config & Interactive Wizard (`init`)**: Walks you through setting up Splitwise and Lunch Money credentials, fetches manual accounts from your Lunch Money profile, and maps target accounts based on naming patterns (`Splitwise {CURRENCY}`).
- **Multi-Currency Account Mapping**: Syncs transactions to separate Lunch Money manual accounts based on the transaction currency.
- **Deleted Account Guard Rails**: Prior to any sync operation, the tool validates that all configured manual accounts exist in Lunch Money. If an account has been deleted or is missing, sync halts immediately with a clear error and exit code `1`.
- **Inline Descriptions & Details**: Dry runs and logs include the target Lunch Money manual account name/display name, transaction note/description (from Splitwise expense description), currency, and net balance.
- **Group Filtering**: Supports ignoring specific Splitwise groups during window sync and query operations by configuring `ignored_groups` in the configuration file.
- **Dry-Run Operations**: Run any sync command with the `--dry-run` flag to preview changes without modifying Lunch Money.

## 🔧 Commands

- `sync window`: Syncs all transactions within a specified time frame (e.g., `3 days`, `1 week`, `30 days`).
- `sync group`: Syncs all transactions associated with a specific Splitwise Group ID.
- `sync balances`: Syncs global Splitwise balances into Lunch Money splitwise accounts.
- `query splitwise get-groups`: List all Splitwise groups you belong to, including outstanding balances in all active currencies.
- `query splitwise window` and `query splitwise group`: Fetches and lists raw Splitwise expenses for review.
- `query lunchmoney categories`: Lists category names and IDs configured in Lunch Money

---

## ⚙️ Expected Install & Setup Flow

### 1. Prerequisites
Ensure you have the Rust toolchain installed. If not, install it via [rustup.rs](https://rustup.rs/):
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### 2. Build the Project
Clone the repository and build the binary:
```bash
git clone https://github.com/your-username/splitwise-lunchmoney.git
cd splitwise-lunchmoney
cargo build --release
```
The compiled binary will be located at `target/release/splitwise-lunchmoney`.

### 3. Initialize Configuration
Before synchronization can run, you must initialize the configuration. Run the interactive setup wizard:
```bash
cargo run -- init
```
This wizard will prompt you for:
1. **Splitwise API Key**: Obtain a personal developer token from Splitwise.
2. **Splitwise User Selection**: Automatically queries the active user and confirms it with you.
3. **Lunch Money API Key**: Generate a developer API key in your Lunch Money settings.
4. **Supported Currencies**: You will be prompted in a loop to enter the 3-letter currency codes you wish to support (e.g., `USD`, `CAD`).
5. **Auto-Account Resolution**: The tool fetches manual accounts from Lunch Money and looks for exact names matching `Splitwise {CURRENCY}` (case-insensitive).

Upon successful completion, the configuration is saved to `splitwise-lunchmoney.toml`.

---

## Usage - Sync

The `sync` command family can be used to import / sync splitwise transactions with lunch money.

> [!WARNING]
>
> Always preview changes first using the `--dry-run` flag!

**Sync last 3 days of transactions:**
```bash
cargo run -- sync window "3 days" --dry-run
```

**Sync group expenses and apply a tag to imported transactions in Lunch Money:**
```bash
cargo run -- sync group 82559678  --tag "Stockholm 2025" --dry-run
```

**Sync global balances into manual accounts:**
```bash
cargo run -- sync balances --dry-run
```

## Usage - Query

As a convenience - you can read-only query various aspects of your Splitwise / Lunch Money account via the `query` command family.

**List all Splitwise groups and outstanding multi-currency balances:**
```bash
cargo run -- query splitwise get-groups
```

**Query raw Splitwise expenses in the last 3 days:**
```bash
cargo run -- query splitwise window "3 days"
```

**Query raw Splitwise expenses for a specific group:**
```bash
cargo run -- query splitwise group 82559678
```

**List all categories configured in your Lunch Money profile:**
```bash
cargo run -- query lunchmoney categories
```

---

## ⏰ Automated Scheduling (Cron)

To keep Lunch Money up-to-date, you can schedule the `sync window` command to run periodically using `cron`.

To avoid spooky edits to long-posted transactions, only transactions within your selected window will be automatically modified / deleted. Transactions outside that window are considered to be logically "posted", and will not be retroactively updated (unless explicitly synced using something like `sync group`).

> NOTE: In the future, it would be good to add extra logic to `sync window` that leverages the splitwise API's `updated_after` API in order to catch backdated transactions / updates to old transactions.
>
> In that case, it could be prudent to add some kind of "soft delete" / "soft update" policy, that doesn't actually modify those old transactions destructively... but somehow signals to the user that they need to be manually looked at (e.g: non-destructively adding tags to those old transactions + importing a "dummy" transaction into lunch money that will alert the user of the backdated modification?)

### Example Crontab Setup

1. Open your user crontab editor:
   ```bash
   crontab -e
   ```

2. Add a cron job. For example, to run the sync every day at 3:00 AM with a rolling window of 7 days:
   ```cron
   0 3 * * * cd /path/to/splitwise-lunchmoney && ./target/release/splitwise-lunchmoney sync window "7 days" >> ./sync.log 2>&1
   ```
