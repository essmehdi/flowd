use rusqlite::{Connection, Params, Result, params_from_iter};
use std::path::Path;
use tokio::fs::{self, File};

use crate::{core::download::DownloadStatus, utils};

use super::{download::Download, config};

const DB_DIR: &str = "~/.local/share/flow/";
const DB_NAME: &str = "downloads.db";

fn get_db_path() -> String {
    let db_path = Path::new(DB_DIR).join(DB_NAME);
    let db_path_string = db_path.to_string_lossy().to_string();
    utils::path::expand(&db_path_string)
}

async fn init_db() {
    let db_path = get_db_path();
    let db_exists = fs::metadata(&db_path).await.is_ok();
    if db_exists {
        return;
    }

    // Create DB directory and file
    fs::create_dir_all(utils::path::expand(DB_DIR))
        .await
        .unwrap();
    File::create(&db_path).await.unwrap();

    // Create downloads table
    let connection = Connection::open(&db_path).unwrap();
    connection
        .execute(
            "
        CREATE TABLE IF NOT EXISTS downloads (
            id INTEGER PRIMARY KEY,
            url TEXT NOT NULL,
            status TEXT NOT NULL,
            data_confirmed INTEGER NOT NULL,
            detected_output_file TEXT,
            output_file TEXT,            
            temp_file TEXT,
            resumable INTEGER NOT NULL,
            date_added INTEGER NOT NULL,
            date_completed INTEGER,
            size INTEGER
        )
        ",
            (),
        )
        .unwrap();
}

async fn connect() -> Result<Connection> {
    init_db().await;
    let db_path = get_db_path();
    Connection::open(&db_path)
}

pub async fn new_download(download: &Download) -> i64 {
    let completed_date = download.date_completed.and_then(|d| Some(d.to_string())).or(Some("NULL".to_string())).unwrap();
    let connection = connect().await.unwrap();
    connection
        .execute(
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
                &download.size.and_then(|size| Some(size.to_string())).unwrap_or("NULL".to_string())
            ],
        )
        .unwrap();
    connection.last_insert_rowid()
}

async fn get_downloads_from_query(query: &str, params: impl Params) -> Vec<Download> {
    let connection = connect().await.unwrap();
    let mut stmt = connection.prepare(query).unwrap();
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
    });

    let mut downloads = Vec::new();
    for download in downloads_iter.unwrap() {
        downloads.push(download.unwrap());
    }
    downloads
}

pub async fn get_all_downloads() -> Vec<Download> {
    get_downloads_from_query("SELECT * FROM downloads", []).await
}

pub async fn get_download_by_id(id: i64) -> Download {
    get_downloads_from_query("SELECT * FROM downloads WHERE id = ?1", [id])
        .await
        .pop()
        .unwrap()
}

pub async fn get_pending_downloads() -> Vec<Download> {
    get_downloads_from_query(
        "SELECT * FROM downloads WHERE status = ?1",
        [DownloadStatus::Pending.get_string()],
    )
    .await
}

pub async fn get_in_progress_downloads() -> Vec<Download> {
    get_downloads_from_query(
        "SELECT * FROM downloads WHERE status = ?1",
        [DownloadStatus::InProgress.get_string()],
    )
    .await
}

pub async fn get_completed_downloads() -> Vec<Download> {
    get_downloads_from_query(
        "SELECT * FROM downloads WHERE status = ?1",
        [DownloadStatus::Completed.get_string()],
    )
    .await
}

pub async fn get_uncompleted_downloads() -> Vec<Download> {
    get_downloads_from_query(
        "SELECT * FROM downloads WHERE status != ?1",
        [DownloadStatus::Completed.get_string()],
    )
    .await
}

pub async fn get_downloads_by_category(category: &str) -> Vec<Download> {
    let categories = config::get_categories().await;
    let category = categories.get(category).unwrap();

    let mut conditions: Vec<String> = vec![];
    for i in 0..category.extensions.len() {
        conditions.push(format!("output_file LIKE %?{} OR detected_output_file LIKE %?{}", i, i));
    }


    let query = format!("SELECT * FROM downloads WHERE {}", conditions.join(" OR "));
    // let extensions = category.extensions.iter().map(|x| x.as_str()).collect::<[&str]>()
    let downloads = get_downloads_from_query(
        query.as_str(),
        params_from_iter(category.extensions.iter())
    ).await;

    downloads
}

pub async fn update_download(download: &Download) {
    let completed_date = download.date_completed.and_then(|d| Some(d.to_string())).or(Some("NULL".to_string())).unwrap();
    let connection = connect().await.unwrap();
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
                &download.size.and_then(|size| Some(size.to_string())).unwrap_or("NULL".to_string()),
                &download.id.to_string(),
            ],
        )
        .unwrap();
}

pub async fn change_download_status(download_id: &i64, status: &DownloadStatus) {
    let connection = connect().await.unwrap();
    connection
        .execute(
            "
        UPDATE downloads
        SET status = ?1
        WHERE id = ?2
        ",
            &[status.get_string(), &download_id.to_string()],
        )
        .unwrap();
}

pub async fn change_download_output_file_path(download_id: i64, output_file: &str) {
    let mut download = get_download_by_id(download_id).await;
    download.output_file = Some(output_file.to_string());
    update_download(&download).await;
}

pub async fn confirm_download_data(download_id: i64) {
    let mut download = get_download_by_id(download_id).await;
    download.data_confirmed = true;
    update_download(&download).await;
}

fn string_to_option(string: String) -> Option<String> {
    if string == "NULL" {
        None
    } else {
        Some(string)
    }
}