use std::io::{Cursor, Read};

use byteorder::ReadBytesExt;
use thiserror::Error;

use crate::MTU;

#[derive(Error, Debug)]
pub enum MarshallError {
    #[error("Could not deserialize the data.")]
    UnableToDeserialize,

    #[error("Buffer too small")]
    OutOfSpace,
}

#[derive(Debug)]
pub enum Message {
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
    pub fn parse(data: &[u8]) -> Result<Self, MarshallError> {
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
                let mut buffer: Vec<u8> = vec![0; data.len() - 4 - 1];
                reader.read_to_end(&mut buffer).map_err(|_| MarshallError::UnableToDeserialize)?;
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

    pub fn serialize(&self) -> Vec<u8> {
        let mut buf: Vec<u8> = Vec::with_capacity(MTU);
        
        match self {
            Message::Send { filename, parts } => {
                buf.push(0);
                buf.extend(parts.to_be_bytes());
                buf.extend(filename.len().to_be_bytes());
                buf.extend(filename.as_bytes());
            },
            Message::Accept => {
                buf.push(1);
            },
            Message::Part { id, data } => {
                buf.push(2);
                buf.extend(id.to_be_bytes());
                buf.extend(data);
            },
            Message::Sync { ids } => {
                buf.push(3);
                buf.extend(ids.len().to_be_bytes());
                for i in ids {
                    buf.extend(i.to_be_bytes());
                }
            },
            Message::Ack { ids } => {
                buf.push(4);
                buf.extend(ids.len().to_be_bytes());
                for i in ids {
                    buf.extend(i.to_be_bytes());
                }
            },
            Message::Loss { ids } => {
                buf.push(5);
                buf.extend(ids.len().to_be_bytes());
                for i in ids {
                    buf.extend(i.to_be_bytes());
                }
            },
        }

        buf
    }
}

#[derive(Debug, Error)]
pub enum BufferError {
    #[error("unknown")]
    Unknown,

    #[error("Too much data for the packet size")]
    DoesNotFit,

    #[error("Reached EOF")]
    EOF,
}

