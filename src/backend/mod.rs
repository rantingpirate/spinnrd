//! # backend
//! The `backend` module is where the code for backends is stored, to make 
//! as simple as possible to add backends.

use super::*;

#[cfg(any(feature = "fsaccel", feature = "iioaccel"))]
use accel::FilteredAccelerometer;

#[cfg(feature = "fsaccel")]
use accel::FsAccel;

#[allow(dead_code)] // doesn't need to be used, just needs to exist
struct DummyOrientator();
impl Orientator for DummyOrientator {
    fn orientation(&mut self) -> Option<Rotation> {
        None
    }
}

#[cfg(feature = "fsaccel")]
type FsAccelT = FsAccel;
#[cfg(feature = "fsaccel")]
type FilteredFsAccelT = FilteredAccelerometer<FsAccel>;
#[cfg(not(feature = "fsaccel"))]
type FsAccelT = DummyOrientator;
#[cfg(not(feature = "fsaccel"))]
type FilteredFsAccelT = DummyOrientator;


pub fn backend_help() -> String {
    format!("{}", fsbackendhelp())
}

#[cfg(feature = "fsaccel")]
fn fsbackendhelp() -> String {
    use accel::fsaccel::*;
    format!("
    For fsaccel:
        path: The path to the accelerometer files.
            [Autodetects if not set]
        scale: Use a set scale instead of reading the scale file.
        defscale: A default scale to use in case the scale file can't be found.
        scalefile: The name of the file to check for the scale.
            [Defaults to \"{}\"]
        data_prefix: The part of the channel data file name before the 
            channel name. [Defaults to \"{}\"]
        descr_prefix: The part of the channel description file name before 
            the channel name. [Defaults to \"{}\"]
        data_suffix: The part of the channel data file name after the 
            channel name. [Defaults to \"{}\"]
        descr_suffix: The part of the channel description file name after 
            the channel name. [Defaults to \"{}\"]
        fix_sign: Whether to apply signfix (when signed integers are 
            written as unsigned). [Defaults to {}]
", DEFAULT_SCALE_FILE, DEFAULT_DATA_PREFIX,
   DEFAULT_DESCR_PREFIX, DEFAULT_DATA_SUFFIX, DEFAULT_DESCR_SUFFIX,
   DEFAULT_FIX_SIGN
    )
}
#[cfg(not(feature = "fsaccel"))]
fn fsbackendhelp() -> String { "".to_owned() }


pub enum OrientatorKind {
    FsAccel(FilteredFsAccelT),
    FsAccelRaw(FsAccelT),
    // IioAccel(FilteredIioAccelT),
    // IioAccelRaw(IioAccelT),
    // FaceCam(FaceCamT),
}

impl Orientator for OrientatorKind {
    fn orientation(&mut self) -> Option <Rotation> {
        match self {
            &mut OrientatorKind::FsAccel(ref mut a) => a.orientation(),
            &mut OrientatorKind::FsAccelRaw(ref mut a) => a.orientation(),
            // &mut OrientatorKind::IioAccel(a)    => a.orientation(),
            // &mut OrientatorKind::FaceCam(c) => c.orientation(),
        }
    }
}

macro_rules! orinit {
    ( $tomatch:ident, $opts:ident: $( $name:expr, $init:ident $(, $mult:expr)* );+ $(;)* ) => {
        match $tomatch.as_str() {
            $(
                $name => {
                    if ! $opts.contains_key($name) {
                        $opts.insert($name.to_owned(), HashMap::new());
                    }
                    $init($opts.get_mut($name).unwrap() $(, $mult)*)
                }),*,
                _     => Err(BackendError::NoSuchBackend($tomatch)),
        }
    }
}
/// Initialize an orientator
pub fn init_orientator(mult: f64) -> Result<OrientatorKind,i32> {
    let (backends, mut opts) = get_backend_options();
    for backend in backends {
        let last_output =  orinit!(backend, opts:
                // "iioaccel", init_iioaccel, Some(mult);
                // "iioaccel_raw", init_iioaccel, None;
                // "camaccel", init_camaccel;
                "fsaccel_raw", init_fsaccel, None;
                "fsaccel", init_fsaccel, Some(mult);
                );
        match last_output {
            Ok(o)   => return Ok(o),
            Err(e)  => warn!("Error initializing backend: {}", e),
        }
    }
    return Err(ERR_NO_ORIENTATOR);
}



#[derive(Debug)]
enum BackendError {
    /// Backend wasn't compiled in
    #[allow(dead_code)] // not always compiled
    NotCompiled(&'static str),
    /// Backend does not exist
    NoSuchBackend(String),
    /// Couldn't find/open filesystem accelerometer files
    FsAccel(std::io::Error),
}

impl Display for BackendError {
    fn fmt(&self, fmt: &mut Formatter) -> FmtResult {
        use self::BackendError::*;
        match self {
            &NotCompiled(b) => {
                write!(fmt, "backend '{}' is not compiled.", b)
            },
            &NoSuchBackend(ref b)   => {
                write!(fmt, "backend '{}' does not exist!", b)
            },
            &FsAccel(ref e) => {
                write!(fmt, "fsaccel init error: {}", e)
            },
        }
    }
}

impl std::error::Error for BackendError {
    fn description(&self) -> &str {
        "couldn't initialize backend"
    }

    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        // use BackendError::*;
        match self {
            &BackendError::NotCompiled(_)   => None,
            &BackendError::NoSuchBackend(_) => None,
            &BackendError::FsAccel(ref e) => Some(e),
        }
    }
}

type BackendResult = Result<OrientatorKind, BackendError>;

#[cfg(not(feature = "fsaccel"))]
/// Don't initiaze a non-compiled filesystem accelerometer
fn init_fsaccel(_opts: HashMap<String, String>) -> BackendResult {
    return Err(BackendError::NotCompiled("fsaccel"));
}
#[cfg(feature = "fsaccel")]
/// Initialize a filesystem accelerometer
fn init_fsaccel(opts: &mut HashMap<String, String>, mult: Option<f64>) -> BackendResult {
    match mult {
        Some(m) => Ok(OrientatorKind::FsAccel(FilteredAccelerometer::new(
                    FsAccel::from_opts(opts).map_err(|e| BackendError::FsAccel(e))?,
                    m
                    ))),
        None    => Ok(OrientatorKind::FsAccelRaw(FsAccel::from_opts(opts).map_err(|e| BackendError::FsAccel(e))?)),
    }
}


