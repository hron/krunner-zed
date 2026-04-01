use std::future::pending;

use krunner_zed::ZedRunner;
use zbus::connection;

#[tokio::main]
async fn main() -> zbus::Result<()> {
    let _conn = connection::Builder::session()?
        .name("dev.algus.krunner_zed")?
        .serve_at("/krunner_zed", ZedRunner)?
        .build()
        .await?;

    pending().await
}
