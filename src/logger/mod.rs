use std::{
    io::{self, Write},
    sync::LazyLock,
};

use crate::REDRAW;
use crate::logger::line_history::LineHistory;
use ansi_to_tui::IntoText;
use steel_utils::locks::SyncMutex;
use tracing_subscriber::fmt::MakeWriter;

mod line_history;

pub(crate) static LOGGER: LazyLock<SyncMutex<LineHistory>> =
    LazyLock::new(|| SyncMutex::new(LineHistory::new()));

/// A writer that forwards all text written into `LOGGER`
#[derive(Debug, Clone, Copy)]
pub struct TuiLoggerWriter;

impl Write for TuiLoggerWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let buf = String::from_utf8_lossy(buf).to_string();

        if buf.is_empty() {
            return Ok(0);
        }

        let text = buf.into_text().expect("failed ansi-to-tui conversion");
        LOGGER.lock().push(text);
        REDRAW.notify_one();

        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for TuiLoggerWriter {
    type Writer = Self;

    fn make_writer(&'a self) -> Self::Writer {
        *self
    }
}
