use std::{
    fs::File,
    io::{Seek, SeekFrom, Write},
    net::UdpSocket,
    sync::{
        mpsc::{Receiver, Sender},
        Arc, Mutex,
    },
};

use tracing::{error, info, warn};

use crate::{protocol::Message, BUF_CAPACITY, MTU, PART_SIZE};

#[derive(Debug)]
struct Sync {
    ack: Vec<u32>,
    loss: Vec<u32>,
}

impl Sync {
    fn new(ack: Vec<u32>, loss: Vec<u32>) -> Self {
        Sync { ack, loss }
    }
}

fn handle_file_write(file: File, nb_parts: u32, file_chan: Receiver<(u32, Vec<u8>)>) {
    let mut file = file;
    let mut part_buffer: Vec<(u32, Vec<u8>)> = Vec::with_capacity(100);
    let mut parts_received = 0;
    loop {
        match file_chan.recv() {
            Ok(part) => {
                part_buffer.push(part);
                parts_received += 1;
                // Our buffer is full or if we received all the parts, we write to the file
                if part_buffer.len() == part_buffer.capacity() || parts_received == nb_parts {
                    part_buffer.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
                    let mut cursor: u64 = (part_buffer[0].0 as usize * PART_SIZE) as u64;
                    let mut slab: Vec<u8> = Vec::with_capacity(BUF_CAPACITY);
                    for i in 0..(part_buffer.len() - 1) {
                        // if the slab is full, we write and empty it
                        if slab.len() == slab.capacity() {
                            file.seek(SeekFrom::Start(cursor))
                                .expect("Could not seek in file");
                            file.write_all(&slab).expect("Could not push slab to file");
                            cursor += slab.len() as u64;
                            slab.clear();
                        }

                        // If the part are continuous, we push to the slab
                        if part_buffer[i].0 + 1 == part_buffer[i + 1].0 {
                            slab.extend(&part_buffer[i].1);
                        } else {
                            // If the part are not continuous, we commit the current slab and start a new one
                            file.seek(SeekFrom::Start(cursor))
                                .expect("Could not seek in file");
                            file.write_all(&slab).expect("Could not push slab to file");
                            cursor += slab.len() as u64;
                            slab.clear();
                            slab.extend(&part_buffer[i].1);
                        }
                    }
                }

                if parts_received == nb_parts {
                    info!("Finished writing the file.");
                    break;
                }
            }
            Err(err) => {
                error!(error = ?err, "Could not receive part.");
                panic!()
            }
        }
    }
}

fn handle_client_sync(socket: UdpSocket, sync_chan: Receiver<Sync>) {
    loop {
        match sync_chan.recv() {
            Ok(sync) => {
                if sync.ack.len() > 0 {
                    socket
                        .send(&Message::Ack { ids: sync.ack }.serialize())
                        .expect("Could not send ACK");
                }
                if sync.loss.len() > 0 {
                    socket
                        .send(&Message::Loss { ids: sync.loss }.serialize())
                        .expect("Could not send LOSS");
                }
            }
            Err(err) => {
                error!(error = ?err, "Failed to received from sync channel.");
            }
        }
    }
}

fn handle_client_read(
    socket: UdpSocket,
    nb_parts: u32,
    parts_received: Arc<Mutex<Vec<u32>>>,
    file_chan: Sender<(u32, Vec<u8>)>,
    sync_chan: Sender<Sync>,
) {
    let mut buf: Vec<u8> = vec![0; MTU];
    loop {
        match socket.recv(&mut buf) {
            Ok(size) => {
                let data = &buf[..size];
                match Message::parse(data) {
                    Ok(Message::Part { id, data }) => {
                        file_chan
                            .send((id, data))
                            .expect("Could not send chunk to writer.");
                        {
                            let mut _guard =
                                parts_received.lock().expect("Could not lock ack array");
                            _guard.push(id);
                            _guard.sort();
                            if _guard.len() as u32 >= nb_parts {
                                info!("Transfer finished!");
                                break;
                            }
                        }
                    }
                    Ok(Message::Sync { ids }) => {
                        let mut ack: Vec<u32> = Vec::new();
                        let mut loss: Vec<u32> = Vec::new();
                        // We check if we received those part and if not we consider them lost.
                        // Note: Due to the lack of ordering garanties in UDP, we might get an id in SYNC that we are going to receive right after
                        for id in ids {
                            if parts_received
                                .lock()
                                .expect("Could not lock ack array")
                                .binary_search(&id)
                                .is_err()
                            {
                                warn!("Detected packet loss.");
                                loss.push(id);
                            } else {
                                ack.push(id);
                            }
                        }
                        sync_chan
                            .send(Sync::new(ack, loss))
                            .expect("Could not trigger sync.");
                    }
                    Ok(msg) => {
                        warn!(message = ?msg, "Received unexpected message.");
                    }
                    Err(err) => {
                        warn!(error = ?err, "Could not parse packet.");
                    }
                }
            }
            Err(err) => {
                error!(error=?err, "Error when receiving from socket.");
                panic!();
            }
        }
    }
}
