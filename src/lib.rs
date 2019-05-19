use futures::future::Future;
use futures::sink::Sink;
use futures::stream::Stream;
use futures::sync::mpsc;
use std::collections::HashMap;
use std::fs::File;
use std::io::prelude::*;
use std::thread;
use std::thread::JoinHandle;
use std::time::Duration;
use websocket::header::Headers;
use websocket::result::WebSocketError;
use websocket::{ClientBuilder, OwnedMessage};

pub struct MimiIO {
    tx_thread: JoinHandle<u32>,
    rx_thread: JoinHandle<u32>,
}

const CHUNK_SIZE: usize = 16384;

impl MimiIO {
    pub fn open(
        host: &str,
        port: i32,
        request_headers: &HashMap<&str, &str>,
    ) -> Result<Self, String> {
        let headers = Self::make_headers(request_headers);
        let url = format!("wss://{}:{}", host, port);

        let (tx_sender, tx_receiver) = mpsc::channel(0);

        let tx_thread = thread::spawn(move || {
            let mut tx_sink = tx_sender.wait();

            let filename = "./audio.raw";
            let mut f = File::open(filename).expect("file open error");
            let mut buf = [0u8; CHUNK_SIZE];
            let mut total = 0;

            'txloop: loop {
                let size = f.read(&mut buf).unwrap();

                match size {
                    0 => {
                        let recog_break =
                            OwnedMessage::Text("{\"command\": \"recog-break\"}".to_owned());
                        tx_sink.send(recog_break).unwrap();
                        eprintln!("send break");
                        break 'txloop;
                    }
                    _ => {
                        let bufv = buf.iter().take(size).map(|&b| b).collect::<Vec<u8>>();
                        let data = OwnedMessage::Binary(bufv);

                        tx_sink.send(data).unwrap();
                        eprintln!("send data: {}", size);
                    }
                }

                total += size as u32;

                thread::sleep(Duration::from_millis(100));
            }
            total
        });

        let rx_thread = thread::spawn(move || {
            let mut runtime = tokio::runtime::current_thread::Builder::new()
                .build()
                .unwrap();

            let runner = ClientBuilder::new(&url)
                .unwrap()
                .custom_headers(&headers)
                .async_connect_secure(None)
                .and_then(|(duplex, _)| {
                    let (sink, stream) = duplex.split();
                    eprintln!("recv wait start");
                    stream
                        .filter_map(|message| {
                            //eprintln!("{:?}", message);

                            match message {
                                OwnedMessage::Close(e) => Some(OwnedMessage::Close(e)),
                                OwnedMessage::Text(t) => {
                                    println!("{}", t);
                                    None
                                }
                                _ => None,
                            }
                        })
                        .select(tx_receiver.map_err(|_| WebSocketError::NoDataAvailable))
                        .forward(sink)
                });
            runtime.block_on(runner).unwrap();
            0
        });

        Ok(MimiIO {
            tx_thread,
            rx_thread,
        })
    }

    pub fn close(self) {
        let total = self.tx_thread.join().unwrap();
        eprintln!("total sent bytes: {}", total);
        self.rx_thread.join().unwrap();
    }

    fn make_headers(headers: &HashMap<&str, &str>) -> Headers {
        let mut ws_headers = Headers::new();

        for (&key, &val) in headers.iter() {
            ws_headers.set_raw(key.to_owned(), vec![val.as_bytes().to_vec()]);
        }
        ws_headers
    }
}
