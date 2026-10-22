// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

#[tokio::main]
async fn main() -> playwright_rs::Result<()> {
    let mut browsers: Vec<String> = std::env::args().skip(1).collect();
    let with_deps = browsers.iter().any(|arg| arg == "--with-deps");
    browsers.retain(|arg| arg != "--with-deps");
    let names: Vec<&str> = browsers.iter().map(String::as_str).collect();
    let names = (!names.is_empty()).then_some(names.as_slice());
    if with_deps {
        playwright_rs::install_browsers_with_deps(names).await
    } else {
        playwright_rs::install_browsers(names).await
    }
}
