use chrono::Local;
use log;
use reqwest::header::{HeaderMap, RANGE};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
use tokio::sync::broadcast::{Receiver, Sender};
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration, Instant};
use zbus::zvariant::{DeserializeDict, SerializeDict, Type};
use zbus::fdo;

use super::config::{self, Config};
use super::db;

mod utils;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Type, SerializeDict, DeserializeDict)]
#[zvariant(signature = "dict")]
pub struct Download {
    pub id: i64,
    pub url: String,
    pub status: DownloadStatus,
    pub data_confirmed: bool,
    pub detected_output_file: Option<String>,
    pub output_file: Option<String>,
    pub temp_file: String,
    pub resumable: bool,
    pub date_added: i64,
    pub date_completed: Option<i64>,
    pub size: Option<u64>,
}

impl Download {
    async fn get_download_from_url(url: String, config: &Config) -> Download {
        Download {
            id: 0,
            url,
            status: DownloadStatus::Pending,
            data_confirmed: false,
            temp_file: utils::get_temp_file(config).await,
            detected_output_file: None,
            output_file: None,
            resumable: false,
            date_added: Local::now().timestamp(),
            date_completed: None,
            size: None,
        }
    }

    async fn refresh_data_from_db(&mut self) {
        let download = db::get_download_by_id(self.id).await;
        self.url = download.url;
        self.status = download.status;
        self.data_confirmed = download.data_confirmed;
        self.detected_output_file = download.detected_output_file;
        self.output_file = download.output_file;
        self.temp_file = download.temp_file;
        self.resumable = download.resumable;
        self.date_added = download.date_added;
        self.date_completed = download.date_completed;
        self.size = download.size;
    }

    async fn change_download_status(&mut self, new_status: DownloadStatus) {
        self.status = new_status;
        db::change_download_status(&self.id, &self.status).await;
    }
}

#[derive(Debug, Clone, Type, Serialize, Deserialize)]
pub enum DownloadStatus {
    Pending,
    Starting,
    InProgress,
    Paused,
    Canceled,
    Completed,
    ServerError,
    ClientError,
    UnknownError,
}

impl DownloadStatus {
    pub fn get_description(&self) -> &str {
        match self {
            DownloadStatus::Pending => "Pending",
            DownloadStatus::Starting => "Starting",
            DownloadStatus::InProgress => "In progress",
            DownloadStatus::Paused => "Paused",
            DownloadStatus::Canceled => "Canceled",
            DownloadStatus::Completed => "Completed",
            DownloadStatus::ServerError => "Server error",
            DownloadStatus::ClientError => "Client error",
            DownloadStatus::UnknownError => "Unknown error",
        }
    }

    pub fn get_string(&self) -> &str {
        match self {
            DownloadStatus::Pending => "pending",
            DownloadStatus::Starting => "starting",
            DownloadStatus::InProgress => "in_progress",
            DownloadStatus::Paused => "paused",
            DownloadStatus::Canceled => "canceled",
            DownloadStatus::Completed => "completed",
            DownloadStatus::ServerError => "server_error",
            DownloadStatus::ClientError => "client_error",
            DownloadStatus::UnknownError => "unknown_error",
        }
    }

    pub fn from_string(value: &str) -> DownloadStatus {
        match value {
            "pending" => DownloadStatus::Pending,
            "starting" => DownloadStatus::Starting,
            "in_progress" => DownloadStatus::InProgress,
            "paused" => DownloadStatus::Paused,
            "canceled" => DownloadStatus::Canceled,
            "completed" => DownloadStatus::Completed,
            "server_error" => DownloadStatus::ServerError,
            "client_error" => DownloadStatus::ClientError,
            "unknown_error" => DownloadStatus::UnknownError,
            _ => panic!("Invalid download status"),
        }
    }
}

pub struct FileInfo {
    file_name: String,
    content_length: Option<u64>,
    content_type: Option<String>,
    resumable: bool,
}

#[derive(Clone, Debug)]
pub enum DownloadEvent {
    // Events
    NewDownload(String, bool),
    PauseDownload(i64),
    CancelDownload(i64),
    // Signals
    DownloadProgress(i64, u64, u64),
    DownloadUpdate(Download),
}

pub struct Downloader {
    pause_requests: Arc<Mutex<HashSet<i64>>>,
    cancel_requests: Arc<Mutex<HashSet<i64>>>,
    downloading: Arc<Mutex<HashSet<i64>>>,
    events_tx: Sender<DownloadEvent>,
    events_rx: Arc<Mutex<Receiver<DownloadEvent>>>,
}

impl Downloader {
    pub fn new(
        tx: Sender<DownloadEvent>,
        rx: Receiver<DownloadEvent>,
    ) -> Downloader {
        Downloader {
            pause_requests: Arc::new(Mutex::new(HashSet::new())),
            cancel_requests: Arc::new(Mutex::new(HashSet::new())),
            downloading: Arc::new(Mutex::new(HashSet::new())),
            events_tx: tx,
            events_rx: Arc::new(Mutex::new(rx)),
        }
    }

    pub async fn listen_to_dbus_events(&self) {
        while let Ok(event) = self.events_rx.lock().await.recv().await {
            match event {
                DownloadEvent::NewDownload(url, confirm) => {
                    self.new_download(url, confirm).await;
                }
                DownloadEvent::PauseDownload(id) => self.request_pause(id).await,
                _ => {}
            }
        }
    }

    pub async fn new_download(&self, url: String, confirm: bool) {
        let config = config::get_config().await;
        let mut download_info = Download::get_download_from_url(url, &config).await;
        download_info.data_confirmed = confirm;
        db::new_download(&download_info).await;
    }

    pub async fn request_pause(&self, download_id: i64) {
        self.pause_requests.lock().await.insert(download_id);
    }

    pub async fn request_cancel(&self, download_id: i64) {
        self.cancel_requests.lock().await.insert(download_id);
    }

    /**
     * The downloader function
     * It is used to start a download that has been added to database
     */
    pub async fn download(&self, download_id: i64) -> fdo::Result<()> {
        let config = config::get_config().await;

        log::info!("Starting download #{}", download_id);
        self.downloading.lock().await.insert(download_id);

        let mut download_info = db::get_download_by_id(download_id).await;

        let mut start_byte: Option<u128> = None;

        // Check if the download was already completed
        if let DownloadStatus::Completed = download_info.status {
            log::error!(
                "Download #{}: Download is already completed",
                &download_info.id
            );

            self.downloading.lock().await.remove(&download_id);
            return Ok(());
        }

        // Check if the download was already started and paused
        if let DownloadStatus::Paused = download_info.status {
            log::info!("Download #{}: Resuming download", &download_info.id);

            let temp_file = OpenOptions::new()
                .read(true)
                .open(&download_info.temp_file)
                .await
                .unwrap();

            let downloaded_size = temp_file.metadata().await.unwrap().len();
            start_byte = Some(downloaded_size as u128);
        }

        download_info
            .change_download_status(DownloadStatus::Starting)
            .await;
        self.events_tx
            .send(DownloadEvent::DownloadUpdate(download_info.clone()))
            .unwrap();

        // Create client
        let mut client_builder = reqwest::Client::builder().user_agent(&config.user_agent);

        // Start from byte if resumed download
        if let Some(byte) = start_byte {
            let mut headers = HeaderMap::new();
            headers.insert(RANGE, format!("bytes={}-", byte).parse().unwrap());
            client_builder = client_builder.default_headers(headers);
        }
        let client = client_builder.build().unwrap();

        download_info
            .change_download_status(DownloadStatus::InProgress)
            .await;
        self.events_tx
            .send(DownloadEvent::DownloadUpdate(download_info.clone()))
            .unwrap();

        log::debug!("Download #{}: Sending request...", &download_id);

        // Perform request
        let mut resp = client.get(&download_info.url).send().await.unwrap();

        // Check if request was successful
        if let Err(err) = resp.error_for_status_ref() {
            log::error!("Download #{}: Unsuccessful response: {}", &download_id, err);

            db::change_download_status(&download_id, &DownloadStatus::ServerError).await;
            self.events_tx
                .send(DownloadEvent::DownloadUpdate(download_info))
                .unwrap();

            self.downloading.lock().await.remove(&download_id);
            return Ok(());
        }

        // Get file info
        let file_info = utils::get_file_info_from_headers(&resp.url().as_str(), resp.headers());

        // Detect output file
        download_info.detected_output_file = Some(utils::get_output_file_path(&file_info, &config).await);
        download_info.size = file_info.content_length;
        db::update_download(&download_info).await;

        log::info!(
            "Download #{}: Detected file name {}",
            &download_id,
            &file_info.file_name
        );

        // Check if file is resumable
        if file_info.resumable {
            download_info.resumable = true;
            db::update_download(&download_info).await;
            self.events_tx
                .send(DownloadEvent::DownloadUpdate(download_info.clone()))
                .unwrap();
        }

        // Write content to temp file
        let mut file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(&download_info.temp_file)
            .await
            .unwrap();

        log::debug!(
            "Download #{}: Writing to {}",
            &download_id,
            &download_info.temp_file
        );

        // Get temp file size in case of resuming
        let mut progress = file.metadata().await.unwrap().len();
        let mut progress_mark = Instant::now();
        while let Some(chunk) = resp.chunk().await.unwrap() {
            // Check cancel requests
            if self.cancel_requests.lock().await.contains(&download_id) {
                log::info!("Download #{}: Cancelled", &download_id);
                download_info
                    .change_download_status(DownloadStatus::Canceled)
                    .await;
                self.events_tx
                    .send(DownloadEvent::DownloadUpdate(download_info.clone()))
                    .unwrap();

                // Delete temp file
                tokio::fs::remove_file(&download_info.temp_file)
                    .await
                    .unwrap();

                self.downloading.lock().await.remove(&download_id);
                return Ok(());
            }
            // Check pause requests
            if self.pause_requests.lock().await.contains(&download_id) {
                log::info!("Download #{}: Paused", &download_id);
                download_info
                    .change_download_status(DownloadStatus::Paused)
                    .await;
                self.events_tx
                    .send(DownloadEvent::DownloadUpdate(download_info.clone()))
                    .unwrap();

                self.downloading.lock().await.remove(&download_id);
                return Ok(());
            }

            file.write_all(&chunk).await.unwrap();
            progress += chunk.len() as u64;
            if (Instant::now() - progress_mark) > Duration::from_secs(1) {
                progress_mark = Instant::now();
                self.events_tx
                    .send(DownloadEvent::DownloadProgress(
                        download_id,
                        progress,
                        file_info.content_length.unwrap_or(0),
                    ))
                    .unwrap();
            }
        }

        // Wait for file metadata confirmation
        download_info.refresh_data_from_db().await;
        while !&download_info.data_confirmed {
            log::info!(
                "Download #{}: Waiting for download data confirmation...",
                &download_id
            );
            sleep(Duration::from_secs(1)).await;
            download_info.refresh_data_from_db().await;
        }

        // Get output path
        let file_output = if let Some(user_output_file) = &download_info.output_file {
            user_output_file.clone()
        } else {
            match &download_info.detected_output_file {
                Some(output_file) => output_file.clone(),
                None => utils::get_output_file_path(&file_info, &config).await,
            }
        };

        let file_output = utils::get_conflict_free_file_path(&file_output);

        log::info!(
            "Download #{}: Moving file to {}",
            &download_id,
            &file_output
        );

        // Check if path exists
        let output_path_parent = Path::new(&file_output).parent();
        if output_path_parent.is_none() || !output_path_parent.unwrap().exists() {
            log::error!(
                "Download #{}: Output directory {} does not exist",
                &download_id,
                output_path_parent.unwrap_or(Path::new("")).to_str().unwrap()
            );
            download_info.change_download_status(DownloadStatus::ClientError).await;
            self.events_tx
                .send(DownloadEvent::DownloadUpdate(download_info))
                .unwrap();
            return Ok(());
        }

        // Move file from temp to output
        tokio::fs::rename(&download_info.temp_file, &file_output)
            .await
            .unwrap();

        // Save conflict free path to database
        if !download_info.output_file.is_none() && download_info.output_file.as_ref().unwrap() != &file_output {
            download_info.output_file = Some(file_output);
            db::update_download(&download_info).await;
        } else if download_info.output_file.is_none() && download_info.detected_output_file.as_ref().unwrap() != &file_output {
            download_info.output_file = Some(file_output);
            db::update_download(&download_info).await;
        }
        
        log::info!("Download #{}: Completed", &download_id);

        download_info.date_completed = Some(Local::now().timestamp());
        db::update_download(&download_info).await;

        // Change download status to completed
        download_info
            .change_download_status(DownloadStatus::Completed)
            .await;
        self.events_tx
            .send(DownloadEvent::DownloadUpdate(download_info))
            .unwrap();

        self.downloading.lock().await.remove(&download_id);
        Ok(())
    }
}
