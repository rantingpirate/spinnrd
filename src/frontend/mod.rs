//! # frontend 
//! The `frontend` module is where the code for frontends is stored, to 
//! make it as simple as possible to add frontends.

use super::*;

use std::io::Error as IoError;
use std::fs::File;


// #[cfg(feature = "x11")]
// type XSender = ???;
#[cfg(not(feature = "x11"))]
type XSender = DummySender;

type SendResult = Result<(), SendError>;

enum SendError {
    IoError(IoError),
}

pub trait Frontend {
    fn send(Rotation) -> SendResult;
}

pub enum FrontendKind {
    File(FileSender),
    //X11(???),
}

pub struct DummySender();


pub struct FileSender {
    path: PathBuf,
}

impl FileSender {
    pub fn init(path: PathBuf) -> SenderInitResult<FileSender> {
        {
            // check if we can create file
            if let Err(e) = File.create(&path) {
                return SenderInitError::FileError(e);
            }
        }
        FileSender {
            path: path,
        }
    }
    pub fn to_string_lossy(&self) -> Cow {
        self.path.to_string_lossy()
    }
}

impl Frontend for FileSender {
    fn send(&mut self, orientation = Rotation) -> SendResult {
        // Opening the file every write so inotifywait is easier (watch for 
        // CLOSE_WRITE once instead of MODIFY twice (open/truncate and 
        // write)).
        match File::create(&self.path) {
            Ok(mut f)   => write!(f, "{}", orientation.unwrap())
                .map_err(|e| SendError::IoError(e)),
            Err(e)  => SendError::IoError(e),
        } // match File::create(spinfile)
    }
}



