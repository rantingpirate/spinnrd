//! fsaccel.rs
//!
//! A module for representing an accelerometer based on data from the filesystem.

use super::AccelerationVector as AVector;

use std::collections::HashMap;
use std::path::{Path,PathBuf};
use std::fs::File;
use std::io::prelude::*;
use std::io::BufReader;
use std::io::Error as IoError;
use std::io::SeekFrom;
use std::ops::Deref;

use regex::Regex;
use glob::*;

type IoResult<T> = Result<T, IoError>;

const DEFAULT_FSACCEL_ENDIANNESS: Endian = Endian::Little;
const DEFAULT_FSACCEL_SIGNED: Signed = Signed::Unsigned;
const DEFAULT_REPEAT: u8 = 0u8;
const DEFAULT_RSHIFT: u8 = 1u8;

static SCANTYPE_RE_STR: &'static str = r"(?:(?P<end>be|le):)?(?P<sign>s|u)?(?P<bit>\d+)/(?P<sto>\d+)(?:X(?P<rep>\d+))?(?:>>(?P<shift>\d+))?";

pub const DEFAULT_FSACCEL_PATH: &str = "/sys/bus/iio/devices/iio:device*";
pub const DEFAULT_SCALE_FILE:   &str = "in_accel_scale";
pub const DEFAULT_DATA_PREFIX:  &str = "in_accel_";
pub const DEFAULT_DESCR_PREFIX: &str = "scan_elements/in_accel_";
pub const DEFAULT_DATA_SUFFIX:  &str = "_raw";
pub const DEFAULT_DESCR_SUFFIX: &str = "_type";
pub const DEFAULT_FIX_SIGN:     &str = "false";


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
            _       => DEFAULT_FSACCEL_ENDIANNESS,
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
            _       => DEFAULT_FSACCEL_SIGNED,
        }
    }
}


// #[derive(Debug)]
pub struct ScanType {
    /// Whether the accelerometer data is little- or big-endian
    endianness: Endian,
    /// Whether the accelerometer data is signed
    sign: Signed,
    bits: u8,
    // bytes: u8,
    storagebits: u8,
    // storagebytes: u8,
    repeat: u8,
    rshift: u8,
    conversion: (&'static str, fn(&str,u8) -> i64),
    fix_sign: bool,
}

impl std::fmt::Debug for ScanType {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "ScanType {{ endianness: {:?}, sign: {:?}, bits: {}, storagebits: {}, repeat: {}, rshift: {}, conversion: {}, fix_sign: {}",
            self.endianness,
            self.sign,
            self.bits,
            self.storagebits,
            self.repeat,
            self.rshift,
            self.conversion.0,
            self.fix_sign
            )
    }
}

impl ScanType {
    /// Create a ScanType from the contents of a file
    pub fn from_file<P: AsRef<Path>>(path: P, fix_sign: bool) -> IoResult<ScanType> {
        debug!("Creating scan type (fixing sign: {}) from file {}", fix_sign, path.as_ref().to_string_lossy());
        let mut contents = String::new();
        {
            let mut file = {File::open(path)?};
            file.read_to_string(&mut contents)?;
        }
        ScanType::from_str(contents.as_str(), fix_sign)
    }

    /// Create a ScanType from the contents of a string
    pub fn from_str<'s, S: Into<&'s str>>(contents: S, fix_sign: bool) -> IoResult<ScanType> {
        lazy_static!{
            static ref SCANTYPE_RE: Regex = Regex::new(SCANTYPE_RE_STR).unwrap();
        }
        let conts = contents.into();
        debug!("Creating scan type (fixing sign: {}) from string {}", fix_sign, &conts);
        let caps = SCANTYPE_RE.captures(conts).unwrap();
        let bits = caps["bit"].parse::<u8>().unwrap();
        let sbits = caps["sto"].parse::<u8>().unwrap();
        let sign = match caps.name("sign") {
            Some(s)   => Signed::from_str(s.as_str()),
            _           => DEFAULT_FSACCEL_SIGNED,
        };

        let rval = ScanType {
            endianness: match caps.name("end") {
            Some(s)   => Endian::from_str(s.as_str()),
                _           => DEFAULT_FSACCEL_ENDIANNESS,
            },
            conversion: match (&fix_sign, &sign) {
                (true, Signed::Signed) => match bits {
                    8   => ("_convus8", _convus8),
                    16  => ("_convus16", _convus16),
                    32  => ("_convus32", _convus32),
                    64  => ("_convus64", _convus64),
                    _   => ("_conv_unsigned_signed", _conv_unsigned_signed),
                },
                (false, Signed::Signed) => {
                    if bits <= 8 {("_parse8", _parse8)}
                    else if bits <= 16 {("_parse16", _parse16)}
                    else if bits <= 32 {("_parse32", _parse32)}
                    else if bits <= 64 {("_parse64", _parse64)}
                    else {("_parse128", _parse128)}
                },
                (_, Signed::Unsigned) => ("_conv_unsigned", _conv_unsigned),
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
            fix_sign: fix_sign,
        };
        debug!("ScanType: {:?}", rval);
        return Ok(rval)
    }

    /// Convert the given number to a signed int.
    ///
    /// # Examples
    /// ```
    /// use fsaccel::ScanType;
    /// assert_eq!(ScanType::from_str("s8/32>>0").convert(255), -1)
    /// assert_eq!(ScanType::from_str("u6/16>>0").convert(17), -15)
    /// ```
    pub fn convert(&self, num: &str) -> i64 {
        (self.conversion.1)(num, self.bits)
    }
}

fn _parse8(n: &str, _s: u8) -> i64 {
    n.parse::<i8>().expect(&format!("Number parsing failed! ({})", n)) as i64
}
fn _parse16(n: &str, _s: u8) -> i64 {
    n.parse::<i16>().expect(&format!("Number parsing failed! ({})", n)) as i64
}
fn _parse32(n: &str, _s: u8) -> i64 {
    n.parse::<i32>().expect(&format!("Number parsing failed! ({})", n)) as i64
}
fn _parse64(n: &str, _s: u8) -> i64 {
    n.parse::<i64>().expect(&format!("Number parsing failed! ({})", n))
}
fn _parse128(n: &str, _s: u8) -> i64 {
    (n.parse::<i128>().expect(&format!("Number parsing failed! ({})", n)) >>1) as i64
}
fn _convus8(n: &str, _s: u8) -> i64 {
    n.parse::<u8>().expect(&format!("Number parsing failed! ({})", n)) as i8 as i64
}
fn _convus16(n: &str, _s: u8) -> i64 {
    n.parse::<u16>().expect(&format!("Number parsing failed! ({})", n)) as i16 as i64
}
fn _convus32(n: &str, _s: u8) -> i64 {
    n.parse::<u32>().expect(&format!("Number parsing failed! ({})", n)) as i32 as i64
}
fn _convus64(n: &str, _s: u8) -> i64 {
    n.parse::<u64>().expect(&format!("Number parsing failed! ({})", n)) as i64
}
fn _conv_unsigned_signed(num: &str, s: u8) -> i64 {
    let n = num.parse::<u64>().expect(&format!("Number parsing failed! (i{} -> i64 '{}')", s, num));
    // Assuming 2's complement.
    if 0 == n & 1<<(s-1) {
        n as i64
    } else {
        (n ^ ((1<<(64-s))-1)) as i64
    }
}
fn _conv_unsigned(num: &str, s: u8) -> i64 {
    let n = num.parse::<u64>().expect(&format!("Number parsing failed! (u{} -> i64 '{}') ", s, num));
    if n >= 1<<(s-1) {
        (n - 1<<(s-1)) as i64
    } else {
        // Assuming 2's complement.
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
    /// Create a new Channel at the given path, loading the ScanType from 
    /// the given file.
    pub fn new_from_file<P: AsRef<Path>,Q: AsRef<Path>>(id: &str, data_file: P, descr_file: Q, fix_sign: bool) -> IoResult<Channel> {
        debug!("Creating new Channel {} from files:\ndata_file: {}\ndescr_file: {}\nfix_sign: {}", id, data_file.as_ref().to_string_lossy(), descr_file.as_ref().to_string_lossy(), fix_sign);
        Ok(Channel {
            id: id.to_owned(),
            scan: ScanType::from_file(descr_file.as_ref(), fix_sign)?,
            reader: BufReader::new(File::open(data_file)?),
        })
    }

    /*
     * /// Create a new Channel from files, assuming the default 
     * /// (iio-sensor-proxy) naming scheme.
     * pub fn from_name<P: AsRef<Path>>(id: &str, path: P, fix_sign: bool) -> IoResult<Channel> {
     *     Ok(Channel {
     *         id: id.to_owned(),
     *         reader: BufReader::new(File::open(path.as_ref().join(format!("in_accel_{}_raw", id)))?),
     *         scan: ScanType::from_file(path.as_ref().join("scan_elements").join(format!("in_accel_{}_type", id)), fix_sign)?,
     *     })
     * }
     */

    /// Read the current value of the channel
    pub fn read(&mut self) -> i64 {
        let mut astr = String::new();
        { self.reader.seek(SeekFrom::Start(0)).expect("Seek failure!"); }
        {
            self.reader.read_to_string(&mut astr).unwrap();
        }
        self.scan.convert(astr.trim())
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

/*
 * macro_rules! newchannels {
 *     ( $path:ident, $fs:ident, $($id:expr),+ ) => {
 *         {
 *             ($(
 *                     Channel::from_name($id, &$path, $fs)?
 *             /*
 *              *     match Channel::from_name($id, &$path) {
 *              *     Ok(c)   => {c},
 *              *     Err(_)  => {return None},
 *              * }
 *              */
 *             ),+)
 *         }
 *     };
 * }
 */

macro_rules! unmapvars {
    [ $opts:ident: $($vars:tt)+ ] => {
        unmapvars!(@inner $opts: $($vars)+)
    };
    ( @inner $opts:ident: $var:ident; $($tail:tt)* ) => {
        unmapvars!(@inner $opts: $vars, ""; $($tail)*)
    };
    ( @inner $opts:ident: $var:ident, $def:expr; $($tail:tt)+ ) => {
        let def_var = $def.to_owned();
        let $var = $opts.get(stringify!($var)).unwrap_or(&def_var);
        unmapvars!(@inner $opts: $($tail)+);
        debug!("{} = {}", stringify!($var), &$var);
    };
    ( @inner $opts:ident: $var:ident, $def:expr; ) => {
        let def_var = $def.to_owned();
        let $var = $opts.get(stringify!($var)).unwrap_or(&def_var);
        debug!("{} = {}", stringify!($var), &$var);
    };
}
pub fn build_channels(chans: (&str, &str, &str), opts: &HashMap<String, String>) -> IoResult<(Channel, Channel, Channel)> {
    debug!("Building channels {:?}", &chans);
    unmapvars![opts:
        data_prefix, DEFAULT_DATA_PREFIX;
        // normally would insist on Path.join, but this app is
        // *nix-exclusive anyway.
        descr_prefix, DEFAULT_DESCR_PREFIX;
        data_suffix, DEFAULT_DATA_SUFFIX;
        descr_suffix, DEFAULT_DESCR_SUFFIX;
        fix_sign, DEFAULT_FIX_SIGN;
    ];
    let fs: bool = fix_sign.parse().expect("fix_sign must be 'true' or 'false'.");
    let path = PathBuf::from(opts.get("path").unwrap_or(&DEFAULT_FSACCEL_PATH.to_owned()));
    debug!("fs = {}", fs);
    macro_rules! newchan {
        ($($chan: expr),+) => {
            Ok(( $( newchan!(@inner $chan) ),+ ))
        };
        (@inner $chan: expr) => {
            Channel::new_from_file(
                $chan,
                path.join(format!("{}{}{}",data_prefix,$chan,data_suffix)),
                path.join(format!("{}{}{}",descr_prefix,$chan,descr_suffix)),
                fs
                )?
        };
    }
    newchan!(chans.0,chans.1,chans.2)
}

#[derive(Debug)]
pub struct FsAccelerometer {
    scale: f64,
    channels: (Channel, Channel, Channel),
}

impl FsAccelerometer {
    /// Creates a new FsAccelerometer with the specified options.
    pub fn from_opts(opts: &mut HashMap<String, String>) -> IoResult<FsAccelerometer> {
        debug!("Creating FsAccelerometer with the following options: {:?}", opts);
        let haskey = opts.contains_key("path");
        let path = match haskey {
            true    => PathBuf::from(opts.get("path").unwrap()),
            false   => {
                let path = guess_path(&DEFAULT_FSACCEL_PATH.to_owned())?;
                opts.insert("path".into(), path.to_string_lossy().into_owned());
                path
            },
        };
        debug!("FsAccel path is {}", &path.to_string_lossy());
        let scale: f64 = match opts.get("scale") {
            //TODO: Log before aborting
            Some(s) => s.parse::<f64>().expect("Scale must be a number"),
            None    => {
                let mut scales = String::new();
                let def_scalef = DEFAULT_SCALE_FILE.to_owned();
                let scalef = path.join(opts.get("scalefile").unwrap_or(&def_scalef));
                { f2s!(&scalef, scales); }
                scales.trim().parse::<f64>().unwrap_or_else(|e| {
                    opts.get("defscale").unwrap_or_else(||
                            panic!(format!("Couldn't parse scale file {}: {}", &scalef.to_string_lossy(), e)))
                        .parse::<f64>().expect("default scale must be a number")
                })
            }
        };
        debug!("Scale is {}", &scale);
        Ok(FsAccelerometer {
            scale: scale,
            channels: build_channels(("x","y","z"), opts)?,
        })
    }


    /*
     * /// Creates a new FsAccelerometer for the IIO device at the specified 
     * /// path.  Should look something like 
     * /// `/sys/bus/iio/devices/iio:device#` (when using iio-sensor-proxy).
     * pub fn from_path<P: AsRef<Path>>(path: P, fix_sign: bool) -> IoResult<FsAccelerometer> {
     *     let mut scale = String::new();
     *     { f2s!(path.as_ref().join(DEFAULT_SCALE_FILE), scale); }
     *     Ok(FsAccelerometer {
     *         scale: scale.trim().parse::<f64>().unwrap(),
     *         channels: (newchannels!(path, fix_sign, "x", "y", "z")),
     *     })
     * }
     */

    /*
     * /// Attempts to find an IIO accelerometer (assuming iio-sensor-proxy) 
     * /// and make an FsAccelerometer for it.
     * pub fn default(fix_sign: bool) -> IoResult<FsAccelerometer> {
     *      FsAccelerometer::from_path(guess_path(DEFAULT_FSACCEL_PATH)?, fix_sign)
     * }
     */
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


fn guess_path(base_path: &str) -> IoResult<PathBuf> {
    for entry in glob(base_path).unwrap() {
        if let Ok(p) = entry {
            if p.deref().join("name").deref().is_file() {
                let mut name = String::new();
                { f2s!(p.deref().join("name"), name) }
                if "accel_3d" == name.trim() {
                    return Ok(p);
                }
            }
        }
    }
    Err(IoError::new(::std::io::ErrorKind::AddrNotAvailable, format!("No accelerometer found within {}!", base_path)))
}


