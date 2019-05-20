extern crate rust_mimiio;
use clap::{App, Arg};
use rust_mimiio::MimiIO;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::File;
use std::io::prelude::*;
use std::sync::Arc;

const CHUNK_SIZE: usize = 8192;

fn read_token(filename: &str) -> Result<String, String> {
    let mut s = String::new();
    File::open(filename)
        .map_err(|e| e.to_string())?
        .read_to_string(&mut s)
        .map_err(|e| e.to_string())?;

    Ok(s.trim().to_string())
}

fn run() -> Result<(), String> {
    let matches = App::new("rust-mimiio")
        .version("0.1.0")
        .arg(
            Arg::with_name("token")
                .required(true)
                .short("t")
                .long("token")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("input")
                .required(true)
                .short("i")
                .long("input")
                .takes_value(true),
        )
        .get_matches();

    let input_file = matches.value_of("input").unwrap();

    let token_filename = matches.value_of("token").unwrap();
    let token = read_token(token_filename)?;
    let token_format = format!("Bearer {}", token);

    let mut headers = HashMap::new();
    headers.insert("x-mimi-process", "asr");
    headers.insert("x-mimi-input-language", "ja");
    headers.insert("Content-Type", "audio/x-pcm;bit=16;rate=16000;channels=1");
    headers.insert("Authorization", &token_format);

    let mut f = File::open(input_file).expect("file open error");
    let mut buf = [0u8; CHUNK_SIZE];
    let mut total = 0;

    let mio = MimiIO::open(
        "service.mimi.fd.ai",
        443,
        &headers,
        Arc::new(move |send_buf: &mut Vec<u8>, recog_break: &mut bool| {
            let size = f.read(&mut buf).unwrap();
            match size {
                0 => *recog_break = true,
                _ => {
                    for &d in buf.iter() {
                        send_buf.push(d);
                    }
                }
            }
        }),
        &mut Arc::new(move |recv_str, _finished| {
            println!("{}", recv_str);
        }),
    )?;

    mio.close();
    Ok(())
}

fn main() {
    match run() {
        Err(e) => eprintln!("{}", e),
        _ => {}
    }
}
