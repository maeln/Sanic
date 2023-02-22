use bytes::buf;
use clap::{Parser, Subcommand};
use std::fs::File;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::{net::UdpSocket, path::PathBuf};
use thiserror::Error;

const MTU: usize = 1500;
const BUF_CAPACITY: usize = 8 * 1024 * 1024; // 8Mib

#[derive(Parser)]
#[command(name = "Sanic")]
#[command(author = "Maël Naccache Tüfekçi <contact@maeln.com>")]
#[command(version = "1.0")]
#[command(about = "Gotta go fast", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Send { ip: String, file: PathBuf },
    Receive {},
}

fn main() {
    let cli = Cli::parse();
    match &cli.command {
        Commands::Send { ip, file } => {
            send(ip.clone(), 6666, file);
        }
        Commands::Receive {} => {
            receive();
        }
    }
}

#[derive(Error, Debug)]
enum MarshallError {
    #[error("Could not deserialize the data.")]
    UnableToDeserialize,

    #[error("Buffer too small")]
    OutOfSpace,
}

enum Message {
    Send { filename: String, parts: u32 },
    Accept,
    Part { id: u32, data: Vec<u8> },
    Sync { ids: Vec<u32> },
    Ack { ids: Vec<u32> },
    Loss { ids: Vec<u32> },
}

#[derive(Debug, Error)]
enum BufferError {
    #[error("unknown")]
    Unknown,

    #[error("Reached EOF")]
    EOF,
}

fn read_to_end(file: &mut File, channel: Sender<Vec<u8>>) {
    loop {
        let mut buf: Vec<u8> = Vec::with_capacity(BUF_CAPACITY);
        // NOTE: We might not fill the buffer fully
        // We should keep track of the actual length of the buffer and shrink the buffer initial capacity if we always read less
        match file.read(&mut buf) { 
            Ok(read) => {
                if read == 0 {
                    println!("Reached EOF.");
                    break;
                }
                channel.send(buf).expect("Failed to send buffer to reader.");
            }
            Err(err) => {
                println!("Error when reading file: {err}");
                panic!();
            },
        }
    }
}

// fn handle_client()

fn send(ip: String, port: usize, file: &PathBuf) -> () {
    let disp_path = file.to_string_lossy();
    println!("Sending {disp_path} to {ip}:{port}");
    let socket = UdpSocket::bind("0.0.0.0:6667").expect("Could not bind socket");
    socket
        .connect(format!("{ip}:{port}"))
        .expect("Could not connect to receiver");

    let mut write_buf: Vec<u8> = Vec::with_capacity(MTU);
    write!(&mut write_buf, "Hello!").expect("Could not write message");

    match socket.send(&write_buf) {
        Ok(sent) => println!("Sent {sent} octets"),
        Err(err) => {
            println!("Error {err}");
            return;
        }
    }

    println!("Finished");
}

fn receive() -> () {
    let socket = UdpSocket::bind("0.0.0.0:6666").expect("Could not bind socket");
    println!("Listening at port {}", socket.local_addr().unwrap().port());
    let mut read_buf: Vec<u8> = vec![0; MTU];
    loop {
        match socket.recv(&mut read_buf) {
            Ok(size) => println!("Received {size} octets."),
            Err(err) => {
                println!("Error {err}.");
                return;
            }
        }
    }

    println!("Finished");
}
