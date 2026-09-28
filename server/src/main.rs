use jbehave_language_server::lsp;

#[tokio::main]
async fn main() {
    lsp::run().await;
}
