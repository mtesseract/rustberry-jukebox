use std::convert::TryFrom;
use std::io::{Error, ErrorKind, SeekFrom};
use std::pin::Pin;

use bytes::Bytes;
use futures::prelude::*;
use reqwest::{self, Response};

pub struct FiniteStream {
    length: usize,
    bytes: Vec<u8>,
    pos: usize,
    stream: Pin<Box<dyn Send + 'static + Stream<Item = Result<Bytes, reqwest::Error>>>>,
    finished: bool,
}

impl FiniteStream {
    pub fn from_response(response: Response) -> Result<Self, Error> {
        let length = if let Some(length) = response.content_length() {
            length
        } else {
            return Err(Error::new(
                ErrorKind::Other,
                "HTTP Response does not contain Content-Length",
            ));
        };
        let length = usize::try_from(length).map_err(|err| Error::new(ErrorKind::Other, err))?;
        let bytes = Vec::with_capacity(length);
        let pos = 0;
        let stream = Box::pin(response.bytes_stream());
        let finished = false;
        Ok(FiniteStream {
            length,
            bytes,
            pos,
            stream,
            finished,
        })
    }

    pub fn new(
        length: usize,
        stream: impl Send + 'static + Stream<Item = Result<Bytes, reqwest::Error>> + 'static,
    ) -> Self {
        let stream = Box::pin(stream);
        Self {
            bytes: Vec::new(),
            pos: 0,
            stream,
            finished: false,
            length,
        }
    }
}
impl std::io::Seek for FiniteStream {
    fn seek(&mut self, pos: SeekFrom) -> Result<u64, std::io::Error> {
        let pos: u64 = match pos {
            SeekFrom::Start(n) => n,
            SeekFrom::End(n) => {
                if n < 0 {
                    self.length as u64 - (n.abs() as u64)
                } else {
                    // truncate
                    self.length as u64
                }
            }
            SeekFrom::Current(n) => {
                if n < 0 {
                    // fixme, underflow?
                    self.pos as u64 - (n.abs() as u64)
                } else {
                    self.pos as u64 + (n as u64)
                }
            }
        };

        // truncate cast!
        let mut pos = pos as usize;
        while pos >= self.bytes.len() {
            //fixme
            match futures::executor::block_on(self.stream.next()) {
                Some(res) => match res {
                    Ok(bytes) => {
                        self.bytes.extend_from_slice(bytes.as_ref());
                    }
                    Err(err) => {
                        return Err(std::io::Error::new(std::io::ErrorKind::Other, err));
                    }
                },
                None => {
                    // truncate pos
                    pos = self.bytes.len();
                    break;
                }
            }
        }

        self.pos = pos;
        Ok(pos as u64)
    }
}

impl std::io::Read for FiniteStream {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, std::io::Error> {
        if self.finished {
            Ok(0)
        } else {
            let n_buf = buf.len();
            // fill buffer more if required.
            if self.pos == self.bytes.len() {
                if let Some(res) = futures::executor::block_on(self.stream.next()) {
                    match res {
                        Ok(bytes) => {
                            self.bytes.extend_from_slice(bytes.as_ref());
                        }
                        Err(err) => {
                            return Err(std::io::Error::new(std::io::ErrorKind::Other, err));
                        }
                    }
                }
            }

            let n_remaining_available = self.bytes.len() - self.pos;
            let n_to_read = std::cmp::min(n_buf, n_remaining_available);

            let slice: &[u8] = self.bytes.as_ref();
            buf[0..n_to_read].copy_from_slice(&slice[self.pos..self.pos + n_to_read]);
            self.pos += n_to_read;
            Ok(n_to_read)
        }
    }
}
