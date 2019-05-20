use futures::future::Future;
use futures::sink::Sink;
use futures::stream::Stream;
use futures::sync::mpsc;
use std::collections::HashMap;
use std::thread;
use std::time::Duration;

use websocket::header::Headers;
use websocket::result::WebSocketError;
use websocket::{ClientBuilder, OwnedMessage};

use std::thread::JoinHandle;

pub type TxCallback = Box<FnMut(&mut Vec<u8>, &mut bool) + Send + Sync + 'static>;
pub type RxCallback = Box<FnMut(&str, &bool) + Send + Sync + 'static>;

pub struct MimiIO {
    tx_thread: JoinHandle<u32>,
    rx_thread: JoinHandle<u32>,
}

impl MimiIO {
    pub fn open(
        host: &str,
        port: i32,
        request_headers: &HashMap<&str, &str>,
        tx_func: &'static mut TxCallback,
        rx_func: &'static mut RxCallback,
    ) -> Result<Self, String> {
        let headers = Self::make_headers(request_headers);
        let url = format!("wss://{}:{}", host, port);

        let (tx_sender, tx_receiver) = mpsc::channel(0);

        let tx_thread = thread::spawn(move || {
            let mut tx_sink = tx_sender.wait();
            let mut total = 0;
            let mut recog_break = false;

            thread::sleep(Duration::from_millis(100));
            'txloop: loop {
                let mut buffer = Vec::new();
                tx_func(&mut buffer, &mut recog_break);

                match recog_break {
                    false => {
                        let size = buffer.len();
                        tx_sink.send(OwnedMessage::Binary(buffer)).unwrap();
                        eprintln!("send data: {}", size);
                        total += size as u32;
                    }
                    true => {
                        let brk_msg = "{\"command\": \"recog-break\"}".to_owned();
                        tx_sink.send(OwnedMessage::Text(brk_msg)).unwrap();
                        eprintln!("send break");
                        break 'txloop;
                    }
                }
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
                        .filter_map(|message| match message {
                            OwnedMessage::Close(e) => Some(OwnedMessage::Close(e)),
                            OwnedMessage::Text(t) => {
                                let finished = false;
                                rx_func(&t, &finished);
                                None
                            }
                            _ => None,
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
        self.tx_thread.join().unwrap();
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
