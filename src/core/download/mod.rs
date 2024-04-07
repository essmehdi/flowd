use chrono::Local;
use log;
use reqwest::header::{HeaderMap, RANGE};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use thiserror::Error;
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;
use tokio::sync::broadcast::error::SendError;
use tokio::sync::broadcast::{Receiver, Sender};
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration, Instant};
use zbus::fdo;
use zbus::zvariant::{DeserializeDict, SerializeDict, Type};

use super::config::{self, Config};
use super::db::{self, DBError};

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

        if let Err(e) = download {
            log::error!("Download #{}: {}", self.id, e);
            return;
        }

        let download = download.unwrap();

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

    async fn change_download_status(&mut self, new_status: DownloadStatus) -> Result<(), DBError> {
        self.status = new_status;
        db::change_download_status(&self.id, &self.status).await?;
        Ok(())
    }

    async fn sync_to_db(&self) -> Result<(), DBError> {
        db::update_download(self).await?;
        Ok(())
    }

    fn is_idle(&self) -> bool {
        match self.status {
            DownloadStatus::Paused
            | DownloadStatus::Canceled
            | DownloadStatus::ClientError
            | DownloadStatus::ServerError
            | DownloadStatus::UnknownError => true,
            _ => false,
        }
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
    ResumeDownload(i64),
    RestartDownload(i64),
    CancelDownload(i64),
    DeleteDownload(i64),
    // Signals
    DownloadProgress(i64, u64, u64),
    DownloadUpdate(Download),
    DownloadError(Option<i64>, String),
}

#[derive(Debug, Error)]
pub enum DownloaderError {
    #[error("Database error: {0}")]
    DBError(#[from] DBError),

    #[error("Channel error: {0}")]
    ChannelError(#[from] SendError<DownloadEvent>),
}

pub struct Downloader {
    pause_requests: Arc<Mutex<HashSet<i64>>>,
    cancel_requests: Arc<Mutex<HashSet<i64>>>,
    downloading: Arc<Mutex<HashSet<i64>>>,
    events_tx: Sender<DownloadEvent>,
    events_rx: Arc<Mutex<Receiver<DownloadEvent>>>,
}

impl Downloader {
    pub fn new(tx: Sender<DownloadEvent>, rx: Receiver<DownloadEvent>) -> Downloader {
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
            _ = self.handle_event(event).await.map_err(|e| {
                log::error!("Error handling event: {:?}", e);
            });
        }
    }

    pub async fn handle_event(&self, event: DownloadEvent) -> Result<(), DownloaderError> {
        match event {
            DownloadEvent::NewDownload(url, confirm) => {
                self.new_download(url, confirm).await?;
            }
            DownloadEvent::PauseDownload(id) => {
                if self.downloading.lock().await.contains(&id) {
                    self.request_pause(id).await;
                }
            }
            DownloadEvent::ResumeDownload(id) => {
                let download = db::get_download_by_id(id).await?;

                if let DownloadStatus::Paused = download.status {
                    db::change_download_status(&id, &DownloadStatus::Pending).await?;
                }
            }
            DownloadEvent::RestartDownload(id) => {
                let download = db::get_download_by_id(id).await?;

                if download.is_idle() {
                    if fs::try_exists(&download.temp_file).await.unwrap_or(false) {
                        _ = utils::empty_temp_file(&download.temp_file).await;
                    }
                    db::change_download_status(&id, &DownloadStatus::Pending).await?;
                }
            }
            DownloadEvent::CancelDownload(id) => {
                if self.downloading.lock().await.contains(&id) {
                    self.request_cancel(id).await
                } else {
                    let mut download = db::get_download_by_id(id).await?;
                    self.cancel_download(&mut download).await?;
                }
            }
            DownloadEvent::DeleteDownload(id) => {
                let download = db::get_download_by_id(id).await?;

                if download.is_idle() {
                    db::delete_download(id).await?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    pub async fn new_download(&self, url: String, confirm: bool) -> Result<(), DBError> {
        let config = config::get_config().await;
        let mut download_info = Download::get_download_from_url(url, &config).await;
        download_info.data_confirmed = confirm;
        db::new_download(&download_info).await?;
        Ok(())
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

        let download = db::get_download_by_id(download_id).await;
        if let Err(e) = download {
            log::error!("{e}");
            return Ok(());
        }
        let mut download = download.unwrap();

        let mut start_byte: Option<u128> = None;

        self.prepare_download(&mut download, &mut start_byte).await;

        _ = self
            .update_download_status_and_notify(&mut download, DownloadStatus::Starting)
            .await
            .map_err(|e| {
                log::error!("{e}");
            });

        let client = self.create_client(start_byte, &config).await;
        if let Err(e) = client {
            log::error!("{e}");
            _ = self
                .update_download_status_and_notify(&mut download, DownloadStatus::ClientError)
                .await;
            return Ok(());
        }
        let client = client.unwrap();

        _ = self
            .update_download_status_and_notify(&mut download, DownloadStatus::InProgress)
            .await;

        log::debug!("Download #{}: Sending request...", &download_id);

        // Perform request
        let mut resp = client.get(&download.url).send().await.unwrap();

        // Check if request was successful
        if let Err(err) = resp.error_for_status_ref() {
            log::error!("Download #{}: Unsuccessful response: {}", &download_id, err);

            _ = self
                .update_download_status_and_notify(&mut download, DownloadStatus::ServerError)
                .await;

            self.downloading.lock().await.remove(&download_id);
            return Ok(());
        }

        // Get file info
        let file_info = utils::get_file_info_from_headers(&resp.url().as_str(), resp.headers());

        // Detect output file
        if let None = download.detected_output_file {
            download.detected_output_file =
                Some(utils::get_output_file_path(&file_info, &config).await);
        }
        if let None = download.size {
            download.size = file_info.content_length;
        }
        _ = self.update_download_in_db_and_notify(&download).await;

        log::info!(
            "Download #{}: Detected file name {}",
            &download_id,
            &file_info.file_name
        );

        // Check if file is resumable
        if file_info.resumable {
            download.resumable = true;
            _ = self.update_download_in_db_and_notify(&download).await;
        }

        // Write content to temp file
        let file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(&download.temp_file)
            .await;
        if let Err(e) = &file {
            log::error!("{e}");
            _ = self.update_download_status_and_notify(&mut download, DownloadStatus::ClientError);
        }
        let mut file = file.unwrap();

        log::debug!(
            "Download #{}: Writing to {}",
            &download_id,
            &download.temp_file
        );

        // Get temp file size in case of resuming
        let mut progress = file.metadata().await.unwrap().len();
        let mut progress_mark = Instant::now();
        let initial_progress_mark = progress_mark.clone();
        while let Some(chunk) = resp.chunk().await.unwrap() {
            if (Instant::now() - progress_mark) > Duration::from_millis(250)
                || initial_progress_mark == progress_mark
            {
                progress_mark = Instant::now();
                self.events_tx
                    .send(DownloadEvent::DownloadProgress(
                        download_id,
                        progress,
                        download.size.unwrap_or(0),
                    ))
                    .unwrap();
            }

            // Check cancel requests
            if self.cancel_requests.lock().await.contains(&download_id) {
                _ = self.cancel_download(&mut download).await;
                self.downloading.lock().await.remove(&download_id);
                return Ok(());
            }
            // Check pause requests
            if self.pause_requests.lock().await.contains(&download_id) {
                _ = self.pause_download(&mut download).await;
                self.downloading.lock().await.remove(&download_id);
                return Ok(());
            }

            file.write_all(&chunk).await.unwrap();
            progress += chunk.len() as u64;
        }

        // Wait for file metadata confirmation
        download.refresh_data_from_db().await;
        while !&download.data_confirmed {
            log::info!(
                "Download #{}: Waiting for download data confirmation...",
                &download_id
            );
            sleep(Duration::from_secs(1)).await;
            download.refresh_data_from_db().await;
        }

        // Get output path
        let file_output = if let Some(user_output_file) = &download.output_file {
            user_output_file.clone()
        } else {
            match &download.detected_output_file {
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
                output_path_parent
                    .unwrap_or(Path::new(""))
                    .to_str()
                    .unwrap()
            );
            _ = self
                .update_download_status_and_notify(&mut download, DownloadStatus::ClientError)
                .await;
            return Ok(());
        }

        // Move file from temp to output
        tokio::fs::rename(&download.temp_file, &file_output)
            .await
            .unwrap();

        // Save conflict free path to database
        if (!download.output_file.is_none()
            && download.output_file.as_ref().unwrap() != &file_output)
            || (download.output_file.is_none()
                && download.detected_output_file.as_ref().unwrap() != &file_output)
        {
            download.output_file = Some(file_output);
            _ = self.update_download_in_db_and_notify(&download).await;
        }

        log::info!("Download #{}: Completed", &download_id);

        download.date_completed = Some(Local::now().timestamp());
        _ = self.update_download_in_db_and_notify(&download).await;

        // Change download status to completed
        _ = self
            .update_download_status_and_notify(&mut download, DownloadStatus::Completed)
            .await;

        self.downloading.lock().await.remove(&download_id);
        Ok(())
    }

    /// Prepare download by checking if temp file has data and setting start byte
    ///
    /// # Arguments
    ///
    /// * `download` - The download to be prepared
    /// * `start_byte` - The byte to start from if resumed download
    async fn prepare_download(&self, download: &mut Download, start_byte: &mut Option<u128>) {
        self.downloading.lock().await.insert(download.id);

        // Check if temp file has data
        if fs::try_exists(&download.temp_file).await.unwrap_or(false) {
            let temp_file = OpenOptions::new()
                .read(true)
                .open(&download.temp_file)
                .await
                .unwrap();

            let downloaded_size = temp_file.metadata().await.unwrap().len();
            if downloaded_size > 0 {
                if download.resumable {
                    log::info!(
                        "Download #{}: Resuming download from byte {}",
                        &download.id,
                        downloaded_size
                    );
                    *start_byte = Some(downloaded_size as u128);
                } else {
                    temp_file.set_len(0).await.unwrap();
                }
            }
        }
    }

    /// Create client with user agent and bytes header if resumed download
    ///
    /// # Arguments
    ///
    /// * `start_byte` - The byte to start from if resumed download
    /// * `config` - The configuration to get user agent from
    ///
    /// # Returns
    ///
    /// * `reqwest::Client` - The new Reqwest client
    async fn create_client(
        &self,
        start_byte: Option<u128>,
        config: &Config,
    ) -> reqwest::Result<Client> {
        // Create client
        let mut client_builder = reqwest::Client::builder().user_agent(&config.user_agent);

        // Start from byte if resumed download
        if let Some(byte) = start_byte {
            let mut headers = HeaderMap::new();
            headers.insert(RANGE, format!("bytes={}-", byte).parse().unwrap());
            client_builder = client_builder.default_headers(headers);
        }

        client_builder.build()
    }

    /// Pause download, save in database and notify in DBus
    ///
    /// # Arguments
    ///
    /// * `download` - The download to be paused
    async fn pause_download(&self, download: &mut Download) -> Result<(), DownloaderError> {
        log::info!("Download #{}: Paused", &download.id);
        download
            .change_download_status(DownloadStatus::Paused)
            .await
            .map_err(|e| {
                log::error!("{e}");
                e
            })?;
        self.events_tx
            .send(DownloadEvent::DownloadUpdate(download.clone()))
            .map_err(|e| {
                log::error!("{e}");
                e
            })?;

        self.pause_requests.lock().await.remove(&download.id);

        Ok(())
    }

    /// Cancel download and delete temp file
    ///
    /// # Arguments
    ///
    /// * `download` - The download to be cancelled
    async fn cancel_download(&self, download: &mut Download) -> Result<(), DownloaderError> {
        log::info!("Download #{}: Cancelled", &download.id);
        download
            .change_download_status(DownloadStatus::Canceled)
            .await
            .map_err(|e| {
                log::error!("{e}");
                e
            })?;
        self.events_tx
            .send(DownloadEvent::DownloadUpdate(download.clone()))
            .map_err(|e| {
                log::error!("{e}");
                e
            })?;

        _ = utils::empty_temp_file(&download.temp_file).await;

        self.cancel_requests.lock().await.remove(&download.id);

        Ok(())
    }

    /// Change download status, save in database and notify in DBus
    ///
    /// # Arguments
    ///
    /// * `download` - The download to update status for
    /// * `new_status` - The new status to be set
    async fn update_download_status_and_notify(
        &self,
        download: &mut Download,
        new_status: DownloadStatus,
    ) -> Result<(), DownloaderError> {
        download
            .change_download_status(new_status)
            .await
            .map_err(|e| {
                log::error!("{e}");
                e
            })?;
        self.events_tx
            .send(DownloadEvent::DownloadUpdate(download.clone()))
            .map_err(|e| {
                log::error!("{e}");
                e
            })?;
        Ok(())
    }

    /// Save download changes in database and notify in DBus
    ///
    /// # Arguments
    ///
    /// * `download` - The download with new data to be save in db
    async fn update_download_in_db_and_notify(
        &self,
        download: &Download,
    ) -> Result<(), DownloaderError> {
        download.sync_to_db().await.map_err(|e| {
            log::error!("{e}");
            e
        })?;
        self.events_tx
            .send(DownloadEvent::DownloadUpdate(download.clone()))
            .map_err(|e| {
                log::error!("{e}");
                e
            })?;
        Ok(())
    }

    /// Report error through DBus
    ///
    /// # Arguments
    ///
    /// * `download_id` - The download id to report error for
    /// * `error` - The error message to be reported
    fn report_error(&self, download_id: Option<i64>, error: &str) -> Result<(), DownloaderError> {
        self.events_tx
            .send(DownloadEvent::DownloadError(download_id, error.to_string()))
            .map_err(|e| {
                log::error!("{e}");
                e
            })?;
        Ok(())
    }
}
