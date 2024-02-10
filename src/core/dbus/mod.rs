use std::sync::Arc;

use tokio::sync::{
    broadcast::{Receiver, Sender},
    Mutex,
};
use zbus::{Result, SignalContext};
use zbus_macros::dbus_interface;

use crate::core::db;

use super::download::{Download, DownloadEvent};

pub struct FlowListener {
    events_rx: Arc<Mutex<Receiver<DownloadEvent>>>,
    events_tx: Sender<DownloadEvent>,
}

impl FlowListener {
    pub fn new(
        events_rx: Receiver<DownloadEvent>,
        events_tx: Sender<DownloadEvent>,
    ) -> FlowListener {
        FlowListener {
            events_rx: Arc::new(Mutex::new(events_rx)),
            events_tx,
        }
    }

    pub async fn listen_to_events(&self, ctx: SignalContext<'_>) {
        while let Ok(event) = self.events_rx.lock().await.recv().await {
            match event {
                DownloadEvent::DownloadProgress(id, progress, content_length) => {
                    Self::notify_download_progress(&ctx, id, progress, content_length)
                        .await
                        .unwrap();
                }
                DownloadEvent::DownloadUpdate(download_info) => {
                    Self::notify_download_update(&ctx, download_info)
                        .await
                        .unwrap();
                }
                _ => {}
            }
        }
    }
}

#[dbus_interface(name = "com.github.essmehdi.Flowd")]
impl FlowListener {
    // Methods
    async fn status(&self) -> &str {
        "UP"
    }

    async fn get_all_downloads(&self) -> Vec<Download> {
        log::info!("Getting all downloads");
        db::get_all_downloads().await
    }

    async fn get_downloads_by_completed_status(&self, completed: bool) -> Vec<Download> {
        log::info!("Getting downloads by completed status: {}", completed);
        if completed {
            db::get_completed_downloads().await
        } else {
            db::get_uncompleted_downloads().await
        }
    }

    async fn get_downloads_by_category(&self, category: &str) -> Vec<Download> {
        log::info!("Getting downloads by category: {}", category);
        db::get_downloads_by_category(category).await
    }

    async fn get_sorted_downloads(&self) -> Vec<Download> {
        log::info!("Getting sorted downloads");
        db::get_sorted_downloads().await
    }

    async fn new_download_wait_confirm(&self, url: &str) -> &str {
        log::info!("New download with data unconfirmed: {}", url);
        match self
            .events_tx
            .send(DownloadEvent::NewDownload(url.to_string(), false))
        {
            Ok(_) => "OK",
            Err(err) => {
                log::error!("Error sending new download event: {}", err);
                "ERROR"
            }
        }
    }

    async fn new_download_confirmed(&self, url: &str) -> &str {
        log::info!("New download with data confirmed: {}", url);
        match self
            .events_tx
            .send(DownloadEvent::NewDownload(url.to_string(), true))
        {
            Ok(_) => "OK",
            Err(err) => {
                log::error!("Error sending new download event: {}", err);
                "ERROR"
            }
        }
    }

    async fn pause_download(&self, id: i64) -> &str {
        log::info!("Pausing download with id: {}", id);
        match self.events_tx.send(DownloadEvent::PauseDownload(id)) {
            Ok(_) => "OK",
            Err(err) => {
                log::error!("Error sending pause download event: {}", err);
                "ERROR"
            }
        }
    }

    async fn cancel_download(&self, id: i64) -> &str {
        log::info!("Cancelling download with id: {}", id);
        match self.events_tx.send(DownloadEvent::CancelDownload(id)) {
            Ok(_) => "OK",
            Err(err) => {
                log::error!("Error sending cancel download event: {}", err);
                "ERROR"
            }
        }
    }

    async fn change_output_file_path(&self, id: i64, new_path: &str) -> &str {
        log::info!("Changing output file path for download with id: {}", id);
        db::change_download_output_file_path(id, new_path).await;
        "OK"
    }

    async fn confirm_download_data(&self, id: i64) -> &str {
        log::info!("Confirming download data for download with id: {}", id);
        db::confirm_download_data(id).await;
        "OK"
    }

    // Signals
    #[dbus_interface(signal)]
    async fn notify_download_error(ctx: &SignalContext<'_>, id: i64, error: &str) -> Result<()>;

    #[dbus_interface(signal)]
    async fn notify_download_update(ctx: &SignalContext<'_>, download_info: Download)
        -> Result<()>;

    #[dbus_interface(signal)]
    async fn notify_download_progress(
        ctx: &SignalContext<'_>,
        id: i64,
        progress: u64,
        content_length: u64,
    ) -> Result<()>;
}
