//! Core backup logic: fetches every endpoint and writes raw JSON to disk.

use std::path::Path;

use anstream::println;
use anyhow::Context;

use lm_common::style::*;

use crate::raw_client::RawClient;

/// Run the full backup, writing each endpoint's raw JSON into `output_dir`.
pub(crate) async fn run(
    client: &RawClient,
    output_dir: &Path,
    start_date: &str,
    skip_attachments: bool,
) -> anyhow::Result<()> {
    std::fs::create_dir_all(output_dir)
        .with_context(|| format!("Failed to create output directory: {}", output_dir.display()))?;

    let bar = "─".repeat(60);

    println! {};
    println! { "{STYLE_HEADER}💾 Lunch Money Full Backup{STYLE_HEADER:#}" };
    println! { "{STYLE_DIM}{bar}{STYLE_DIM:#}" };
    println! { "  Output: {}", output_dir.display() };
    println! {};

    // ── 1. User ──────────────────────────────────────────────
    fetch_and_save(client, "me", &[], output_dir, "user.json").await?;

    // ── 2. Categories ────────────────────────────────────────
    fetch_and_save(
        client,
        "categories",
        &[("format", "nested")],
        output_dir,
        "categories.json",
    )
    .await?;

    // ── 3. Tags ──────────────────────────────────────────────
    fetch_and_save(client, "tags", &[], output_dir, "tags.json").await?;

    // ── 4. Manual accounts ───────────────────────────────────
    fetch_and_save(
        client,
        "manual_accounts",
        &[],
        output_dir,
        "manual_accounts.json",
    )
    .await?;

    // ── 5. Plaid accounts ────────────────────────────────────
    fetch_and_save(
        client,
        "plaid_accounts",
        &[],
        output_dir,
        "plaid_accounts.json",
    )
    .await?;

    // ── 6. Recurring items ───────────────────────────────────
    fetch_and_save(
        client,
        "recurring_items",
        &[],
        output_dir,
        "recurring_items.json",
    )
    .await?;

    // ── 7. Budget settings ───────────────────────────────────
    fetch_and_save(
        client,
        "budgets/settings",
        &[],
        output_dir,
        "budget_settings.json",
    )
    .await?;

    // ── 8. Budget summary ────────────────────────────────────
    let today = jiff::Zoned::now().date();
    let end_date = today.to_string();
    fetch_and_save(
        client,
        "summary",
        &[
            ("start_date", start_date),
            ("end_date", &end_date),
            ("include_exclude_from_budgets", "true"),
            ("include_occurrences", "true"),
            ("include_past_budget_dates", "true"),
            ("include_totals", "true"),
            ("include_rollover_pool", "true"),
        ],
        output_dir,
        "budget_summary.json",
    )
    .await?;

    // ── 9. Transactions (paginated) ──────────────────────────
    let transactions = fetch_all_transactions(client, start_date, &end_date).await?;
    write_json(output_dir, "transactions.json", &transactions)?;
    let tx_count = transactions["transactions"]
        .as_array()
        .map_or(0, |a| a.len());
    println! { "  {STYLE_INFO}✓{STYLE_INFO:#} transactions.json ({} transactions)", tx_count };

    // ── 10. Attachments ──────────────────────────────────────
    if skip_attachments {
        println! { "  {STYLE_DIM}⏭ Skipping attachments (--skip-attachments){STYLE_DIM:#}" };
    } else {
        download_attachments(client, &transactions, output_dir).await?;
    }

    println! {};
    println! { "{STYLE_HEADER}✅ Backup complete → {}{STYLE_HEADER:#}", output_dir.display() };
    println! {};

    Ok(())
}

/// Fetch a single endpoint and write the raw JSON to a file.
async fn fetch_and_save(
    client: &RawClient,
    endpoint: &str,
    query: &[(&str, &str)],
    output_dir: &Path,
    filename: &str,
) -> anyhow::Result<()> {
    let value = client.get(endpoint, query).await?;
    write_json(output_dir, filename, &value)?;
    println! { "  {STYLE_INFO}✓{STYLE_INFO:#} {filename}" };
    Ok(())
}

/// Pretty-print a JSON value to a file.
fn write_json(dir: &Path, filename: &str, value: &serde_json::Value) -> anyhow::Result<()> {
    let path = dir.join(filename);
    let file = std::fs::File::create(&path)
        .with_context(|| format!("Failed to create {}", path.display()))?;
    let writer = std::io::BufWriter::new(file);
    serde_json::to_writer_pretty(writer, value)
        .with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

/// Paginate through all transactions and merge into a single JSON value.
async fn fetch_all_transactions(
    client: &RawClient,
    start_date: &str,
    end_date: &str,
) -> anyhow::Result<serde_json::Value> {
    let limit = "500";
    let mut all_txs: Vec<serde_json::Value> = Vec::new();
    let mut offset: u32 = 0;

    loop {
        let offset_str = offset.to_string();
        let query = &[
            ("start_date", start_date),
            ("end_date", end_date),
            ("limit", limit),
            ("offset", offset_str.as_str()),
            ("include_pending", "true"),
            ("include_children", "true"),
            ("include_files", "true"),
            ("include_metadata", "true"),
            ("include_group_children", "true"),
            ("include_split_parents", "true"),
        ];

        let resp = client.get("transactions", query).await?;

        let page = resp["transactions"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let has_more = resp["has_more"].as_bool().unwrap_or(false);

        let page_len = page.len();
        all_txs.extend(page);

        if has_more && page_len > 0 {
            offset += page_len as u32;
        } else {
            break;
        }
    }

    Ok(serde_json::json!({
        "transactions": all_txs,
        "has_more": false,
        "total_count": all_txs.len(),
    }))
}

/// Download every file attachment referenced in the transactions.
async fn download_attachments(
    client: &RawClient,
    transactions: &serde_json::Value,
    output_dir: &Path,
) -> anyhow::Result<()> {
    let txs = transactions["transactions"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    // Collect all (transaction_id, attachment) pairs first.
    let mut items: Vec<(serde_json::Value, serde_json::Value)> = Vec::new();
    for tx in &txs {
        if let Some(files) = tx["files"].as_array() {
            for file in files {
                items.push((tx["id"].clone(), file.clone()));
            }
        }
    }

    if items.is_empty() {
        println! { "  {STYLE_DIM}⏭ No attachments to download{STYLE_DIM:#}" };
        return Ok(());
    }

    let attachments_dir = output_dir.join("attachments");
    std::fs::create_dir_all(&attachments_dir)?;

    let mut manifest: Vec<serde_json::Value> = Vec::new();
    let total = items.len();

    for (idx, (tx_id, file)) in items.iter().enumerate() {
        let file_id = file["id"].as_u64().unwrap_or(0);
        let filename = file["name"].as_str().unwrap_or("unknown");
        let save_name = format!("{}_{}", file_id, sanitize_filename(filename));

        // Get the signed download URL.
        let url_resp = client
            .get(&format!("transactions/attachments/{}", file_id), &[])
            .await;

        match url_resp {
            Ok(resp) => {
                if let Some(signed_url) = resp["url"].as_str() {
                    match client.download_bytes(signed_url).await {
                        Ok(bytes) => {
                            std::fs::write(attachments_dir.join(&save_name), &bytes)?;
                            manifest.push(serde_json::json!({
                                "transaction_id": tx_id,
                                "attachment_id": file_id,
                                "original_filename": filename,
                                "saved_as": save_name,
                                "mime_type": file["type"],
                                "size_kb": file["size"],
                                "bytes_downloaded": bytes.len(),
                            }));
                            println! {
                                "  {STYLE_INFO}✓{STYLE_INFO:#} attachment [{}/{}] {}",
                                idx + 1, total, save_name
                            };
                        }
                        Err(e) => {
                            println! {
                                "  {STYLE_WARNING}⚠ attachment [{}/{}] download failed: {}{STYLE_WARNING:#}",
                                idx + 1, total, e
                            };
                            manifest.push(serde_json::json!({
                                "transaction_id": tx_id,
                                "attachment_id": file_id,
                                "original_filename": filename,
                                "error": format!("download failed: {}", e),
                            }));
                        }
                    }
                } else {
                    println! {
                        "  {STYLE_WARNING}⚠ attachment [{}/{}] no signed URL returned{STYLE_WARNING:#}",
                        idx + 1, total
                    };
                }
            }
            Err(e) => {
                println! {
                    "  {STYLE_WARNING}⚠ attachment [{}/{}] URL fetch failed: {}{STYLE_WARNING:#}",
                    idx + 1, total, e
                };
                manifest.push(serde_json::json!({
                    "transaction_id": tx_id,
                    "attachment_id": file_id,
                    "original_filename": filename,
                    "error": format!("URL fetch failed: {}", e),
                }));
            }
        }
    }

    write_json(
        &attachments_dir,
        "manifest.json",
        &serde_json::Value::Array(manifest),
    )?;
    println! { "  {STYLE_INFO}✓{STYLE_INFO:#} attachments/manifest.json" };

    Ok(())
}

/// Strip characters that are problematic in filenames.
fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | '\0' => '_',
            _ => c,
        })
        .collect()
}
