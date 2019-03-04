//! fsaccel.rs
//!
//! A module for representing an accelerometer based on data from the filesystem.

use super::AccelerationVector as AVector;

use std::path::PathBuf;
use std::fs::File;
use std::io::prelude::*;
use std::io::BufReader;
use std::io::Error as IoError;
use std::io::SeekFrom;
use std::ops::Deref;

use regex::Regex;
use glob::*;

type IoResult<T> = Result<T, IoError>;

const DEFAULT_ENDIANNESS: Endian = Endian::Little;
const DEFAULT_SIGNED: Signed = Signed::Unsigned;
const DEFAULT_REPEAT: u8 = 0u8;
const DEFAULT_RSHIFT: u8 = 1u8;
static SCANTYPE_RE_STR: &'static str = r"(?:(?P<end>be|le):)?(?P<sign>s|u)?(?P<bit>\d+)/(?P<sto>\d+)(?:X(?P<rep>\d+))?(?:>>(?P<shift>\d+))?";

#[derive(Debug)]
pub enum Endian {
    Big,
    Little,
}

impl Endian {
    fn from_str(s: &str) -> Endian {
        match s {
            "be"    => Endian::Big,
            "le"    => Endian::Little,
            _       => DEFAULT_ENDIANNESS,
        }
    }
}


#[derive(Debug)]
pub enum Signed {
    Signed,
    Unsigned,
}

impl Signed {
    fn from_str(s: &str) -> Signed {
        match s {
            "s"    => Signed::Signed,
            "u"    => Signed::Unsigned,
            _       => DEFAULT_SIGNED,
        }
    }
}


#[derive(Debug)]
pub struct ScanType {
    endianness: Endian,
    sign: Signed,
    bits: u8,
    // bytes: u8,
    storagebits: u8,
    // storagebytes: u8,
    repeat: u8,
    rshift: u8,
    conversion: fn(u64,u8) -> i64,
}

impl ScanType {
    /// Create a ScanType from the contents of a file
    pub fn from_file<P: AsRef<Path>>(path: P) -> IoResult<ScanType> {
        let mut contents = String::new();
        {
            let mut file = {File::open(path)?};
            file.read_to_string(&mut contents)?;
        }
        ScanType::from_str(contents.as_str())
    }

    /// Create a ScanType from the contents of a string
    pub fn from_str<'s, S: Into<&'s str>>(contents: S) -> IoResult<ScanType> {
        let caps = SCANTYPE_RE.captures(contents.into()).unwrap();
        let bits = caps["bit"].parse::<u8>().unwrap();
        let sbits = caps["sto"].parse::<u8>().unwrap();
        let sign = match caps.name("sign") {
            Some(s)   => Signed::from_str(s.as_str()),
            _           => DEFAULT_SIGNED,
        };

        Ok(ScanType {
            endianness: match caps.name("end") {
            Some(s)   => Endian::from_str(s.as_str()),
                _           => DEFAULT_ENDIANNESS,
            },
            conversion: match sign {
                Signed::Signed => match bits {
                    8   => _conv8,
                    16  => _conv16,
                    32  => _conv32,
                    64  => _conv64,
                    _   => _conv_signed,
                },
                Signed::Unsigned => _conv_unsigned,
            },
            sign: sign,
            bits: bits,
            // bytes: bits / 8 + if 0 == bitz % 8 {0} else {1},
            storagebits: sbits,
            // storagebytes: sbits / 8 + if 0 == sbitz % 8 {0} else {1},
            repeat: match caps.name("rep") {
                Some(s) => match s.as_str().parse::<u8>() {
                    Ok(n)   => n,
                    Err(_)  => DEFAULT_REPEAT,
                },
                None    => DEFAULT_REPEAT,
            },
            rshift: match caps.name("shift") {
                Some(s) => match s.as_str().parse::<u8>() {
                    Ok(n)   => n,
                    Err(_)  => DEFAULT_RSHIFT,
                },
                None    => DEFAULT_RSHIFT,
            },
            // diffbits: (bits / 8) * 8 - bits,
        })
    }

    /// Convert the given number to a signed int.
    ///
    /// # Examples
    /// ```
    /// use fsaccel::ScanType;
    /// assert_eq!(ScanType::from_str("s8/32>>0").convert(255), -1)
    /// assert_eq!(ScanType::from_str("u6/16>>0").convert(17), -15)
    /// ```
    pub fn convert<N: Into<u64>>(&self, num: N) -> i64 {
        (self.conversion)(num.into(), self.bits)
    }
}


fn _conv8(n:u64, _s: u8) -> i64 {n as u8 as i8 as i64}
fn _conv16(n:u64, _s: u8) -> i64 {n as u16 as i16 as i64}
fn _conv32(n:u64, _s: u8) -> i64 {n as u32 as i32 as i64}
fn _conv64(n:u64, _s: u8) -> i64 {n as i64}
fn _conv_signed(n:u64, s: u8) -> i64 {
    if 0 == n & 1<<(s-1) {
        n as i64
    } else {
        (n ^ ((1<<(64-s))-1)) as i64
    }
}
fn _conv_unsigned(n:u64, s: u8) -> i64 {
    if n >= 1<<(s-1) {
        (n - 1<<(s-1)) as i64
    } else {
        0i64 - (1u64<<(s-1) as u64 - n) as i64
    }
}

#[derive(Debug)]
pub struct Channel {
    id: String,
    reader: BufReader<File>,
    scan: ScanType,
}

impl Channel {
    pub fn from_name<P: AsRef<Path>>(id: &str, path: P) -> IoResult<Channel> {
        Ok(Channel {
            id: id.to_owned(),
            reader: BufReader::new(File::open(path.as_ref().join(format!("in_accel_{}_raw", id)))?),
            scan: ScanType::from_file(path.as_ref().join("scan_elements").join(format!("in_accel_{}_type", id)))?,
        })
    }

    pub fn read(&mut self) -> i64 {
        let mut astr = String::new();
        { self.reader.seek(SeekFrom::Start(0)).expect("Seek failure!"); }
        {
            self.reader.read_to_string(&mut astr).unwrap();
        }
        self.scan.convert(astr.trim().parse::<u64>().unwrap())
    }
}

macro_rules! f2s {
    ( $f:expr, $s:ident ) => {
        {
            File::open($f)?.read_to_string(&mut $s)?;
            /*
             * match File::open($f) {
             *     Ok(mut v)   => match v.read_to_string(&mut $s) {
             *         Ok(v)   => v,
             *         Err(_)  => return None,
             *     },
             *     Err(_)  => return None,
             * }
             */
        }
    };
}

macro_rules! newchannels {
    ( $path:ident, $($id:expr),+ ) => {
        {
            ($(
                    Channel::from_name($id, &$path)?
            /*
             *     match Channel::from_name($id, &$path) {
             *     Ok(c)   => {c},
             *     Err(_)  => {return None},
             * }
             */
            ),+)
        }
    };
}

#[derive(Debug)]
pub struct FsAccelerometer {
    scale: f64,
    channels: (Channel, Channel, Channel),
}

impl FsAccelerometer {
    /// Creates a new FsAccelerometer for the IIO device at the specified path.
    /// Should look something like `/sys/bus/iio/devices/iio:device#`.
    pub fn new<P: AsRef<Path>>(path: P) -> IoResult<FsAccelerometer> {
        let mut scale = String::new();
        { f2s!(path.as_ref().join("in_accel_scale"), scale); }
        Ok(FsAccelerometer {
            scale: scale.trim().parse::<f64>().unwrap(),
            channels: (newchannels!(path, "x", "y", "z")),
        })
    }
    /// Attempts to find an IIO accelerometer and make an FsAccelerometer for it.
    pub fn default() -> IoResult<FsAccelerometer> {
        for entry in glob("/sys/bus/iio/devices/iio:device*").unwrap() {
            if let Ok(p) = entry {
                if p.deref().join("name").deref().is_file() {
                    let mut name = String::new();
                    { f2s!(p.deref().join("name"), name) }
                    if "accel_3d" == name.trim() {
                        return FsAccelerometer::new(p);
                    }
                }
            }
        }
        Err(IoError::new(::std::io::ErrorKind::AddrNotAvailable, "No accelerometer found!"))
    }
}

impl super::Accelerometer for FsAccelerometer {

    fn read(&mut self) -> AVector<f64> {
        AVector::<f64> {
            x: { self.channels.0.read() as f64 * self.scale },
            y: { self.channels.1.read() as f64 * self.scale },
            z: { self.channels.2.read() as f64 * self.scale },
        }
    }
    fn read_raw(&mut self) -> AVector<i32> {
        AVector::<i32> {
            x: { self.channels.0.read() as i32 },
            y: { self.channels.1.read() as i32 },
            z: { self.channels.2.read() as i32 },
        }
    }
    fn get_scale(&self) -> f64 {
        return self.scale;
    }
}


/// Builder for making a new FsAccelerometer
struct FsAccelBuilder {
    path: Option<PathBuf>,
    prefix: String,
    suffix: String,
    z_file: String,
    x_file: String,
    y_file: String,
    scale: Option<i32>,
}


