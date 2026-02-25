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
                    return;
                },
                res = reader.read(&mut buf) => {
                    match res {
                        Ok(0) => {
                            _ = close.send(());
                            return;
                        },
                        Ok(size) => {
                            if writer.write_all(&buf[..size]).await.is_err() {
                                _ = close.send(());
                                return;
                            }
                        },
                        Err(_) => {
                            _ = close.send(());
                            return;
                        }
                    }
                }
            }
        }
    })
}
