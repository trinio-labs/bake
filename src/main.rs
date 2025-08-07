#[tokio::main]
async fn main() -> anyhow::Result<()> {
    bake::run().await
}
