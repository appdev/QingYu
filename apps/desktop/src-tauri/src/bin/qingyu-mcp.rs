#[tokio::main]
async fn main() {
    if let Err(error) = markra_lib::run_mcp_bridge().await {
        eprintln!("qingyu-mcp: {error}");
        std::process::exit(1);
    }
}
