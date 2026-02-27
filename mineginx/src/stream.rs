use tokio::{
    task::JoinHandle,
    net::tcp::{OwnedReadHalf, OwnedWriteHalf},
    io::{AsyncReadExt, AsyncWriteExt},
};

/// Forwards data from `reader` to `writer` until EOF or error,
/// then shuts down the writer (sends TCP FIN).
pub fn forward_half(
    mut reader: OwnedReadHalf,
    mut writer: OwnedWriteHalf,
    buffer_size: usize,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut buf = vec![0; buffer_size];
        loop {
            match reader.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if writer.write_all(&buf[..n]).await.is_err() {
                        break;
                    }
                }
            }
        }
        _ = writer.shutdown().await;
    })
}
