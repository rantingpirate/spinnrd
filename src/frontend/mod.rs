//! # frontend 
//! The `frontend` module is where the code for frontends is stored, to 
//! make it as simple as possible to add frontends.

use super::*;

use std::io::Error as IoError;
use std::fs::File;
use std::io::Write;


// #[cfg(feature = "x11")]
// type XSender = ???;
// #[cfg(not(feature = "x11"))]
// type XSender = DummySender;

type SendResult = Result<(), SendError>;

pub fn frontend_help() -> String {
    format!("{}", file_sender_help())
}

fn file_sender_help() -> String {
    format!("
    For File:
        path: The path to the spinfile. Defaults to {}.\n",
        DEFAULT_SPINFILE
        )
}
        

#[derive(Debug)]
pub enum SendError {
    IoError(IoError),
}

impl Display for SendError {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            &SendError::IoError(ref e)  => {
                write!(fmt, "io error sending rotation: {}", e)
            },
        }
    }
}

impl std::error::Error for SendError {
    fn description(&self) -> &str {
        "error sending rotation"
    }

    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            &SendError::IoError(ref e)  => Some(e),
        }
    }
}


pub trait Frontend {
    fn send(&mut self, Rotation) -> SendResult;
}

//FIXME: why does this need display???
pub enum FrontendKind {
    File(FileSender),
    //X11(???),
}

impl Frontend for FrontendKind {
    fn send(&mut self, orientation: Rotation) -> SendResult {
        match self {
            &mut FrontendKind::File(ref mut s)  => s.send(orientation),
        }
    }
}

impl std::fmt::Display for FrontendKind {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            &FrontendKind::File(ref fs)   => {
                write!(fmt, "FileSender to {}", fs.to_string_lossy())
            },
        }
    }
}

#[allow(dead_code)]
pub struct DummySender();


pub struct FileSender {
    path: PathBuf,
}

impl FileSender {
    pub fn init(path: PathBuf) -> InitResult<FileSender> {
        {
            // check if we can create file
            if let Err(e) = File::create(&path) {
                return Err(FrontendError::FileSender(e, path));
            }
        }
        Ok(FileSender {
            path: path,
        })
    }
    pub fn to_string_lossy(&self) -> std::borrow::Cow<str> {
        self.path.to_string_lossy()
    }
}

impl Frontend for FileSender {
    fn send(&mut self, orientation: Rotation) -> SendResult {
        // Opening the file every write so inotifywait is easier (watch for 
        // CLOSE_WRITE once instead of MODIFY twice (open/truncate and 
        // write)).
        File::create(&self.path)
            .and_then(|mut f| write!(f, "{}", orientation))
            .map_err(|e| SendError::IoError(e))
    }
}

#[derive(Debug)]
/// Represents an error initializing a frontend
pub enum FrontendError {
    #[allow(dead_code)]
    NotCompiled(&'static str),
    NoSuchFrontend(String),
    FileSender(IoError, PathBuf),
    // X11(???),
}

impl Display for FrontendError {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            &FrontendError::NotCompiled(s)  => {
                write!(fmt, "frontend '{}' is not compiled.", s)
            },
            &FrontendError::NoSuchFrontend(ref s)   => {
                write!(fmt, "frontend '{}' does not exist!", s)
            },
            &FrontendError::FileSender(ref e, ref p)    => {
                write!(fmt, "can't use file '{}' ({})", p.to_string_lossy(), e)
            },
        }
    }
}

impl std::error::Error for FrontendError {
    fn description(&self) -> &str {
        "couldn't initialize backend"
    }

    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            &FrontendError::NotCompiled(_)  => None,
            &FrontendError::NoSuchFrontend(_)   => None,
            &FrontendError::FileSender(ref e, _)   => Some(e),
        }
    }
}

type InitResult<T> = Result<T, FrontendError>;

macro_rules! frinit {
    ( $tomatch:ident, $opts:ident: $( $name:expr, $init:ident );+ $(;)* ) => {
        match $tomatch.as_str() {
            $(
                $name => {
                    if ! $opts.contains_key($name) {
                        $opts.insert($name.to_owned(), HashMap::new());
                    }
                    $init($opts.get_mut($name).unwrap())
                }),*,
                _     => Err(FrontendError::NoSuchFrontend($tomatch)),
        }
    }
}
/// Initialize a frontend
pub fn init_frontend() -> Result<FrontendKind, i32> {
    let (frontends, mut opts) = get_frontend_options();
    for frontend in frontends {
        let last_output = frinit!(frontend, opts:
            // "x11", init_x11;
            "file", init_file;
            );
        match last_output {
            Ok(o)   => return Ok(o),
            Err(e)  => warn!("Error initializing backend: {}", e),
        }
    }
    return Err(ERR_NO_FRONTEND);
}

fn init_file(opts: &mut HashMap<String, String>) -> InitResult<FrontendKind> {
    let def = DEFAULT_SPINFILE.to_owned();
    Ok(FrontendKind::File(FileSender::init(
        PathBuf::from(parse_path(opts.get("path").unwrap_or(&def),false))
        )?))
}



