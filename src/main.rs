mod cache;
mod cli;
mod config;
mod errors;
mod install;
mod shim;
mod version;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    cli::run().await
}
