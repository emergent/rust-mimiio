//use rust_mimiio::MimiIO;
use anyhow::Context;
use std::collections::HashMap;
use std::env;

use std::path::PathBuf;
use structopt::StructOpt;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use futures_util::{future, pin_mut, FutureExt, StreamExt};
use std::net::ToSocketAddrs;
use tokio::time::Duration;
use tokio_tungstenite::client_async_tls;
use tokio_tungstenite::tungstenite::handshake::client::Request;
use tokio_tungstenite::tungstenite::Message;

use log::*;

const CHUNK_SIZE: usize = 8192;

#[derive(StructOpt, Debug)]
#[structopt(name = "basic")]
struct Opt {
    #[structopt(short, long, parse(from_os_str))]
    token: PathBuf,

    #[structopt(short, long, parse(from_os_str))]
    input: PathBuf,
}

async fn read_token(filename: PathBuf) -> anyhow::Result<String> {
    tokio::fs::read_to_string(filename)
        .await
        .context("couldn't read file")
        .map(|s| s.trim().to_string())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env::set_var("RUST_LOG", "debug");
    env_logger::init();

    let opt = Opt::from_args();
    info!("{:#?}", opt);

    let input_file = opt.input;
    let token_file = opt.token;
    let token = read_token(token_file).await?;
    let token_format = format!("Bearer {}", token);

    let mut headers = HashMap::new();
    headers.insert("x-mimi-process", "asr");
    headers.insert("x-mimi-input-language", "ja");
    headers.insert("Content-Type", "audio/x-pcm;bit=16;rate=16000;channels=1");
    headers.insert("Authorization", &token_format);

    // 標準入力を受け取るためのStream
    let (tx_sender, tx_receiver) = futures_channel::mpsc::unbounded();
    // `sink`のdrop時に`close`してしまわないようにするための終了通知チャンネル
    let (sig_sender, sig_receiver) = futures_channel::oneshot::channel::<bool>();

    tokio::spawn(read_file(tx_sender, input_file, sig_receiver));

    let host = "dev-service.mimi.fd.ai";
    let port: i32 = 443;
    let url = format!("wss://{}:{}", host, port);

    let mut builder = Request::builder().uri(url);
    for (k, v) in headers {
        builder = builder.header(k, v);
    }
    let req = builder.body(())?;

    let mut addrs = format!("{}:{}", host, port).to_socket_addrs()?;
    let addr = addrs.next().context("addr not found")?;
    let con = tokio::net::TcpStream::connect(addr).await?;
    let (ws_stream, resp) = client_async_tls(req, con).await.context("hoge?")?;
    debug!("{:?}", resp);

    let (sink, stream) = ws_stream.split();

    let file_to_ws = tx_receiver.map(Ok).forward(sink);
    let ws_to_stdout = {
        stream
            .for_each(|message| async {
                match message {
                    Ok(m) => {
                        let mut data = m.into_data();
                        data.push('\n' as u8);
                        tokio::io::stdout().write_all(&data).await.unwrap();
                    }
                    Err(e) => {
                        debug!("received close frame");
                        trace!("{}", e.to_string());
                    }
                }
            })
            .then(|_| async {
                // `tungstenite` のSinkがDrop時に`close`しないようになればここは削除する
                sig_sender.send(true).unwrap();
                future::ready(())
            })
    };

    pin_mut!(file_to_ws, ws_to_stdout);
    let (_, _) = future::join(file_to_ws, ws_to_stdout).await;

    Ok(())
}

async fn read_file(
    tx: futures_channel::mpsc::UnboundedSender<Message>,
    input_file: PathBuf,
    sig: futures_channel::oneshot::Receiver<bool>,
) -> anyhow::Result<()> {
    let mut f = File::open(input_file).await.context("file open error")?;

    loop {
        let mut buf = vec![0u8; CHUNK_SIZE];
        let size = f.read(&mut buf).await?;

        match size {
            0 => {
                let brk_msg = "{\"command\": \"recog-break\"}";
                tx.unbounded_send(Message::text(brk_msg))?;
                debug!("send break");
                break;
            }
            n => {
                buf.truncate(n);
                tx.unbounded_send(Message::binary(buf))?;
                debug!("send data: {}", n);
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
    }

    let _ = sig.await?;

    Ok(())
}
