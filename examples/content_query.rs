//! Query a content provider (here: the SMS inbox) into typed rows.
//!
//! Put free-text columns last: the `content` tool does not escape `, ` in
//! values, and the parser lets only the final projected column contain it.
//!
//! ```sh
//! cargo run --example content_query
//! ```

mod common;

#[tokio::main]
async fn main() -> rsadb::Result<()> {
    let device = common::connect().await?;
    let rows = device
        .content_query_with(
            "content://sms/inbox",
            &["_id", "address", "date", "body"],
            &["--sort", "date DESC"],
        )
        .await?;
    println!("{} rows", rows.len());
    for row in rows.iter().take(10) {
        println!(
            "#{} {} @{}: {}",
            row["_id"],
            row["address"],
            row["date"],
            row["body"].lines().next().unwrap_or("")
        );
    }
    Ok(())
}
