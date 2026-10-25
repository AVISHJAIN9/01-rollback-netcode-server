use rollback_netcode_server::network::udp_server::UdpServer;
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt::init();
    tracing::info!("Starting Rollback Netcode Server on 0.0.0.0:9000...");

    let server = UdpServer::new("0.0.0.0:9000".parse()?);
    server.run().await?;

    Ok(())
}
