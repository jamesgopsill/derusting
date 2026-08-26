use core::{convert::Infallible, marker::PhantomData};

use alloc::vec::Vec;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};

use crate::lwip::tcp::TcpProtocolControlBlock;

pub struct PicoReader {
    receiver: Channel<CriticalSectionRawMutex, Vec<u8>, 2>,
    buf: Vec<u8>,
}

impl embedded_io::ErrorType for PicoReader {
    type Error = Infallible;
}

impl picoserve::io::Read for PicoReader {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        if self.buf.is_empty() {
            let data = self.receiver.receive().await;
            self.buf = data;
        }
        let n = core::cmp::min(self.buf.len(), buf.len());
        buf[..n].copy_from_slice(&self.buf[..n]);
        self.buf.drain(..n);
        Ok(n)
    }
}

pub struct PicoWriter {
    tcp: TcpProtocolControlBlock,
}

impl embedded_io::ErrorType for PicoWriter {
    type Error = Infallible;
}

impl picoserve::io::Write for PicoWriter {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        // TODO: add error variant
        match unsafe { self.tcp.write(buf) } {
            Ok(_) => Ok(buf.len()),
            Err(_) => Ok(0),
        }
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        let _ = self.tcp.output();
        Ok(())
    }
}

pub struct PicoSocket<T> {
    reader: PicoReader,
    writer: PicoWriter,
    _phantom: PhantomData<T>,
}

impl<T> PicoSocket<T> {
    pub fn new(tcp: TcpProtocolControlBlock) -> Self {
        Self {
            reader: PicoReader {
                receiver: Channel::new(),
                buf: Vec::new(),
            },
            writer: PicoWriter { tcp },
            _phantom: PhantomData,
        }
    }
}

impl<T> picoserve::io::Socket<T> for PicoSocket<T> {
    type Error = Infallible;

    type ReadHalf<'a>
        = &'a mut PicoReader
    where
        Self: 'a;

    type WriteHalf<'a>
        = &'a mut PicoWriter
    where
        Self: 'a;

    fn split(&mut self) -> (Self::ReadHalf<'_>, Self::WriteHalf<'_>) {
        todo!()
    }

    async fn abort<R: picoserve::Timer<T>>(
        self,
        timeouts: &picoserve::Timeouts,
        timer: &mut R,
    ) -> Result<(), picoserve::Error<Self::Error>> {
        Ok(())
    }

    async fn shutdown<R: picoserve::Timer<T>>(
        self,
        timeouts: &picoserve::Timeouts,
        timer: &mut R,
    ) -> Result<(), picoserve::Error<Self::Error>> {
        Ok(())
    }
}

/*
use core::convert::Infallible;

use alloc::vec::Vec;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use picoserve::io::{ErrorType, Read, Socket, Write};

use crate::lwip::tcp::TcpProtocolControlBlock;

pub struct PicoChannel {
    receiver: Channel<CriticalSectionRawMutex, Vec<u8>, 2>,
    buf: Vec<u8>,
    tcp: TcpProtocolControlBlock,
}

impl ErrorType for PicoChannel {
    type Error = core::convert::Infallible;
}

impl Read for PicoChannel {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        if self.buf.is_empty() {
            let data = self.receiver.receive().await;
            self.buf = data;
        }

        let n = core::cmp::min(self.buf.len(), buf.len());
        buf[..n].copy_from_slice(&self.buf[..n]);
        self.buf.drain(..n);

        Ok(n)
    }
}

impl Write for PicoChannel {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        // TODO: add error variant
        match unsafe { self.tcp.write(buf) } {
            Ok(_) => Ok(buf.len()),
            Err(_) => Ok(0),
        }
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        let _ = self.tcp.output();
        Ok(())
    }
}

impl Socket for PicoChannel {
    type Error;

    type ReadHalf<'a>
    where
        Self: 'a;

    type WriteHalf<'a>
    where
        Self: 'a;

    fn split(&mut self) -> (Self::ReadHalf<'_>, Self::WriteHalf<'_>) {
        todo!()
    }

    async fn abort<T: picoserve::Timer<T>>(
        self,
        timeouts: &picoserve::Timeouts,
        timer: &mut T,
    ) -> Result<(), picoserve::Error<Self::Error>> {
        todo!()
    }

    async fn shutdown<T: picoserve::Timer<T>>(
        self,
        timeouts: &picoserve::Timeouts,
        timer: &mut T,
    ) -> Result<(), picoserve::Error<Self::Error>> {
        todo!()
    }
}
*/
