extern crate bincode;
extern crate csv;
extern crate telamon;
extern crate telamon_utils as utils;

use std::io;
use std::io::Seek;
use telamon::explorer::TreeEvent;
use utils::tfrecord::{ReadError, RecordReader};

fn main() -> Result<(), ReadError> {
    let mut f = std::fs::File::open("eventlog.tfrecord")?;
    let mut offset;

    let mut writer = csv::Writer::from_writer(io::stdout());
    loop {
        offset = f.seek(io::SeekFrom::Current(0))?;

        match f.read_record() {
            Ok(record) => match bincode::deserialize(&record).unwrap() {
                TreeEvent::Evaluation {
                    actions,
                    score: _score,
                } => {
                    writer
                        .write_record(&[actions.to_vec().len().to_string()])
                        .unwrap();
                }
            },
            Err(err) => {
                // If we reached eof and no bytes were read, we were
                // at the end of a well-formed file and we can safely
                // exit. Otherwise, we propagate the error.
                if let ReadError::IOError(ref error) = err {
                    if error.kind() == io::ErrorKind::UnexpectedEof
                        && offset == f.seek(io::SeekFrom::Current(0))?
                    {
                        writer.flush().unwrap();
                        return Ok(());
                    }
                }
                return Err(err);
            }
        }
    }
}
