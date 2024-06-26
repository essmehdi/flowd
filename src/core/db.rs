use rusqlite;
use rusqlite::{params_from_iter, Connection, Params};
use std::path::Path;
use thiserror::Error;
use tokio::fs::{self, File};
use tokio::io;

use crate::{core::download::DownloadStatus, utils};

use super::{config, download::Download};

#[derive(Error, Debug)]
pub enum DBError {
    #[error("Download #{0} not found")]
    DownloadNotFound(i64),

    #[error("Rusqlite error: {0}")]
    RusqliteError(#[from] rusqlite::Error),

    #[error("IO error: {0}")]
    IOError(#[from] io::Error),
}

const DB_DIR: &str = "~/.local/share/flowd/";
const DB_NAME: &str = "downloads.db";

const FLOWD_MIGRATIONS_DIR: &str = "/usr/share/flowd/migrations";

fn get_db_path() -> String {
    let db_path = Path::new(DB_DIR).join(DB_NAME);
    let db_path_string = db_path.to_string_lossy().to_string();
    utils::path::expand(&db_path_string)
}

async fn get_migrations(from_version: u16) -> Result<String, io::Error> {
    let mut migrations = String::new();

    let mut version = from_version;
    loop {
        let file = Path::new(FLOWD_MIGRATIONS_DIR).join(format!("{}.sql", version));

        if !file.as_path().exists() {
            break;
        }

        let sql_script = fs::read(&file).await?;
        migrations.push_str(&String::from_utf8(sql_script).unwrap());

        version += 1;
    }

    Ok(migrations)
}

async fn verify_and_update_schema(init: bool) -> Result<(), DBError> {
    log::debug!("Verifying and updating database schema...");

    let next_version = if init {
        1
    } else {
        let connection = connect().await?;

        connection.pragma_query_value(None, "user_version", |row| Ok(row.get::<_, u16>(0)))?? + 1
        // Start from next version
    };
    
    let migrations = get_migrations(next_version).await?;
    let connection = connect().await?;
    connection.execute_batch(&migrations)?;

    Ok(())
}

pub async fn init() -> Result<(), DBError> {
    let db_path = get_db_path();
    let db_exists = Path::new(&db_path).exists();
    if db_exists {
        verify_and_update_schema(false).await?;
        return Ok(());
    }

    // Create DB directory and file
    fs::create_dir_all(utils::path::expand(DB_DIR))
        .await?;
    File::create(&db_path).await?;

    verify_and_update_schema(true).await?;

    Ok(())
}

async fn connect() -> rusqlite::Result<Connection> {
    let db_path = get_db_path();
    Connection::open(&db_path)
}

pub async fn new_download(download: &Download) -> Result<i64, DBError> {
    let completed_date = download
        .date_completed
        .and_then(|d| Some(d.to_string()))
        .or(Some("NULL".to_string()))
        .unwrap();
    let connection = connect().await?;
    connection.execute(
        "
        INSERT INTO downloads (
            url,
            status,
            data_confirmed,
            detected_output_file,
            output_file,
            temp_file,
            resumable,
            date_added,
            date_completed,
            size
        )
        VALUES (
            ?1,
            ?2,
            ?3,
            ?4,
            ?5,
            ?6,
            ?7,
            ?8,
            ?9,
            ?10
        )
        ",
        &[
            &download.url,
            download.status.get_string(),
            &download.data_confirmed.to_string(),
            download
                .detected_output_file
                .as_deref()
                .or(Some("NULL"))
                .unwrap(),
            download.output_file.as_deref().or(Some("NULL")).unwrap(),
            &download.temp_file,
            &download.resumable.to_string(),
            &download.date_added.to_string(),
            &completed_date,
            &download
                .size
                .and_then(|size| Some(size.to_string()))
                .unwrap_or("NULL".to_string()),
        ],
    )?;
    Ok(connection.last_insert_rowid())
}

async fn get_downloads_from_query(
    query: &str,
    params: impl Params,
) -> Result<Vec<Download>, DBError> {
    let connection = connect().await?;

    let mut stmt = connection.prepare(query)?;
    let downloads_iter = stmt.query_map(params, |row| {
        let status: String = row.get(2).unwrap();
        Ok(Download {
            id: row.get(0)?,
            url: row.get(1)?,
            status: DownloadStatus::from_string(&status),
            data_confirmed: row.get::<usize, String>(3)?.parse::<bool>().unwrap(),
            detected_output_file: string_to_option(row.get(4)?),
            output_file: string_to_option(row.get(5)?),
            temp_file: row.get(6)?,
            resumable: row.get::<usize, String>(7)?.parse::<bool>().unwrap(),
            date_added: row.get::<usize, i64>(8)?,
            date_completed: row.get::<usize, i64>(9).ok(),
            size: row.get(10).ok(),
        })
    })?;

    let mut downloads = Vec::new();
    for download in downloads_iter {
        downloads.push(download?);
    }
    Ok(downloads)
}

pub async fn get_all_downloads() -> Result<Vec<Download>, DBError> {
    get_downloads_from_query("SELECT * FROM downloads", []).await
}

pub async fn get_download_by_id(id: i64) -> Result<Download, DBError> {
    let download = get_downloads_from_query("SELECT * FROM downloads WHERE id = ?1", [id])
        .await?
        .pop();

    if let Some(download) = download {
        return Ok(download);
    }
    
    return Err(DBError::DownloadNotFound(id));
}

pub async fn get_pending_downloads() -> Result<Vec<Download>, DBError> {
    get_downloads_from_query(
        "SELECT * FROM downloads WHERE status = ?1",
        [DownloadStatus::Pending.get_string()],
    )
    .await
}

pub async fn get_sorted_downloads() -> Result<Vec<Download>, DBError> {
    get_downloads_from_query(
        "SELECT * FROM downloads ORDER BY CASE WHEN status = ?1 THEN 1 WHEN status = ?2 THEN 2 WHEN status = ?3 THEN 3 WHEN status = ?4 THEN 4 WHEN status = ?5 THEN 6 ELSE 5 END, date_added DESC",
        [
            DownloadStatus::InProgress.get_string(),
            DownloadStatus::Starting.get_string(),
            DownloadStatus::Pending.get_string(),
            DownloadStatus::Paused.get_string(),
            DownloadStatus::Completed.get_string(),
        ],
    )
    .await
}

pub async fn get_in_progress_downloads() -> Result<Vec<Download>, DBError> {
    get_downloads_from_query(
        "SELECT * FROM downloads WHERE status = ?1",
        [DownloadStatus::InProgress.get_string()],
    )
    .await
}

pub async fn get_completed_downloads() -> Result<Vec<Download>, DBError> {
    get_downloads_from_query(
        "SELECT * FROM downloads WHERE status = ?1",
        [DownloadStatus::Completed.get_string()],
    )
    .await
}

pub async fn get_uncompleted_downloads() -> Result<Vec<Download>, DBError> {
    get_downloads_from_query(
        "SELECT * FROM downloads WHERE status != ?1",
        [DownloadStatus::Completed.get_string()],
    )
    .await
}

pub async fn get_downloads_by_category(category: &str) -> Result<Vec<Download>, DBError> {
    let categories = config::get_categories().await;
    let the_category = categories.get(category);

    if let None = the_category {
        log::error!("Category `{}` does not exist", category);
        return Ok(vec![]);
    }

    let category = the_category.unwrap();

    let mut conditions: Vec<String> = vec![];
    for i in 0..category.extensions.len() {
        conditions.push(format!(
            "output_file LIKE %?{} OR detected_output_file LIKE %?{}",
            i, i
        ));
    }

    let query = format!("SELECT * FROM downloads WHERE {}", conditions.join(" OR "));
    // let extensions = category.extensions.iter().map(|x| x.as_str()).collect::<[&str]>()
    let downloads =
        get_downloads_from_query(query.as_str(), params_from_iter(category.extensions.iter()))
            .await;

    downloads
}

pub async fn update_download(download: &Download) -> Result<usize, DBError> {
    let completed_date = download
        .date_completed
        .and_then(|d| Some(d.to_string()))
        .or(Some("NULL".to_string()))
        .unwrap();
    let connection = connect().await?;
    connection
        .execute(
            "
        UPDATE downloads
        SET
            url = ?1,
            status = ?2,
            data_confirmed = ?3,
            detected_output_file = ?4,
            output_file = ?5,
            temp_file = ?6,
            resumable = ?7,
            date_added = ?8,
            date_completed = ?9,
            size = ?10
        WHERE id = ?11
        ",
            &[
                &download.url,
                download.status.get_string(),
                &download.data_confirmed.to_string(),
                download
                    .detected_output_file
                    .as_deref()
                    .or(Some("NULL"))
                    .unwrap(),
                download.output_file.as_deref().or(Some("NULL")).unwrap(),
                &download.temp_file,
                &download.resumable.to_string(),
                &download.date_added.to_string(),
                &completed_date,
                &download
                    .size
                    .and_then(|size| Some(size.to_string()))
                    .unwrap_or("NULL".to_string()),
                &download.id.to_string(),
            ],
        )
        .map_err(|e| DBError::RusqliteError(e))
}

pub async fn delete_download(download_id: i64) -> Result<usize, DBError> {
    let connection = connect().await?;
    connection
        .execute(
            "
        DELETE FROM downloads
        WHERE id = ?1
        ",
            &[&download_id.to_string()],
        )
        .map_err(|e| DBError::RusqliteError(e))
}

pub async fn change_download_status(
    download_id: &i64,
    status: &DownloadStatus,
) -> Result<usize, DBError> {
    let connection = connect().await?;
    connection
        .execute(
            "
        UPDATE downloads
        SET status = ?1
        WHERE id = ?2
        ",
            &[status.get_string(), &download_id.to_string()],
        )
        .map_err(|e| DBError::RusqliteError(e))
}

pub async fn change_download_output_file_path(
    download_id: i64,
    output_file: &str,
) -> Result<(), DBError> {
    let mut download = get_download_by_id(download_id).await?;
    download.output_file = Some(output_file.to_string());
    update_download(&download).await?;
    Ok(())
}

pub async fn confirm_download_data(download_id: i64) -> Result<(), DBError> {
    let mut download = get_download_by_id(download_id).await?;
    download.data_confirmed = true;
    update_download(&download).await?;
    Ok(())
}

fn string_to_option(string: String) -> Option<String> {
    if string == "NULL" {
        None
    } else {
        Some(string)
    }
}
