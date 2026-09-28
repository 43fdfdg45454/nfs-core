//! Entry point for the fuzz targets (fuzz/): what a server sends, from the socket on.

/// A byte stream as a server would send it: each record read (record marking) and parsed.
pub fn records(data: &[u8]) {
    thread_local! {
        static RUNTIME: tokio::runtime::Runtime =
            tokio::runtime::Builder::new_current_thread().build().expect("a runtime");
    }
    RUNTIME.with(|runtime| {
        runtime.block_on(async {
            let mut stream = data;
            while let Ok(record) = crate::record::read(&mut stream).await {
                _ = crate::incoming::parse(record);
            }
        })
    });
}
