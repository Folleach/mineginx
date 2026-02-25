use tokio::{
    task::JoinHandle,
    sync::oneshot::{
        Sender, Receiver
    },
    net::tcp::{
        OwnedReadHalf, OwnedWriteHalf
    },
    io::{AsyncReadExt, AsyncWriteExt}
};

pub fn forward_stream(
    close: Sender<()>,
    mut close_by_other: Receiver<()>,
    mut reader: OwnedReadHalf,
    mut writer: OwnedWriteHalf,
    buffer_size: usize) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut buf = vec![0; buffer_size];
        loop {
            tokio::select! {
                _ = &mut close_by_other => {
                    break;
                },
                res = reader.read(&mut buf) => {
                    match res {
                        Ok(0) | Err(_) => break,
                        Ok(size) => {
                            if writer.write_all(&buf[..size]).await.is_err() {
                                break;
                            }
                        }
                    }
                }
            }
        }
        // Signal the other forwarding task to stop (dropping the sender
        // causes the receiver to resolve, which triggers its close_by_other branch)
        drop(close);
        // Shut down write half so the remote end receives FIN immediately
        // instead of waiting for its own keepalive/timeout to detect the dead connection
        _ = writer.shutdown().await;
    })
}
