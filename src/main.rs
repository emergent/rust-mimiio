//use rust_mimiio::MimiIO;
use anyhow::Context;
use std::collections::HashMap;
use std::env;

use std::path::PathBuf;
use structopt::StructOpt;
use tokio::fs::File;
use tokio::io::AsyncReadExt;

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
    /// access token file
    #[structopt(short, long, parse(from_os_str))]
    token: PathBuf,

    /// input audio file
    #[structopt(short, long, parse(from_os_str))]
    input: PathBuf,

    /// specify mimi ASR's engine (asr, nict-asr, google-asr)
    #[structopt(short = "x", long, default_value = "asr")]
    process: String,

    /// specify mimi ASR's input language (ja, en, ...)
    #[structopt(short, long, default_value = "ja")]
    language: String,

    /// host name or IP address
    #[structopt(short, long)]
    host: String,

    /// port number
    #[structopt(short, long)]
    port: u32,

    /// use TLS
    #[structopt(long)]
    tls: bool,

    /// use RealTime mode
    #[structopt(long)]
    real: bool,

    /// use partial result (available only when using nict-asr)
    #[structopt(long)]
    partial: bool,

    /// use temporary result (available only when using nict-asr)
    #[structopt(long)]
    temp: bool,
}

async fn read_token(filename: PathBuf) -> anyhow::Result<String> {
    tokio::fs::read_to_string(filename)
        .await
        .context("couldn't read file")
        .map(|s| s.trim().to_string())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env::set_var("RUST_LOG", "info,rust_mimiio=trace");
    env_logger::init();

    let opt = Opt::from_args();
    info!("{:#?}", opt);

    let input_file = opt.input;
    let token = read_token(opt.token).await?;
    let token_format = format!("Bearer {}", token);
    let process: &str = &opt.process;
    let lang: &str = &opt.language;

    let url = if opt.tls {
        format!("wss://{}:{}", opt.host, opt.port)
    } else {
        format!("ws://{}:{}", opt.host, opt.port)
    };

    let mut headers: HashMap<&str, &str> = HashMap::new();
    headers.insert("x-mimi-process", process);
    headers.insert("x-mimi-input-language", lang);
    headers.insert("Content-Type", "audio/x-pcm;bit=16;rate=16000;channels=1");
    headers.insert("Authorization", &token_format);

    let asr_options = format!(
        "response_format=v2;progressive={};temporary={}",
        opt.partial, opt.temp
    );
    headers.insert("x-mimi-nict-asr-options", &asr_options);

    // 標準入力を受け取るためのStream
    let (tx_sender, tx_receiver) = futures_channel::mpsc::unbounded();
    // `sink`のdrop時に`close`してしまわないようにするための終了通知チャンネル
    let (sig_sender, sig_receiver) = futures_channel::oneshot::channel::<bool>();

    tokio::spawn(read_file(tx_sender, input_file, opt.real, sig_receiver));

    let mut builder = Request::builder().uri(url);
    for (k, v) in headers {
        builder = builder.header(k, v);
    }
    let req = builder.body(())?;

    let mut addrs = format!("{}:{}", opt.host, opt.port).to_socket_addrs()?;
    let addr = addrs.next().context("addr not found")?;
    let con = tokio::net::TcpStream::connect(addr).await?;
    let (ws_stream, resp) = client_async_tls(req, con).await.context("hoge?")?;
    trace!("{:?}", resp);

    let (sink, stream) = ws_stream.split();

    let file_to_ws = tx_receiver.map(Ok).forward(sink);
    let ws_to_stdout = stream
        .for_each(|message| async {
            match message {
                Ok(m) => {
                    if m.is_text() {
                        info!("text frame: {}", m);
                    } else if m.is_close() {
                        info!("close frame: {:?}", m);
                    } else {
                        info!("the other frame: {:?}", m);
                    }
                }
                Err(e) => {
                    trace!("{}", e.to_string());
                }
            }
        })
        .then(|_| async {
            sig_sender.send(true).unwrap();
            future::ready(())
        });

    pin_mut!(file_to_ws, ws_to_stdout);
    let (_, _) = future::join(file_to_ws, ws_to_stdout).await;

    Ok(())
}

async fn read_file(
    tx: futures_channel::mpsc::UnboundedSender<Message>,
    input_file: PathBuf,
    real: bool,
    sig: futures_channel::oneshot::Receiver<bool>,
) -> anyhow::Result<()> {
    let mut f = File::open(input_file).await.context("file open error")?;

    loop {
        let mut buf = vec![0u8; CHUNK_SIZE];
        let size = f.read(&mut buf).await?;

        if size > 0 && real {
            // RealTime mode
            let ms = size as u64 / 32;
            tokio::time::sleep(Duration::from_millis(ms)).await;
        }

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
                //trace!("send data: {}", n);
            }
        }
    }

    let _ = sig.await?;
    debug!("signal received");

    Ok(())
}
