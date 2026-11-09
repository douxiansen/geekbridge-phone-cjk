// SPDX-License-Identifier: GPL-3.0-or-later
//! An RFCOMM byte stream to a service on a paired phone, through WinRT. Windows resolves the
//! channel itself from the service id, so no SDP code is needed here.
use phone_core::platform::PlatformError;
use std::io::{self, Read, Write};
use windows::Devices::Bluetooth::BluetoothCacheMode;
use windows::Devices::Bluetooth::BluetoothDevice;
use windows::Devices::Bluetooth::Rfcomm::RfcommServiceId;
use windows::Networking::Sockets::StreamSocket;
use windows::Storage::Streams::{DataReader, DataWriter, InputStreamOptions};

/// Message Access Server (texts) and Phonebook Access Server (contacts).
pub const MAP_MAS: u32 = 0x1132;
pub const PBAP_PSE: u32 = 0x112F;

fn ioerr(e: windows::core::Error) -> io::Error {
    io::Error::other(e.to_string())
}

/// `AA:BB:CC:DD:EE:FF` to the 48-bit number WinRT uses.
pub fn parse_address(a: &str) -> Option<u64> {
    let parts: Vec<&str> = a.split(':').collect();
    if parts.len() != 6 {
        return None;
    }
    parts.iter().try_fold(0u64, |acc, p| {
        Some(acc << 8 | u64::from_str_radix(p, 16).ok()?)
    })
}

pub struct RfcommStream {
    /// The remote device as `AA:BB:CC:DD:EE:FF`, when Windows reports it.
    pub address: String,
    // Keeps the connection alive.
    _socket: StreamSocket,
    reader: DataReader,
    writer: DataWriter,
}

// WinRT objects created in the MTA are agile; the stream is only used from one thread at a time.
unsafe impl Send for RfcommStream {}

impl RfcommStream {
    /// Wraps an already connected socket (one the phone opened to a service we advertise).
    pub fn from_socket(socket: StreamSocket) -> Result<Self, PlatformError> {
        let werr = |e: windows::core::Error| PlatformError::Other(e.to_string());
        let reader =
            DataReader::CreateDataReader(&socket.InputStream().map_err(werr)?).map_err(werr)?;
        reader
            .SetInputStreamOptions(InputStreamOptions::Partial)
            .map_err(werr)?;
        let writer =
            DataWriter::CreateDataWriter(&socket.OutputStream().map_err(werr)?).map_err(werr)?;
        let raw = socket
            .Information()
            .and_then(|i| i.RemoteAddress())
            .and_then(|h| h.RawName())
            .map(|n| n.to_string())
            .unwrap_or_default();
        let hex: String = raw.chars().filter(|c| c.is_ascii_hexdigit()).collect();
        let address = if hex.len() == 12 {
            (0..6)
                .map(|i| hex[i * 2..i * 2 + 2].to_uppercase())
                .collect::<Vec<_>>()
                .join(":")
        } else {
            String::new()
        };
        Ok(Self {
            address,
            _socket: socket,
            reader,
            writer,
        })
    }

    pub fn connect(address: &str, service: u32) -> Result<Self, PlatformError> {
        let addr = parse_address(address)
            .ok_or_else(|| PlatformError::Other(format!("bad Bluetooth address {address}")))?;
        let werr = |e: windows::core::Error| PlatformError::Other(e.to_string());
        let dev = BluetoothDevice::FromBluetoothAddressAsync(addr)
            .map_err(werr)?
            .join()
            .map_err(werr)?;
        let found = dev
            .GetRfcommServicesForIdWithCacheModeAsync(
                &RfcommServiceId::FromShortId(service).map_err(werr)?,
                BluetoothCacheMode::Uncached,
            )
            .map_err(werr)?
            .join()
            .map_err(werr)?;
        let services = found.Services().map_err(werr)?;
        let svc = services
            .into_iter()
            .next()
            .ok_or(PlatformError::NoService)?;
        let socket = StreamSocket::new().map_err(werr)?;
        socket
            .ConnectAsync(
                &svc.ConnectionHostName().map_err(werr)?,
                &svc.ConnectionServiceName().map_err(werr)?,
            )
            .map_err(werr)?
            .join()
            .map_err(werr)?;
        Self::from_socket(socket)
    }
}

impl Read for RfcommStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let n = self
            .reader
            .LoadAsync(buf.len() as u32)
            .map_err(ioerr)?
            .join()
            .map_err(ioerr)? as usize;
        let n = n.min(buf.len());
        self.reader.ReadBytes(&mut buf[..n]).map_err(ioerr)?;
        Ok(n)
    }
}

impl Write for RfcommStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.writer.WriteBytes(buf).map_err(ioerr)?;
        self.writer
            .StoreAsync()
            .map_err(ioerr)?
            .join()
            .map_err(ioerr)?;
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        self.writer
            .FlushAsync()
            .map_err(ioerr)?
            .join()
            .map_err(ioerr)?;
        Ok(())
    }
}

/// Read half of a split [`RfcommStream`].
pub struct RfcommReader {
    _socket: StreamSocket,
    reader: DataReader,
}
/// Write half of a split [`RfcommStream`].
pub struct RfcommWriter {
    _socket: StreamSocket,
    writer: DataWriter,
}
unsafe impl Send for RfcommReader {}
unsafe impl Send for RfcommWriter {}

impl RfcommStream {
    /// Splits the stream so one thread can block reading while another writes.
    pub fn split(self) -> (RfcommReader, RfcommWriter) {
        let _ = &self.address;
        (
            RfcommReader {
                _socket: self._socket.clone(),
                reader: self.reader,
            },
            RfcommWriter {
                _socket: self._socket,
                writer: self.writer,
            },
        )
    }
}

impl Read for RfcommReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let n = self
            .reader
            .LoadAsync(buf.len() as u32)
            .map_err(ioerr)?
            .join()
            .map_err(ioerr)? as usize;
        let n = n.min(buf.len());
        self.reader.ReadBytes(&mut buf[..n]).map_err(ioerr)?;
        Ok(n)
    }
}

impl Write for RfcommWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.writer.WriteBytes(buf).map_err(ioerr)?;
        self.writer
            .StoreAsync()
            .map_err(ioerr)?
            .join()
            .map_err(ioerr)?;
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
