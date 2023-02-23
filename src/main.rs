use byteorder::ReadBytesExt;
use clap::{Parser, Subcommand};
use tracing::{error, info, warn};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write, Cursor};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Sender, Receiver};
use std::sync::{Arc, Mutex};
use std::{net::UdpSocket, path::PathBuf};
use thiserror::Error;

mod protocol;
mod server;
mod client;

const MTU: usize = 1500;
const BUF_CAPACITY: usize = 8 * 1024 * 1024; // 8Mib
const PART_SIZE: usize = MTU - 1 - 4;

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

fn send(ip: String, port: usize, file: &Path) -> () {
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
