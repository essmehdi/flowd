use std::sync::Arc;
use tokio::{sync::broadcast, time::Duration};

use flow_lib::core::{
    config, db,
    dbus::FlowListener,
    download::{DownloadEvent, Downloader},
};
use zbus::{ConnectionBuilder, Result, SignalContext};

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let (tx, _) = broadcast::channel::<DownloadEvent>(32);

    // Initialize downloads controller
    let downloader_arc = Arc::new(Downloader::new(
        tx.clone(),
        tx.subscribe(),
    ));

    // Listen to events from DBus
    let events_listener_arc = Arc::clone(&downloader_arc);
    tokio::spawn(async move {
        events_listener_arc.listen_to_dbus_events().await;
    });

    // Initialize DBus connection
    let con = ConnectionBuilder::session()?
        .name("com.github.essmehdi.Flowd")?
        .serve_at(
            "/com/github/essmehdi/Flowd/Listener",
            FlowListener::new(tx.subscribe(), tx.clone()),
        )?
        .build()
        .await?;

    // Listen to signals from DownloadsController
    let signal_ctx = SignalContext::new(&con, "/com/github/essmehdi/Flowd/Broadcast")?;
    let listener = con
        .object_server()
        .interface::<_, FlowListener>("/com/github/essmehdi/Flowd/Listener")
        .await?;
    tokio::spawn(async move {
        listener.get().await.listen_to_events(signal_ctx).await;
    });

    loop {
        // TODO: Reload config only when needed by watching the config file
        let config = config::get_config().await;
        pending_downloads_checker(Arc::clone(&downloader_arc), config.max_sim_downloads).await;
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/**
 * This function is used to check for pending downloads
 * and start them.
 */
async fn pending_downloads_checker(controller: Arc<Downloader>, max_downloads: u16) {
    let mut in_progress_downloads_count = db::get_in_progress_downloads().await.len();
    let downloads = db::get_pending_downloads().await;
    for download in downloads {
        // Check if we reached the maximum number of downloads
        if in_progress_downloads_count >= max_downloads.into() {
            break;
        }
        let controller_clone = Arc::clone(&controller);
        tokio::spawn(async move {
            let _ = controller_clone.download(download.id).await;
        });
        in_progress_downloads_count += 1;
    }
}
