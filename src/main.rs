//! # spinnrd
//! spinnrd, the spinnr daemon, translates accelerometer output
//! into screen orientation.

// extern crate c_fixed_string;
#[macro_use] extern crate lazy_static;
extern crate daemonize;
extern crate simplelog;
extern crate chrono;
extern crate signal;
extern crate syslog;
// extern crate errno;
extern crate regex;
extern crate clap;
extern crate libc;
#[macro_use] extern crate log;

#[cfg(feature = "sysd")]
extern crate systemd;

// For fs-accel
#[cfg(feature = "fsaccel")]
extern crate glob;

mod accel;
#[allow(dead_code)]
mod metadata {
    include!(concat!(env!("OUT_DIR"), "/built.rs"));
}


use accel::{Accelerometer, FilteredAccelerometer};
#[cfg(feature = "fsaccel")]
use accel::FsAccel;
#[cfg(feature = "fsaccel")]
type FsAccelT = FsAccel;
#[cfg(feature = "fsaccel")]
type FilteredFsAccelT = FilteredAccelerometer<FsAccel>;
#[cfg(not(feature = "fsaccel"))]
type FsAccelT = DummyOrientator;
#[cfg(not(feature = "fsaccel"))]
type FilteredFsAccelT = DummyOrientator;

use std::collections::HashMap;
use std::time::{Duration, Instant};
use std::thread::sleep;
use std::thread;
use std::sync::mpsc;
use std::fs::{File,remove_file,OpenOptions};
// use std::ffi::CStr;
// use std::os::unix::io::AsRawFd;
use std::io::Write;
use std::path::{PathBuf};
// use std::io::Error as IoError;
// use std::io::ErrorKind as IoErrorKind;
// use std::io::SeekFrom;
use std::fmt::{Display, Formatter};
use std::fmt::Result as FmtResult;

// use c_fixed_string::CFixedStr;
use daemonize::Daemonize;
use clap::{Arg,ArgMatches};
use signal::trap::Trap;
use signal::Signal;
use chrono::{DateTime, Utc, Local};
// Apparantly I need this for some of the things I'm doing, even though it 
// never gets used explicitly.
#[allow(unused_imports)]
use chrono::TimeZone;
// use errno::{errno,Errno};
use regex::{Regex,Captures};
use log::{LevelFilter,SetLoggerError};
use simplelog::WriteLogger;
// use libc::{uid_t,gid_t,getuid,getgid};
use libc::{geteuid,getegid};
// use libc::{getpwuid_r,getgrgid_r};
// use libc::group as CGroup;
// use libc::passwd as CPasswd;
// use libc::{pid_t, fork, setsid, chdir, close};
// use libc::{getpid, fcntl, flock, F_SETLK, SEEK_SET};

#[cfg(feature = "sysd")]
use systemd::journal::JournalLog;

// pub const F_RDLCK: ::libc::c_short = 1;

/// The default interval between accelerometer polls (in ms)
/// Note that this must stay in the same units as the hysteresis!
const DEFAULT_PERIOD: u32   = 150;
const DEFAULT_PERIOD_STR: &str  = "150";

/// Multiply by the period to get nanoseconds
const PERIOD_NS_MULT: u32   = 1000000;

/// Divide the period by this to get seconds
const PERIOD_SEC_DIV: u32   = 1000;

/// The default amount of time we're filtering over (in ms)
/// Note that this must stay in the same units as the period!
const DEFAULT_HYSTERESIS: u32   = 1000;
const DEFAULT_HYSTERESIS_STR: &str  = "1000";

// This helps filter out transitional rotations, i.e. when turning the screen 180°
/// The default delay before committing an orientation change (in ms)
const DEFAULT_DELAY: u32    = 350;
const DEFAULT_DELAY_STR: &str   = "350";

/// Multiply by the delay to get nanoseconds
const DELAY_NS_MULT: u32    = 1000000;

/// Divide the delay by this to get seconds
const DELAY_SEC_DIV: u32   = 1000;

const DEFAULT_SENSITIVITY: f64 = 5.0;
const DEFAULT_SENSITIVITY_STR: &str = "5.0";

/// The default pid file
const DEFAULT_PID_FILE: &'static str = "/run/spinnrd.pid";

// /// The backup pid file
// const BACKUP_PID_FILE: &'static str = "/tmp/spinnrd.pid";

/// The default spinfile
const DEFAULT_SPINFILE: &'static str = "spinnrd.spin";

/// The default working directory
const DEFAULT_WORKING_DIRECTORY: &'static str = "/run/spinnrd";

/// The default backend options
const DEFAULT_BACKEND_OPTS: &'static str = "";
/// The default backend(s)
// #[cfg(feature = "fsaccel")]
const DEFAULT_BACKEND: &'static str = "fsaccel";

/// The default logging level
#[cfg(debug_assertions)]
const DEFAULT_LOG_LEVEL: log::LevelFilter = log::LevelFilter::Debug;
#[cfg(not(debug_assertions))]
const DEFAULT_LOG_LEVEL: log::LevelFilter = log::LevelFilter::Info;

/// The default logfile
const DEFAULT_LOG_FILE: &'static str = "syslog";

/// The file to (try) to write the logging fail message to
const LOG_FAIL_FILE: &'static str = "/tmp/spinnr.%t.logfail";

/// Whether to quit by default on spinfile write error
const DEFAULT_QUIT_ON_SPIN_WRITE_ERR: bool = false;
// this gets logged and it shouldn't really happy anyway.

/// Whether to quit by default on spinfile open error
const DEFAULT_QUIT_ON_SPIN_OPEN_ERR: bool = true;
// if we can't communicate there's no point in running...


///# Formatting Arguments
///`strftime` string for basic ISO 8601
const STRF_8601_BASIC: &'static str = "%Y%m%dT%H%M%S%z";

///`strftime` string for basic ISO 8601, with nanoseconds
const STRF_8601_BASIC_NS: &'static str = "%Y%m%dT%H%M%S.%f%z";

/// Error indicating no backend
const ERR_NO_ORIENTATOR: i32 = -1313;

lazy_static!{
    static ref VERSION: String = format!("{} ({})", metadata::PKG_VERSION, metadata::FEATURES_STR);
    static ref FMT_HELP_STR: String = format!("FILENAME FORMATTING
Filenames accept a few variables that will be expanded as follows:
    %e: The current epoch time (in seconds)
    %E: The current UTC epoch time (in seconds.nanoseconds)
    %_e:    The current epoch time (in milliseconds)
    %_E:    The current epoch time (in seconds.milliseconds)
	%d:  The spinnr directory
    %t:  The current local date and time, in basic ISO 8601 format to the 
    nearest second (YYYYmmddTHHMMSS±hhmm)
    %_t:  The current local date and time, in basic ISO 8601 format to the 
    nearest nanosecond (YYYYmmddTHHMMSS.NN±hhmm)
    %T:  The current UTC date and time, in basic ISO 8601 format to the 
    nearest second (YYYYmmddTHHMMSS±hhmm)
    %_T:  The current UTC date and time, in basic ISO 8601 format to the 
    nearest nanosecond (YYYYmmddTHHMMSS.NN±hhmm)
    %f{{<FORMAT>}} | %F{{<FORMAT}}:  The current (local | UTC) date and 
        time, formatted according to the custom format string FORMAT 
        (strftime-like, see 
        https://docs.rs/chrono/{}/chrono/format/strftime/index.html for 
        details). Use '%}}' to embed a '}}' in the format string.
",chrono_ver());
}
//FIXME: Document the freaking backend options!!!

// the part where we define the command line arguments
lazy_static!{
    /// The command line arguments
    static ref CLI_ARGS: ArgMatches<'static> = clap::App::new("Spinnr")

        .version((*VERSION).as_str())
        .author("James Wescott <james@wescottdesign.com>")
        .about("Parses accelerometer data into device rotation")
        .arg(Arg::with_name("quiet")
             .short("q")
             .long("quiet")
             .help("Turns off printing to stdout")
            )
        .arg(Arg::with_name("period")
             .long("interval")
             .short("i")
             .validator(validate_u32)
             .help("Set the polling interval in milliseconds")
             .value_name("PERIOD")
             .default_value(DEFAULT_PERIOD_STR)
            )
        .arg(Arg::with_name("hysteresis")
             .long("hysteresis")
             .short("H")
             .validator(validate_u32)
             .help("How long to average the accelerometer inputs over, in milliseconds")
             .value_name("HYSTERESIS")
             .default_value(DEFAULT_HYSTERESIS_STR)
            )
        .arg(Arg::with_name("sensitivity")
             .long("sensitivity")
             .short("s")
             .validator(validate_f64)
             .help("The higher this is, the flatter we'll detect a rotation")
             .value_name("SENSITIVITY")
             .default_value(DEFAULT_SENSITIVITY_STR)
             )
        /*
         * .arg(Arg::with_name("wait")
         *      .short("w")
         *      .long("wait")
         *      .value_name("TIME")
         *      // .default_value(DEFAULT_WAIT_SECONDS)
         *      .validator(validate_u32)
         *      .help("Wait TIME seconds before starting")
         *      .empty_values(true)
         *     )
         */
        .arg(Arg::with_name("pidfile")
             .long("pid-file")
             .number_of_values(1)
             .value_name("PIDFILE")
             .default_value(DEFAULT_PID_FILE)
             .help("Location of the pid file (if daemonizing). Uses filename formatting.")
            )
        .arg(Arg::with_name("workingdir")
             .long("working-directory")
             .number_of_values(1)
             .value_name("WORKING_DIR")
             .default_value(DEFAULT_WORKING_DIRECTORY)
             .help("The working directory for the daemon.")
             )
        .arg(Arg::with_name("logfile")
             .long("log-file")
             .number_of_values(1)
             .value_name("LOGFILE")
             .default_value(DEFAULT_LOG_FILE)
             .help("Location of file to log to. 'systemd', 'system', and 'syslog' log to system log. Uses filename formatting.")
             .long_help("'systemd' will log to systemd journal. 'system' will log to the systemd journal if available, or the system log otherwise. 'syslog' will log to the system log.")
             )
        .arg(Arg::with_name("spinfile")
             .long("spin-file")
             .short("f")
             .value_name("SPINFILE")
             .default_value(DEFAULT_SPINFILE)
             .help("Location of the file to write the current orientation to.")
             .long_help("This should be on a RAM-backed filesystem, where possible.")
             )
        .arg(Arg::with_name("nopidfile")
             .long("no-pid-file")
             .help("Don't make a pid file")
            )
        .arg(Arg::with_name("daemonize")
             .short("D")
             .long("daemonize")
             .help("Run as background daemon.")
            )
        .arg(Arg::with_name("delay")
             .long("delay")
             .short("d")
             .value_name("DELAY")
             .validator(validate_u32)
             .help("Wait for orientation to be stable for DELAY milliseconds before rotating display.")
             .default_value(DEFAULT_DELAY_STR)
            )
        .arg(Arg::with_name("backend")
             .long("backend")
             .value_name("BACKEND[[,OPT=VALUE]...][;BACKEND[[,OPT=VALUE]...]]")
             .value_delimiter(";")
             .help("Choose which backend(s) to get data from and set options")
             )
        .arg(Arg::with_name("backend_opts")
             .long("backend-options")
             .value_name("BACKEND[,[OPT]...]")
             .multiple(true)
             .number_of_values(1)
             .help("Set options for various backends without changing which backend(s) to use.")
             )
        .arg(Arg::with_name("loglvl")
             .long("log-level")
             .value_name("LOG_LEVEL")
             .possible_values(&["trace", "debug", "info", "warn", "error"])
             .help("Set the verbosity of the logging.")
             .long_help("Values are listed in order of decreasing verbosity")
             )
        .after_help((*FMT_HELP_STR).as_str())
        .get_matches();
}

fn chrono_ver() -> &'static str {
    for item in metadata::DEPENDENCIES.iter() {
        if let ("chrono", v) = item {
            return v
        }
    }
    return "latest"
}

lazy_static! {
    static ref SENSITIVITY: f64 = get_f64_arg_val("sensitivity").unwrap_or(DEFAULT_SENSITIVITY);
}

fn main() {
    // lets us exit with status - important for running under systemd, etc.
    ::std::process::exit(mainprog());
}

/// The actual main body of the program
fn mainprog() -> i32 {
    match init_logger() {
        Ok(l)   => {
            if ! is_quiet() {
                println!("Logging initialized to {}", l);
            }
            debug!("Logging initialized to {}", l);
        },
        Err(e)  => {
            if ! is_quiet() {
                eprintln!("{}", e);
            }
            log_logging_failure(e);
            return 2i32
        }
    }

    let rval;
    let mut pidfile: Option<PathBuf> = None;
    if is_daemon() {
        info!("Attempting abyssal arachnid generation...");
        pidfile = Some(get_pid_file());
        /*let daemon =*/ Daemonize::new()
            .pid_file(match &pidfile {Some(ref s) => s, None => panic!()})
            .chown_pid_file(true)
            .working_directory(get_working_dir())
            .user(get_user())
            .group(get_group())
            .umask(0o023)
            ;
        //FEEP: Use socket to communicate (maybe). Or pipe file?

        /*
         * match daemonize() {
         *     Daemonized::Error(e)    => {
         *         //log error
         *         match e {
         *             DaemonizationError::PidFileLock(ref _r, ref _f, ref p)  => {
         *                 rm_pid_file(p);
         *                 warn!("{}. Deleted.",e);
         *             },
         *             DaemonizationError::PidFileWrite(ref _r, ref p) => {
         *                 rm_pid_file(p);
         *                 error!("{}. Deleted; aborting.",e);
         *                 return 1i32;
         *             }
         *             _   => {
         *                 error!("{}. Aborting.",e);
         *                 return 1i32;
         *             }
         *         } // match e
         *     }, // Daemonized::Error(e) =>
         *     Daemonized::Parent(p)   => {
         *         info!("Child forked with PID {}. Exiting...", p);
         *         return 0;
         *     },
         *     Daemonized::Child(f)    => match f {
         *         Some((f,p)) => {
         *             info!("Successfully daemonized with PID-file '{}'", p.display());
         *             _pid_file   = Some(f);
         *             pid_fpath   = Some(p);
         *         },
         *         None    => {
         *             _pid_file   = None;
         *             pid_fpath   = None;
         *             info!("Successfully daemonized with no PID-file");
         *         },
         *     },
         * } // match daemonize()
         */
    } // if is_daemon()


    let hyst = get_u32_arg_val("hysteresis").unwrap_or(DEFAULT_HYSTERESIS);
    let period = get_u32_arg_val("period").unwrap_or(DEFAULT_PERIOD);
    let delay = get_u32_arg_val("delay").unwrap_or(DEFAULT_DELAY);
    // a_now = m * (measurement - a_last)
    // where m is the amount of time we're low-pass filtering over
    // times the frequency with which we're polling
    // (AKA the time we're filtering over divided by the period)
    match init_orientator(period as f64 / (hyst as f64)) {
        Ok(orientator) => {
            rval = runloop(orientator, period, delay);
        },
        Err(e)  => {
            rval = e;
        },
    }

    if let Some(p) = pidfile {
        rm_pid_file(&p)
    }
    return rval;
}

fn runloop(mut orient: OrientatorKind, period: u32, delay: u32) -> i32 {
    let (handle, sigrx) = init_sigtrap(&[Signal::SIGHUP,Signal::SIGINT,Signal::SIGTERM]);

    let spinfile = get_spinfile();
    // period is in ms, so multiply by 10^6 to get ns
    let period = Duration::new(
        (period / PERIOD_SEC_DIV) as u64,
        (period % PERIOD_SEC_DIV) * PERIOD_NS_MULT);
    let delay = Duration::new(
        (delay / DELAY_SEC_DIV) as u64,
        (delay % DELAY_SEC_DIV) * DELAY_NS_MULT);

    let mut orientation: Option<Rotation>;
    let mut last_written: Option<Rotation> = None;
    let mut last_change: Option<Rotation> = None;
    let mut last_change_time = Instant::now();

    let mut rval = 0;
    info!("Spinning...");
    'mainloop: loop {
        match sigrx.try_recv() {
            Ok(s)   => {
                warn!("Recieved {:?}, closing...", s);
                break 'mainloop
            },
            Err(mpsc::TryRecvError::Empty)  => {},
            Err(mpsc::TryRecvError::Disconnected)   => {
                error!("Signal handler died unexpectedly! Aborting!");
                rval = 17;
                break 'mainloop
            },
        } // match sigrx.try_recv()

        orientation = orient.orientation();
        if orientation.is_some() {
            debug!("Orientation is {}", orientation.unwrap());
            if last_change != orientation {
                last_change = orientation;
                last_change_time = Instant::now();
            } else {
                if last_change != last_written && last_change_time.elapsed() >= delay {
                    info!("Writing {} to {}", orientation.unwrap(), &spinfile.to_string_lossy());
                    // Opening the file every write so inotifywait
                    // is easier (watch for CLOSE_WRITE once instead
                    // of MODIFY twice (open/truncate and write)).
                    match File::create(&spinfile) {
                        // unwrap is safe here because we've already 
                        // checked
                        // that orientation isn't none
                        Ok(mut f)   => match write!(f, "{}", orientation.unwrap()) {
                            Ok(_)   => {last_written = orientation;},
                            Err(e)  => {
                                error!("Error writing to spinfile! ({})", e);
                                if quit_on_spinfile_write_error() {
                                    rval = 5;
                                    break 'mainloop
                                }
                            }
                        }, // match write!
                        Err(e)  => {
                            error!("Error opening spinfile! ({})", e);
                            if quit_on_spinfile_open_error() { // This defaults to true!
                                rval = 4;
                                break 'mainloop
                            }
                        }
                    } // match File::create(spinfile)
                } // if last_change_time.elapsed() >= delay
            } // if last_change != orientation
        } // if orientation.is_some()
        sleep(period);
    } // 'mainloop: loop
    // unwrapping because it should rejoin nicely
    // and it doesn't matter TOO much if it panics.
    handle.join().unwrap();
    return rval;
}


/// The type returned by init_logger upon failure to initialize a logger.
// #[allow(missing_copy_implementations)]
#[derive(Debug)]
enum LoggingError {
    /// Error parsing the log level
    LogLevel(log::ParseLevelError, String),
    /// Error opening (or writing to?) the log file
    LogFile(std::io::Error, PathBuf),
    /// Error initializing the journald connection
    #[allow(dead_code)] // not always compiled
    SystemdDup(SetLoggerError),
    /// Can't initialize journald without systemd functionality
    NoSystemd,
    /// Error initializing the syslog connection
    Syslog(syslog::Error),
    /// Tried to initialize a file logger when another was already initialized
    FileDup(SetLoggerError),
}

impl Display for LoggingError {
    fn fmt(&self, fmt: &mut Formatter) -> FmtResult {
        use LoggingError::*;
        match self {
            &LogLevel(ref e, ref s)  => {
                write!(fmt, "couldn't parse log level '{}': {}", e, s)
            },
            &LogFile(ref e, ref p)   => {
                write!(fmt, "couldn't open log file '{}': {}'", e, p.to_str().unwrap_or(""))
            },
            &NoSystemd  => {
                write!(fmt, "couldn't initialize systemd logging: systemd logging not built in")
            },
            &SystemdDup(ref e)  => {
                write!(fmt, "couldn't initialize journald logging: {}", e)
            },
            &Syslog(ref e)  => {
                write!(fmt, "couldn't initialize syslog logging: {}", e)
            }
            &FileDup(ref e) => {
                write!(fmt, "can't initialize multiple loggers: {}", e)
            }
        }
    }
}

// #[cfg(feature = "std")] // I don't think I need this...?
impl std::error::Error for LoggingError {
    fn description(&self) -> &str {
        /*
         * match self {
         *     &LogLevel(_,s)  => format!("couldn't parse log level '{}'", s),
         *     &LogFile(_,p)   => format!("couldn't use log file '{}'", p),
         *     &SystemdDup(_) => "couldn't initialize journald logging",
         *     &Syslog(_)  => "couldn't initialize syslog logging",
         * }
         */
        "couldn't initialize logging"
    }

    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        use LoggingError::*;
        match self {
            &LogLevel(ref e,_)  => Some(e),
            &LogFile(ref e,_)   => Some(e),
            &NoSystemd  => None,
            &SystemdDup(ref e)  => Some(e),
            &Syslog(ref e)  => Some(e),
            &FileDup(ref e) => Some(e),
        }
    }
}

/// Where we're logging to
#[derive(Debug, PartialEq)]
enum LogLocation {
    /// Logging to a file
    File(PathBuf),
    /// Logging to systemd journald
    #[allow(dead_code)] // not always compiled
    Systemd,
    /// Logging to kernel/syslog
    Syslog,
}

impl Display for LogLocation {
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        use LogLocation::*;
        match self {
            &File(ref p)    => write!(f, "{}", p.to_str().unwrap_or("")),
            &Systemd    => write!(f, "systemd journal"),
            &Syslog => write!(f, "syslog"),
        }
    }
}

type LogInitResult = Result<LogLocation, LoggingError>;

/// Globally initialize the logger.
fn init_logger() -> LogInitResult {
    //FEEP: add filename parsing (e.g. `date`-style string)
    let logfile = CLI_ARGS.value_of("logfile").unwrap_or(DEFAULT_LOG_FILE);

    let loglvl = match CLI_ARGS.value_of("loglvl") {
        Some(s) => s.parse().map_err(|e| LoggingError::LogLevel(e,s.to_owned()))?,
        None    => DEFAULT_LOG_LEVEL,
    };

    if "sysd" == logfile {
        init_sysd_journal(loglvl)
    } else if "system" == logfile {
        // We'd log the failure but there's nothing to log to!
        init_sysd_journal(loglvl).or_else(|e| {
            if ! is_quiet() {
                eprintln!("Couldn't init systemd journaling ({}); trying *nix syslog instead.", e);
            }
            init_splatnix_syslog(loglvl)
        })
    } else if "syslog" == logfile {
        init_splatnix_syslog(loglvl)
    } else {
        init_logfile(loglvl, logfile)
    }
}

/// Use the systemd journal as the logger
#[cfg(feature = "sysd")]
fn init_sysd_journal(level: LevelFilter) -> LogInitResult {
    // JournalLog::init()
    //     .map_err(|e| LoggingError::SystemdDup(e))
    //     .and_then(|_| log::set_max_level(loglvl); Ok(_))
    match systemd::JournalLog::init() {
        Ok(_)   => {
            log::set_max_level(level);
            Ok(LogLocation::Systemd)
        },
        Err(e)  => LoggingError::SystemdDup(e),
    }
}

/// Systemd support not compiled in - can't use it!
#[cfg(not(feature = "sysd"))]
fn init_sysd_journal(_level: LevelFilter) -> LogInitResult {
    Err(LoggingError::NoSystemd)
}

// as per
// https://rust-lang-nursery.github.io/rust-cookbook/development_tools/debugging/log.html#log-to-the-unix-syslog
/// Use the system log as the logger
fn init_splatnix_syslog(level: LevelFilter) -> LogInitResult {
    syslog::init(
        get_syslog_facility(),
        level, 
        Some("spinnrd")
        )
        .map_err(|e| LoggingError::Syslog(e))
        .map(|_| LogLocation::Syslog)
}

/// Get the appropriate syslog facility
/// (DAEMON if daemonizing, USER otherwise)
fn get_syslog_facility() -> syslog::Facility {
    if is_daemon() {
        syslog::Facility::LOG_DAEMON
    } else {
        syslog::Facility::LOG_USER
    }
}

/// Initialize a file as the logger
fn init_logfile(loglvl: LevelFilter, logfile: &str) -> LogInitResult {
    let logpath = PathBuf::from(logfile);
    WriteLogger::init(loglvl,
                      simplelog::Config::default(),
                      open_log_file(&logpath)?)
        .map_err(|e| LoggingError::FileDup(e))
        .map(|_| LogLocation::File(logpath))
}

/// Open the log file
// this would be generic but it's complaining about expecting a type
// parameter and getting a std::fs::File
// fn open_log_file<W: Write + Send + 'static>(logfile: &PathBuf) -> Result<W,LoggingError> {
fn open_log_file(logfile: &PathBuf) -> Result<File,LoggingError> {
    OpenOptions::new()
        .append(true)
        .create(true)
        .open(logfile)
        .map_err(|e| LoggingError::LogFile(e, logfile.to_owned()))
}

/// Log the failure to open a logfile
fn log_logging_failure<D: Display>(err: D) {
    // not using the filename formatting because it's unnecessary
    // and might cause its own problems.
    let logfailfile = LOG_FAIL_FILE.replace(
        "%d",
        format!("{}", chrono::Local::now()
                .format("%Y%m%dT%H%M%S%.f%z")).as_str());

    match File::create(&logfailfile) {
        Ok(mut file)    => {
            write!(file, "{}", err)
                .unwrap_or_else(|e| eprintln!(
                        "Couldn't log logging error '{}' to {}. ({})",
                        err, logfailfile, e));
        },
        Err(e)  => {
            eprintln!("Couldn't open {} to log logging error '{}'. ({})", 
                      logfailfile, err, e);
        }
    }
}

/// Returns true if we are to daemonize
#[inline]
fn is_daemon() -> bool {
    CLI_ARGS.is_present("daemonize")
}

/// Initializes the signal handler
fn init_sigtrap(sigs: &[Signal]) -> (thread::JoinHandle<()>, mpsc::Receiver<Signal>) {
    debug!("initializing signal trap...");
    let mut sigtrap = Trap::trap(sigs);
    let (tx, rx) = mpsc::sync_channel::<Signal>(1);
    let handle = thread::spawn(move || tx.send(sigtrap.next().unwrap()).unwrap());
    (handle, rx)
}

enum OrientatorKind {
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

#[allow(dead_code)] // doesn't need to be used, just needs to exist
struct DummyOrientator();
impl Orientator for DummyOrientator {
    fn orientation(&mut self) -> Option<Rotation> {
        None
    }
}

/// Initialize an orientator
fn init_orientator(mult: f64) -> Result<OrientatorKind,i32> {
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

// #[cfg(not(feature = "iioaccel"))]
// fn init_iio

type BackendResult = Result<OrientatorKind, BackendError>;

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

/*
 * impl BackendError {
 *     fn backend(&self) -> &str {
 *         use BackendError::*;
 *         match self {
 *             &NotCompiled(ref s) => s,
 *             &NoSuchBackend(ref s)   => &s[..],
 *             &FsAccel(_) => "fsaccel",
 *         }
 *     }
 * }
 */

impl Display for BackendError {
    fn fmt(&self, fmt: &mut Formatter) -> FmtResult {
        use BackendError::*;
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

#[cfg(not(feature = "fsaccel"))]
/// Don't initiaze a non-compiled filesystem accelerometer
fn init_fsaccel(_opts: HashMap<String, String>) -> BackendResult {
    return Err(BackendError::NotCompiled("fsaccel"));
}

/// Get backend options from command line
fn get_backend_options() -> (Vec<String>, HashMap<String, HashMap<String, String>>) {
    lazy_static! {
        static ref BACKEND_RE: Regex = Regex::new(r"^(?x)
        (?P<backend>\w+)
        (?P<options>,
            (?:
                (?:[^;]*[^;\\])?
                (?:\\\\)*
                \\;
            )*
            [^;]+
        )?").unwrap();
    }
    let mut backlist = Vec::new();
    let mut optmap = HashMap::new();
    let backends = CLI_ARGS.value_of("backend").unwrap_or(DEFAULT_BACKEND);
    let backend_options = CLI_ARGS.value_of("backend_opts").unwrap_or(DEFAULT_BACKEND_OPTS);
    for caps in BACKEND_RE.captures_iter(backend_options) {
        let backend = &caps["backend"];
        if ! optmap.contains_key(backend) {
            optmap.insert(backend.to_owned(), HashMap::new());
        }
        parse_options(
            &caps.name("options").map(|m| m.as_str()).unwrap_or(""),
            optmap.get_mut(backend).unwrap());
    }
    for caps in BACKEND_RE.captures_iter(backends) {
        let backend = &caps["backend"];
        if ! optmap.contains_key(backend) {
            optmap.insert(backend.to_owned(), HashMap::new());
        }
        backlist.push(backend.to_owned());
        parse_options(
            &caps.name("options").map(|m| m.as_str()).unwrap_or(""),
            optmap.get_mut(backend).unwrap());

    }
    return (backlist,optmap)
}

fn parse_options<'s>(optstr: &'s str, optmap: &mut HashMap<String, String>) {
    lazy_static! {
        static ref OPT_RE: Regex = Regex::new(r"(?x)
        [,;]
        (?P<name>\w+)=
        (?P<value>
            (?:
                (?:
                    [^,;]*
                    [^,;\\]
                )?
                (?:\\{2})*
                \\[,;]
            )*
        [^,;]+)").unwrap();
    }
    for caps in OPT_RE.captures_iter(optstr) {
        //TODO: Un-escape commas and semicolons
        optmap.insert((&caps["name"]).to_owned(), (&caps["value"]).to_owned());

    }
}

/// Something that can give the device's orientation.
pub trait Orientator {
    /// Returns the current orientation, if it can figure it out.
    fn orientation(&mut self) -> Option<Rotation>;
}


impl<T: Accelerometer> Orientator for T {
    fn orientation(&mut self) -> Option<Rotation> {
        let acc = self.read();
        if (acc.x.abs() - acc.y.abs()).abs() > acc.z.abs() / *SENSITIVITY + 1.4715 {
            if acc.x.abs() > acc.y.abs() {
                if acc.x < 0.0 {
                    trace!("rot: {}; accel: {}", Rotation::Right, acc);
                    Some(Rotation::Right)
                } else {
                    trace!("rot: {}; accel: {}", Rotation::Left, acc);
                    Some(Rotation::Left)
                }
            } else {
                if acc.y < 0.0 {
                    trace!("rot: {}; accel: {}", Rotation::Normal, acc);
                    Some(Rotation::Normal)
                } else {
                    trace!("rot: {}; accel: {}", Rotation::Inverted, acc);
                    Some(Rotation::Inverted)
                }
            }
        } else {
            trace!("rot: {}; accel: {}", "None (dxy too low)", acc);
            None
        }
    }
}

#[derive(Debug,PartialEq,Clone,Copy)]
pub enum Rotation {
    Normal,
    Left,
    Inverted,
    Right,
}
use self::Rotation::*;

// pub struct RotParseErr (
#[derive(Debug)]
pub enum RotParseErrKind {
    TooShort,
    TooLong,
    NoMatch,
}

impl Default for Rotation {
    fn default() -> Rotation {
        Rotation::Normal
    }
}

impl Display for Rotation {
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        match self {
            &Normal => write!(f, "normal"),
            &Left   => write!(f, "left"),
            &Inverted   => write!(f, "inverted"),
            &Right  => write!(f, "right"),
        }
    }
}

lazy_static! {
    static ref NOW_UTC: DateTime<Utc> = Utc::now();
    static ref NOW_LOCAL: DateTime<Local> = Local::now();
}

/// Returns true if the "quiet" flag was passed on the command line
// #[inline]
fn is_quiet() -> bool {
    CLI_ARGS.is_present("quiet")
}

/*
 * /// Returns true if we shouldn't write to stdout/stderr
 * // #[inline]
 * fn be_quiet() -> bool {
 *     is_quiet() || is_daemon()
 * }
 */

/// Gets the path to the pid file.
#[inline]
fn get_pid_file() -> PathBuf {
    get_path("pidfile", DEFAULT_PID_FILE, false)
}

/// Gets the user spinnrd should run as
fn get_user() -> daemonize::User {
    CLI_ARGS.value_of("user")
        .map_or_else(
            || daemonize::User::from(unsafe{geteuid()}),
            |s| match s.parse::<u32>() {
                    Ok(n)   => daemonize::User::from(n),
                    Err(_)  => daemonize::User::from(s)
            })
}
/// Gets the group spinnrd should run as
fn get_group() -> daemonize::Group {
    CLI_ARGS.value_of("group")
        .map_or_else(
            || daemonize::Group::from(unsafe{getegid()}),
            |s| match s.parse::<u32>() {
                    Ok(n)   => daemonize::Group::from(n),
                    Err(_)  => daemonize::Group::from(s)
            })
}

/*
 * /// Gets the name corresponding to a uid
 * fn get_uid_name(uid: uid_t) -> String {
 *     //TODO: writeme
 * 	let mut buf = mem::uninitialized::<[c_char; 256]>();
 * 	let ptr = buf.as_ptr();
 * 	let len = buf.len();
 *     String::new();
 * }
 * /// Gets the group name corresponding to a gid
 * fn get_gid_name(gid: gid_t) -> String {
 *     //TODO: writeme
 *     String::new();
 * }
 */

/// Get the location of the spinfile
#[inline]
fn get_spinfile() -> PathBuf {
    get_path("spinfile", DEFAULT_SPINFILE, false)
}

macro_rules! timef {
    ( $now:ident, $str:expr, $($func:ident),* ) => {
        timef!(@inner $now, $str, $($func),*$(,)*)
    };
    ( $now:ident $func:ident ) => {
        timef!(@inner $now, "{}", $func)
    };
    ( $now:ident $f1:ident $f2:ident ) => {
        timef!(@inner $now, "{}.{}", $f1, $f2)
    };
    ( $now:ident: $str:expr ) => {
        format!("{}",$now.format($str))
    };
    ( @inner $now:ident, $str:expr, $($func:ident),* ) => {
        format!($str, $($now.$func()),*)
    };
}

fn parse_path(input: &str, isdir: bool) -> String {
    lazy_static! {
        static ref PATH_RE: Regex = Regex::new(r"(?x)
        # The basic matches
        %(_?[eEdtTuUgG%]
        # A custom format string
        | ([fF]) \{
            # The format string
            (
                # Allow %} to print close brace
                (?:
                    # Any number of non-close-brace characters (lazy)
                    [^\}]*?
                    # An optional (printed) close brace
                    (?:
                        # First, a non-'%' character
                        [^%]
                        # Any number of printed '%'s
                        (?:%{2})*
                        # Then '%}'
                        %\}
                    # Optionally
                    )?
                # Repeat any number of times and there you go!
                )*
            )
        \}
        )").unwrap();
        static ref BRACE_RE: Regex = Regex::new(r"((?:%{2})*)%\}").unwrap();
    }
    PATH_RE.replace_all(input, |caps: &Captures| {
        match &caps[1] {
            "e"  => timef!(NOW_UTC timestamp),
            "_e" => timef!(NOW_UTC timestamp_millis),
    
            "E"  => timef!(NOW_UTC timestamp timestamp_subsec_nanos),
            "_E"  => timef!(NOW_UTC timestamp timestamp_subsec_millis),
            "d"  => {
                if isdir {"%d".to_owned()}
                else { get_working_dir().to_str().unwrap_or("BADDIR").to_owned() }
            },
            "t"  => timef!(NOW_LOCAL: STRF_8601_BASIC),
            "_t" => timef!(NOW_LOCAL: STRF_8601_BASIC_NS),
            "T"  => timef!(NOW_UTC: STRF_8601_BASIC),
            "_T" => timef!(NOW_UTC: STRF_8601_BASIC_NS),
            x    => {
                //not sure this'll work, but it conveys the gist
                if caps.len() > 3 && 0 < (&caps[2]).len() {
                    let fstr = BRACE_RE.replace_all(&caps[3], "${1}}").to_owned();
                    match &caps[2] {
                        "f" => format!("{}",NOW_LOCAL.format(&fstr)),
                        "F" => format!("{}",NOW_UTC.format(&fstr)),
                        _   => String::new(),
                    }
                } else {
                    format!("%{}", x)
                }
            },
        }
    }).into_owned()
}
fn get_path(name: &str, default: &str, isdir: bool) -> PathBuf {
    PathBuf::from(parse_path(CLI_ARGS.value_of(name).unwrap_or(default), isdir))
}

/// Get the working directory (where files go by default)
#[inline]
fn get_working_dir() -> PathBuf {
    //TODO: Do I want to make sure it ends in a slash? Probably not...
    get_path("workingdir", DEFAULT_WORKING_DIRECTORY, true)
}

/// Get the list of orientators to try


/// Get the u32 value of an argument to a command-line option.
/// Returns `None` if parsing fails.
fn get_u32_arg_val(name: &str) -> Option<u32> {
    if let Some(s) = CLI_ARGS.value_of(name) {
        s.parse::<u32>().map_err(|e|
                                 warn!("Can't parse '{}' as a uint ({})!", s, e)
                                )
            .ok()
    } else { None }
}

/// Get the f64 value of an argument to a command-line option.
/// Returns `None` if parsing fails.
fn get_f64_arg_val(name: &str) -> Option<f64> {
    if let Some(s) = CLI_ARGS.value_of(name) {
        s.parse::<f64>().map_err(|e|
                                 warn!("Can't parse '{}' as a uint ({})!", s, e)
                                )
            .ok()
    } else { None }
}

/// Check that an argument is a valid u32
fn validate_u32(v: String) -> Result<(), String> {
    if "" == v { return Ok(()) };
    match v.parse::<u32>() {
        Ok(_)   => Ok(()),
        Err(e)  => Err(format!("Try using a positive integer, not {}. ({:?})",v,e)),
    }
}

/// Check that an argument is a valid f64
fn validate_f64(v: String) -> Result<(), String> {
    if "" == v { return Ok(()) };
    match v.parse::<f64>() {
        Ok(_)   => Ok(()),
        Err(e)  => Err(format!("Try using a positive integer, not {}. ({:?})",v,e)),
    }
}

/// Returns true if we should quit if an error occurs
/// when writing to the spinfile
fn quit_on_spinfile_write_error() -> bool {
    //TODO: addopt //addopt means add command line option
    DEFAULT_QUIT_ON_SPIN_WRITE_ERR
}

/// Returns true if we should quit if an error occurs
/// when opening the spinfile
fn quit_on_spinfile_open_error() -> bool {
    //TODO: addopt
    DEFAULT_QUIT_ON_SPIN_OPEN_ERR
}

/// Remove the pid file at `p`.
fn rm_pid_file(p: &PathBuf) {
    match remove_file(p) {
        Ok(_)   => {},
        Err(e)  => {
            if ! is_quiet() {
                error!("Error removing PID file '{}': {}", p.display(), e);
            }
            // return 7i32;
        },
    }
}
