# `lm-backup`

Backup a copy of your Lunch Money account via the v2 API.

Technically, this isn't a _full_ copy, as not all info is exposed via the API (e.g: historical balances, rules, etc...), but it certainly includes most critical data (notably: transaction history).

Unlike the built in CSV export tools, this backup tool includes plaid / custom metadata associated with transactions, which can be quite the goldmine of additional info.

## Usage

```console
$ lm-utils backup                           # dump everything to ./lm-backup-{timestamp}/
$ lm-utils backup -o ~/backups/lm-latest    # custom output directory
$ lm-utils backup --skip-attachments        # skip downloading file attachments
$ lm-utils backup --start-date 2023-01-01   # limit transaction history
```

## What gets backed up

| Endpoint                        | Output file              |
| ------------------------------- | ------------------------ |
| `GET /me`                       | `user.json`              |
| `GET /categories` (nested)      | `categories.json`        |
| `GET /tags`                     | `tags.json`              |
| `GET /manual_accounts`          | `manual_accounts.json`   |
| `GET /plaid_accounts`           | `plaid_accounts.json`    |
| `GET /recurring_items`          | `recurring_items.json`   |
| `GET /budgets/settings`         | `budget_settings.json`   |
| `GET /summary`                  | `budget_summary.json`    |
| `GET /transactions` (paginated) | `transactions.json`      |
| Attachment file downloads       | `attachments/` directory |

Transactions are fetched with all `include_*` flags enabled (pending,
children, files, metadata, group children, split parents) and
auto-paginated across the full date range.

Attachments are downloaded via their signed URLs and saved alongside a
`manifest.json` that maps each file back to its transaction.  Failures
are logged as warnings and recorded in the manifest without aborting
the backup.

## Options

| Flag                  | Default                         | Description                                    |
| --------------------- | ------------------------------- | ---------------------------------------------- |
| `-o, --output <DIR>`  | `./lm-backup-{timestamp}`       | Output directory                               |
| `--skip-attachments`  | off                             | Don't download file attachments                |
| `--start-date <DATE>` | `1997-12-21`                    | Earliest transaction date to include           |
| `--api-url <URL>`     | `https://api.lunchmoney.dev/v2` | Override the API base URL (mainly for testing) |

## Configuration

Only needs `[common].lm_api_key` in `lm_utils.toml` — no tool-specific config section.
