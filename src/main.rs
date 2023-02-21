
use std::{path::PathBuf, net::UdpSocket};
use std::io::Write;
use bytes::BufMut;
use thiserror::Error;
use clap::{Parser, Subcommand};

const MTU: u16 = 1500;

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
    Send { 
        ip: String,
        file: PathBuf,
    },
    Receive {}
}

fn main() {
    let cli = Cli::parse();
    match &cli.command {
        Commands::Send { ip, file } => {
            send(ip.clone(), 6666, file);
        },
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

trait Serialize {
    fn serialize(&self, buffer: &mut dyn BufMut) -> Result<(), MarshallError>;
}

trait Deserialize {
    fn deserialize(data: &[u8]) -> Result<Self, MarshallError> where Self: Sized;
}

enum Message {
    Send {
        filename: String,
        parts: u32,
    },
    Accept,
    Part {
        id: u32,
        data: Vec<u8>
    },
    Sync {
        ids: Vec<u32>
    },
    Ack {
        ids: Vec<u32>
    },
    Loss {
        ids: Vec<u32>
    }
}

impl Serialize for Message {
    fn serialize(&self, buf: &mut dyn BufMut) -> Result<(), MarshallError> {
        match self {
            Message::Send { filename, parts } => {
                buf.put_u8(0); // Message ID
                buf.put_u32(*parts); // Number of parts
                buf.put_u32(filename.len() as u32); // Filename length (in octets, not number of char)
                for b in filename.as_bytes() { // filename string.
                    buf.put_u8(*b);
                }
                return Ok(());
            },
            Message::Accept => {
                buf.put_u8(1);
                return Ok(());
            }
            Message::Part { id, data } => {
                let mut buf = Vec::with_capacity(1 + 4 + data.len());
                buf.push(2);
                buf.extend(id.to_be_bytes());
                buf.extend(data);
                return buf;
            }
            Message::Sync { ids } => {
                let mut buf = Vec::with_capacity(ids.len() * 4);

            }
            _ => todo!()
        }
    }
}

fn send(ip: String, port: usize, file: &PathBuf) -> () {
    let disp_path = file.to_string_lossy();
    println!("Sending {disp_path} to {ip}:{port}");
    let socket = UdpSocket::bind("0.0.0.0:6667").expect("Could not bind socket");
    socket.connect(format!("{ip}:{port}")).expect("Could not connect to receiver");

    let mut write_buf: Vec<u8> = Vec::with_capacity(MTU as usize);
    write!(&mut write_buf, "Hello!").expect("Could not write message");
    
    match socket.send(&write_buf) {
        Ok(sent) => println!("Sent {sent} octets"),
        Err(err) => {
            println!("Error {err}");
            return;
        },
    }

    println!("Finished");
}

fn receive() -> () {
    let socket = UdpSocket::bind("0.0.0.0:6666").expect("Could not bind socket");
    println!("Listening at port {}", socket.local_addr().unwrap().port());
    let mut read_buf: Vec<u8> = vec![0; MTU as usize];
    loop {
        match socket.recv(&mut read_buf) {
            Ok(size) => println!("Received {size} octets."),
            Err(err) => {
                println!("Error {err}.");
                return;
            },
        }
    }

    println!("Finished");
}