use tokio::{
    task::JoinHandle,
    sync::oneshot::{
        Sender, Receiver, error::TryRecvError
    },
    net::tcp::{
        OwnedReadHalf, OwnedWriteHalf
    },
    io::{AsyncReadExt, AsyncWriteExt}
};

pub fn forward_stream(
    close: Sender<()>,
    close_by_other: Receiver<()>,
    mut reader: OwnedReadHalf,
    mut writer: OwnedWriteHalf) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut buf = vec![0; 2048];
        let mut close = Some(close);
        let mut close_by_other = Some(close_by_other);
        let mut closed = false;
        loop {
            if let Some(mut receiver) = close_by_other.take() {
                match receiver.try_recv() {
                    Err(e ) => closed |= e == TryRecvError::Closed,
                    Ok(_) => closed = true
                }
            }
            if closed {
                return;
            }
            let res = reader.read(&mut buf).await;
            match res {
                Ok(size) => {
                    if size == 0 {
                        if let Some(sender) = close.take() {
                            closed = true;
                            match sender.send(()) {
                                Err(_) => { },
                                Ok(_) => { }
                            }
                        }
                    }
                    let writed = writer.write_all(&buf[..size]).await;
                    match writed {
                        Ok(_) => { },
                        Err(_) => {
                            if let Some(sender) = close.take() {
                                match sender.send(()) {
                                    Err(_) => { },
                                    Ok(_) => { }
                                }
                            }
                            return;
                        }
                    }
                },
                Err(_) => {
                    if let Some(sender) = close.take() {
                        match sender.send(()) {
                            Err(_) => { },
                            Ok(_) => { }
                        }
                    }
                    return;
                }
            }
        }
    })
}
