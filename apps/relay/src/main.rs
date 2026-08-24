//! Relay process entry point.

use anyhow::Result;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("leave=info,tower_http=info")),
        )
        .json()
        .init();

    let (config, bind) = leave_relay::config_from_environment().await?;
    leave_relay::serve(config, bind).await
}
