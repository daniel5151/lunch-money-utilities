# Prilik's Lunch Money Utilities

Assorted [Lunch Money](https://lunchmoney.app/) utilities.

Some are pretty generic (e.g: `lm-splitwise-sync`), others are a bit more
specialized (e.g: `lm-payslip-importer`), but given that they all share a common
foundation... it makes sense to keep 'em all under one roof.

> [!WARNING]
>
> This repo is nearly 100% free range Gemini Flash 3.5 / Opus 4.8 vibe code.
>
> While The Prompter (Daniel Prilik) _has_ been auditing code as it's generated,
> and trying his darndest to make sure obvious slop gets refactored and
> tightened up... you may wish to audit the code in this repo yourself before
> deploying any of these projects.
>
> That said, The Prompter _is_ actively using this code with his personal
> Lunch Money account... so hey, it's Probably Fine™️

## Crates

| Crate                                         | Path                   | Description                                                                                                                                                                                                           |
| --------------------------------------------- | ---------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| [`lunch_money`](lunch_money/)                 | `lunch_money/`         | Lib: Client for interacting with the Lunch Money v2 API                                                                                                                                                               |
| [`lm-splitwise-sync`](lm_splitwise_sync/)     | `lm_splitwise_sync/`   | Bin: syncs Splitwise transactions and outstanding balances into Lunch Money manual accounts. See its [README](lm_splitwise_sync/README.md).                                                                           |
| [`lm-payslip-importer`](lm_payslip_importer/) | `lm_payslip_importer/` | Bin: Imports direct deposits and RSU vests from payslips (primarily tested with Workday, but designed to be generic) into granular transaction lines in Lunch Money. See its [README](lm_payslip_importer/README.md). |
| [`lm-venmo-balfixer`](lm_venmo_balfixer/)     | `lm_venmo_balfixer/`   | Bin: Fixup Plaid-synced Venmo accounts to follow proper double-entry accounting principles. See its [README](lm_venmo_balfixer/README.md).                                                                            |

Expect more utilities to appear over time...

## License

Everything is MIT. See [LICENSE](LICENSE).

Have fun!
