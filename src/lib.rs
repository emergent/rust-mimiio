/*
//use futures::channel::mpsc;
//use futures::future::{Future, FutureExt};
//use futures::sink::Sink;
//use futures::stream::Stream;
use futures_util::stream::StreamExt;
use std::collections::HashMap;
use tokio::time::Duration;

use anyhow::Context;
//use futures::StreamExt;
use http::Request;
use std::net::ToSocketAddrs;
//use tokio::io::{ErrorKind, Sink, Split};
//use tokio::stream::StreamExt;
//use tokio::stream::StreamExt;
//use futures_util::TryStreamExt;
use tokio::task::JoinHandle;
use tokio_tungstenite::client_async_tls;
use tokio_tungstenite::tungstenite::Message;

pub struct MimiIO {
    tx_thread: JoinHandle<u32>,
    rx_thread: JoinHandle<u32>,
}

impl MimiIO {
    pub async fn open<TXCB, RXCB>(
        host: &str,
        port: i32,
        request_headers: &HashMap<&str, &str>,
        tx_func0: TXCB,
        rx_func0: RXCB,
    ) -> anyhow::Result<Self>
    where
        TXCB: FnMut(&mut Vec<u8>, &mut bool) + Send + Sync + 'static,
        RXCB: FnMut(&str, bool) + Send + Sync + 'static,
    {
        //let headers = Self::make_headers(request_headers);
        let url = format!("wss://{}:{}", host, port);

        let mut tx_func = Box::new(tx_func0);
        let mut rx_func = Box::new(rx_func0);

        let mut builder = Request::builder().uri(url);
        for (&k, &v) in request_headers {
            builder = builder.header(k, v);
        }
        let req = builder.body(())?;

        let mut addrs = format!("{}:{}", host, port).to_socket_addrs().unwrap();
        let addr = addrs.next().unwrap();
        let con = tokio::net::TcpStream::connect(addr).await?;
        let (ws_stream, a) = client_async_tls(req, con).await.context("hoge?")?;
        let (sink, stream) = ws_stream.split();
        let (tx_sender, tx_receiver) = futures_channel::mpsc::unbounded();

        println!("{:?}", a);

        let tx_thread = tokio::spawn(async move {
            let mut recog_break = false;
            let mut total = 0;

            'txloop: loop {
                let mut buffer = Vec::new();
                tx_func(&mut buffer, &mut recog_break);

                match recog_break {
                    true => {
                        let brk_msg = "{\"command\": \"recog-break\"}";
                        tx_sender
                            .unbounded_send(Message::Text(brk_msg.into()))
                            .await
                            .unwrap();
                        eprintln!("send break");
                        break 'txloop;
                    }
                    false => {
                        let size = buffer.len();
                        tx_sender
                            .unbounded_send(Message::Binary(buffer))
                            .await
                            .unwrap();
                        eprintln!("send data: {}", size);
                        total += size as u32;
                    }
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            total
        });

        let rx_thread = tokio::spawn(async move {
            eprintln!("recv wait start");

            stream
                .filter_map(|message| futures_util::future::ready(None))
                // .filter_map(|message| match message.unwrap() {
                //     Message::Close(_) => futures_util::future::ok(()),
                //     Message::Text(t) => {
                //         let finished = false;
                //         rx_func(&t, finished);
                //         futures_util::future::ok(())
                //     }
                //     _ => futures_util::future::ok(()),
                // })
                .select_next_some(tx_receiver)
                // .select(tx_receiver.map_err(|_| {
                //     tokio_tungstenite::tungstenite::error::Error::Io(std::io::Error::from(
                //         std::io::ErrorKind::BrokenPipe,
                //     ))
                // }))
                .forward(sink)
                .await
                .unwrap();
            0
        });

        Ok(MimiIO {
            tx_thread,
            rx_thread,
        })
    }

    pub async fn close(self) {
        self.tx_thread.await.unwrap();
        self.rx_thread.await.unwrap();
    }

    // fn make_headers(headers: &HashMap<&str, &str>) -> Headers {
    //     let mut ws_headers = Headers::new();
    //
    //     for (&key, &val) in headers.iter() {
    //         ws_headers.set_raw(key.to_owned(), vec![val.as_bytes().to_vec()]);
    //     }
    //     ws_headers
    // }
}
*/
