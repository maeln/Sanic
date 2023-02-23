use std::{fs::File, sync::{mpsc::{Sender, Receiver}, Arc, Mutex}, io::Read, net::UdpSocket, collections::HashMap, time::Duration};
use tracing::{info, error, warn};

use crate::{protocol::{BufferError, Message}, MTU, BUF_CAPACITY, PART_SIZE};

fn read_to_end(file: File, channel: Sender<Vec<u8>>) {
    let mut file = file;
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
    if data.len()>PART_SIZE {
        return Err(BufferError::DoesNotFit);
    }

    packet[0]=2;

    let pid = part_id.to_be_bytes();
    packet[1] = pid[0];
    packet[2] = pid[1];
    packet[3] = pid[2];
    packet[4] = pid[3];

    for i in 0..data.len() {
        packet[5+i] = data[i];
    }
    
    Ok(())
}

fn handle_ack_and_loss(socket: UdpSocket, parts_waiting_ack: Arc<Mutex<Vec<u32>>>, packet_in_flight: Arc<Mutex<HashMap<u32, Vec<u8>>>>) {
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
                    Ok(msg) => {
                        warn!(message = ?msg, "Received unexpected message.");                    }
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

fn handle_send(socket: UdpSocket, channel: Receiver<Vec<u8>>, parts_waiting_ack: Arc<Mutex<Vec<u32>>>, packet_in_flight: Arc<Mutex<HashMap<u32, Vec<u8>>>>) {
    let mut part_id: u32 = 0;
    // TODO: add CRC16
    loop {
        match channel.recv() {
            Ok(data) => {
                // MTU - 1 (message ID) - 4 (part id)
                for chunk in data.chunks(PART_SIZE) {
                    let mut packet_data: Vec<u8> = Vec::with_capacity(MTU);
                    make_parts_packet(chunk, part_id, &mut packet_data).expect("Chunk too big!");
                    socket.send(&packet_data).expect("Could not send part to client.");
                    parts_waiting_ack.lock().expect("Could not lock ack array.").push(part_id);
                    packet_in_flight.lock().expect("Could not lock parts in flight.").insert(part_id, packet_data);
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

fn handle_sync(socket: UdpSocket, parts_waiting_ack: Arc<Mutex<Vec<u32>>>) {
    loop {
        std::thread::sleep(Duration::from_millis(200));
        let len = parts_waiting_ack.lock().expect("Could not lock ack array").len();
        let mut ids: Vec<u32> = Vec::with_capacity(len);
        {
            let _guard = parts_waiting_ack.lock().expect("Could not lock ack array");
            for i in _guard.iter() {
                ids.push(i.clone());
            }
        }
        let sync_msg = Message::Sync { ids: ids };
        socket.send(&sync_msg.serialize()).expect("Could not sync message.");
    }
}