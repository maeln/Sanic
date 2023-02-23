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
    // ID: 0
    Send { filename: String, parts: u32 },
    // ID: 1
    Accept,
    // ID: 2
    Part { id: u32, data: Vec<u8> },
    // ID: 3
    Sync { ids: Vec<u32> },
    // ID: 4
    Ack { ids: Vec<u32> },
    // ID: 5
    Loss { ids: Vec<u32> },
}

impl Message {
    fn parse(data: &[u8]) -> Result<Self, MarshallError> {
        match data[0] {
            0 => {
                let mut reader = Cursor::new(data);
                let parts = reader.read_u32::<byteorder::BigEndian>().map_err(|_| MarshallError::UnableToDeserialize)?;
                let string_size = reader.read_u32::<byteorder::BigEndian>().map_err(|_| MarshallError::UnableToDeserialize)?;
                let mut string_bytes: Vec<u8> =  vec![0; string_size as usize];
                reader.read_exact(&mut string_bytes).map_err(|_| MarshallError::UnableToDeserialize)?;
                let filename = String::from_utf8_lossy(&string_bytes).to_string();
                Ok(Message::Send { filename, parts })
            }
            1 => Ok(Message::Accept),
            2 => {
                let mut reader = Cursor::new(data);
                let id = reader.read_u32::<byteorder::BigEndian>().map_err(|_| MarshallError::UnableToDeserialize)?;
                let mut buffer: Vec<u8> = vec![0; data.len() - 4];
                reader.read_exact(&mut buffer).map_err(|_| MarshallError::UnableToDeserialize)?;
                Ok(Message::Part { id, data: buffer })
            },
            3 => {
                let mut reader = Cursor::new(data);
                let size = reader.read_u32::<byteorder::BigEndian>().map_err(|_| MarshallError::UnableToDeserialize)?;
                let mut ids: Vec<u32> =  Vec::with_capacity(size as usize);
                for _ in 0..size {
                    let id = reader.read_u32::<byteorder::BigEndian>().map_err(|_| MarshallError::UnableToDeserialize)?;
                    ids.push(id);
                }
                Ok(Message::Sync { ids })
            }
            4 => {
                let mut reader = Cursor::new(data);
                let size = reader.read_u32::<byteorder::BigEndian>().map_err(|_| MarshallError::UnableToDeserialize)?;
                let mut ids: Vec<u32> =  Vec::with_capacity(size as usize);
                for _ in 0..size {
                    let id = reader.read_u32::<byteorder::BigEndian>().map_err(|_| MarshallError::UnableToDeserialize)?;
                    ids.push(id);
                }
                Ok(Message::Ack { ids })
            }
            5 => {
                let mut reader = Cursor::new(data);
                let size = reader.read_u32::<byteorder::BigEndian>().map_err(|_| MarshallError::UnableToDeserialize)?;
                let mut ids: Vec<u32> = Vec::with_capacity(size as usize);
                for _ in 0..size {
                    let id = reader.read_u32::<byteorder::BigEndian>().map_err(|_| MarshallError::UnableToDeserialize)?;
                    ids.push(id);
                }
                Ok(Message::Loss { ids })
            }
            _ => Err(MarshallError::UnableToDeserialize)
        }
    }
}

#[derive(Debug, Error)]
enum BufferError {
    #[error("unknown")]
    Unknown,

    #[error("Too much data for the packet size")]
    DoesNotFit,

    #[error("Reached EOF")]
    EOF,
}

fn read_to_end(file: &mut File, channel: Sender<Vec<u8>>) {
    loop {
        let mut buf: Vec<u8> = vec![0; BUF_CAPACITY];
        // NOTE: We might not fill the buffer fully
        // We should keep track of the actual length of the buffer and shrink the buffer initial capacity if we always read less
        match file.read(&mut buf) { 
            Ok(read) => {
                if read == 0 {
                    info!("Reached EOF.");
                    break;
                }
                channel.send(buf).expect("Failed to send buffer to reader.");
            }
            Err(err) => {
                error!(error = ?err, "Error when reading file.");
                panic!();
            },
        }
    }
}

fn make_parts_packet(data: &[u8], part_id: u32, packet: &mut [u8]) -> Result<(), BufferError> {
    if data.len()>(packet.len()-4-4-1) {
        return Err(BufferError::DoesNotFit);
    }

    packet[0]=2;

    let pid = part_id.to_be_bytes();
    packet[1] = pid[0];
    packet[2] = pid[1];
    packet[3] = pid[2];
    packet[4] = pid[3];

    let len = (data.len() as u32).to_be_bytes();
    packet[5] = len[0];
    packet[6] = len[1];
    packet[7] = len[2];
    packet[8] = len[3];

    for i in 0..data.len() {
        packet[9+i] = data[i];
    }
    
    Ok(())
}

fn handle_server_read(socket: UdpSocket, parts_waiting_ack: Arc<Mutex<Vec<u32>>>, packet_in_flight: Arc<Mutex<HashMap<u32, Vec<u8>>>>) {
    let mut buf: Vec<u8> = vec![0; MTU];
    loop {
        match socket.recv(&mut buf) {
            Ok(size) => {
                let data = &buf[..size];
                match Message::parse(data) {
                    Ok(Message::Ack { ids }) => {
                        for id in ids {
                            let mut acks_guard = parts_waiting_ack.lock().expect("Could not lock on acks array");
                            if let Ok(id_pos) = acks_guard.binary_search(&id) {
                                acks_guard.remove(id_pos);
                            } else {
                                warn!(id, "received a ack with an id we don't have in our array.");
                            }
                        }
                    }
                    Ok(Message::Loss { ids }) => {
                        for id in ids {
                            if let Some(packet) = packet_in_flight.lock().expect("Could not lock packet in flight").get(&id) {
                                socket.send(packet).expect("Failed to send packet to client");
                            } else {
                                warn!(id, "Client lost a packet that is not in flight.");
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(err) => {
                        warn!(error = ?err, "Could not parse packet.");
                    }
                }
            },
            Err(err) => {
                error!(error = ?err, "Could not receive from udp socket.");
                panic!();
            },
        }
    }
}

fn handle_server_send(socket: UdpSocket, channel: Receiver<Vec<u8>>, parts_waiting_ack: Arc<Mutex<Vec<u32>>>, packet_in_flight: Arc<Mutex<HashMap<u32, Vec<u8>>>>) {
    let mut part_id: u32 = 0;
    // TODO: add CRC16
    loop {
        match channel.recv() {
            Ok(data) => {
                // MTU - 1 (message ID) - 4 (part id) - 4 (array size)
                for chunk in data.chunks(MTU - 1 - 4 - 4) {
                    let mut packet_data: Vec<u8> = Vec::with_capacity(MTU);
                    make_parts_packet(chunk, part_id, &mut packet_data).expect("Chunk too big!");
                    socket.send(&packet_data).expect("Could not send part to client.");
                    {
                        parts_waiting_ack.lock().expect("Could not lock ack array.").push(part_id);
                    }
                    {
                        packet_in_flight.lock().expect("Could not lock parts in flight.").insert(part_id, packet_data);
                    }
                    part_id += 1;
                }
            },
            Err(err) => {
                error!(error = ?err, "Error when receiving from channel");
                break;
            },
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
