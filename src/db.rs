use anyhow::Result;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};
use std::path::Path;

pub async fn init_db() -> Result<SqlitePool> {
    let data_dir = Path::new("data");
    if !data_dir.exists() {
        std::fs::create_dir(data_dir)?;
    }

    let options = SqliteConnectOptions::new()
        .filename("data/slaves.db")
        .create_if_missing(true);

    let pool = SqlitePool::connect_with(options).await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS slaves (
            username TEXT PRIMARY KEY
        )
        "#,
    )
    .execute(&pool)
    .await?;

    Ok(pool)
}

pub async fn add_slave(pool: &SqlitePool, username: &str) -> Result<()> {
    sqlx::query("INSERT OR IGNORE INTO slaves (username) VALUES (?)")
        .bind(username)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn get_slaves(pool: &SqlitePool) -> Result<Vec<String>> {
    let slaves = sqlx::query_as::<_, (String,)>("SELECT username FROM slaves")
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(|row| row.0)
        .collect();
    Ok(slaves)
}
