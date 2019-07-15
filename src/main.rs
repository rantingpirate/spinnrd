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


macro_rules! qprintln {
    ( $($args:tt)* ) => {
        if ! *IS_QUIET { println!($($args)*); }
    };
}
macro_rules! qprinterr {
    ( $($args:tt)*) => {
        if ! *IS_QUIET { eprintln!($($args)*); }
    };
}


mod logging;
mod frontend;
mod backend;
#[cfg(any(feature = "fsaccel", feature = "iioaccel"))]
mod accel;
#[allow(dead_code)]
mod metadata {
    include!(concat!(env!("OUT_DIR"), "/built.rs"));
}


use frontend::*;
use backend::*;
use logging::*;

use std::collections::HashMap;
use std::time::{Duration, Instant};
use std::thread::sleep;
use std::thread;
use std::sync::mpsc;
use std::fs::{File,remove_file,OpenOptions};
// use std::ffi::CStr;
// use std::os::unix::io::AsRawFd;
// #[allow(unused_imports)] // for File.write()
// use std::io::Write;
use std::path::{PathBuf};
use std::io::Error as IoError;
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
// use libc::{uid_t,gid_t,getuid,getgid};
use libc::{geteuid,getegid};
// use libc::{getpwuid_r,getgrgid_r};
// use libc::group as CGroup;
// use libc::passwd as CPasswd;
// use libc::{pid_t, fork, setsid, chdir, close};
// use libc::{getpid, fcntl, flock, F_SETLK, SEEK_SET};

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
const DEFAULT_PID_FILE: &'static str = "%d/spinnrd.pid";

// /// The backup pid file
// const BACKUP_PID_FILE: &'static str = "/tmp/spinnrd.pid";

/// The default spinfile
const DEFAULT_SPINFILE: &'static str = "%d/spinnrd.spin";

/// The default working directory
const DEFAULT_WORKING_DIRECTORY: &'static str = "/run/spinnrd";

/// The backup working directory
const BACKUP_WORKING_DIRECTORY: &str = "/tmp";

/// The default frontend options
const DEFAULT_FRONTEND_OPTS: &'static str = "";

/// The default frontend(s)
const DEFAULT_FRONTEND: &'static str = "file";

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
const DEFAULT_LOG_FILE: &'static str = "/var/log/spinnrd.log";

/// The backup logfile
const BACKUP_LOG_FILE: &str = "%d/spinnrd.log";

/// The file to (try) to write the logging fail message to
const LOG_FAIL_FILE: &'static str = "/tmp/spinnrd.%t.logfail";

/// Whether to quit by default if an error occurs when sending rotation.
const DEFAULT_QUIT_ON_ROTATION_SEND_ERR: bool = false;
// this gets logged and it shouldn't really happy anyway.

///# Formatting Arguments
///`strftime` string for basic ISO 8601
const STRF_8601_BASIC: &'static str = "%Y%m%dT%H%M%S%z";

///`strftime` string for basic ISO 8601, with nanoseconds
const STRF_8601_BASIC_NS: &'static str = "%Y%m%dT%H%M%S.%f%z";

/// Error indicating no frontend
const ERR_NO_FRONTEND: i32 = -1314;

/// Error indicating no backend
const ERR_NO_ORIENTATOR: i32 = -1313;

lazy_static!{
    static ref VERSION: String = format!("{} ({})", metadata::PKG_VERSION, metadata::FEATURES_STR);
    static ref AFTER_HELP_STR: String = format!("FILENAME FORMATTING
Filenames accept a few variables that will be expanded as follows:
    %d:  The spinnr working directory
    %x:  $XDG_RUNTIME_DIR if it is set and valid Unicode; /tmp otherwise.
    %e:  The current epoch time (in seconds)
    %E:  The current UTC epoch time (in seconds.nanoseconds)
    %_e: The current epoch time (in milliseconds)
    %_E: The current epoch time (in seconds.milliseconds)
    %t:  The current local date and time, in basic ISO 8601 format to the 
        nearest second (YYYYmmddTHHMMSS±hhmm)
    %_t: The current local date and time, in basic ISO 8601 format to the 
        nearest nanosecond (YYYYmmddTHHMMSS.NN±hhmm)
    %T:  The current UTC date and time, in basic ISO 8601 format to the 
        nearest second (YYYYmmddTHHMMSS±hhmm)
    %_T: The current UTC date and time, in basic ISO 8601 format to the 
        nearest nanosecond (YYYYmmddTHHMMSS.NN±hhmm)
    %f{{<FORMAT>}} | %F{{<FORMAT}}:  The current (local | UTC) date and 
        time, formatted according to the custom format string FORMAT 
        (strftime-like, see 
        https://docs.rs/chrono/{}/chrono/format/strftime/index.html for 
        details). Use '%}}' to embed a '}}' in the format string.

BACKEND OPTIONS
The available backend options are as follows:{}

FRONTEND OPTIONS
The available frontend options are as follows:{}
",chrono_ver(),backend_help(),frontend_help());
}

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
             .value_name("BACKEND[[,OPT=VALUE]...][;BACKEND[[,OPT=VALUE]...]]...")
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
        .arg(Arg::with_name("frontend")
             .long("frontend")
             .value_name("FRONTEND[[,OPT=VALUE]...][;FRONTEND[[,OPT=VALUE]...]]")
             .value_delimiter(";")
             .help("Choose which frontend(s) to get data from and set options")
             )
        .arg(Arg::with_name("frontend_opts")
             .long("frontend-options")
             .value_name("FRONTEND[,[OPT]...]")
             .multiple(true)
             .number_of_values(1)
             .help("Set options for various frontends without changing which frontend(s) to use.")
             )
        .arg(Arg::with_name("loglvl")
             .long("log-level")
             .value_name("LOG_LEVEL")
             .possible_values(&["trace", "debug", "info", "warn", "error"])
             .help("Set the verbosity of the logging.")
             .long_help("Values are listed in order of decreasing verbosity")
             )
        //TODO: add --log-fmt
        .after_help((*AFTER_HELP_STR).as_str())
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
    static ref IS_QUIET: bool = CLI_ARGS.is_present("quiet");
}


fn main() {
    // lets us exit with status - important for running under systemd, etc.
    ::std::process::exit(mainprog());
}

/// The actual main body of the program
fn mainprog() -> i32 {
    match init_logger() {
        Ok(l)   => {
            qprintln!("Logging initialized to {}", l);
            debug!("Logging initialized to {}", l);
        },
        Err(e)  => {
            qprinterr!("{}", e);
            log_logging_failure(e);
            return 2i32
        }
    }

    let rval;
    let mut pidfile: Option<PathBuf> = None;
    if is_daemon() {
        info!("Attempting abyssal arachnid generation...");
        pidfile = Some(get_pid_file());
        let daemon = Daemonize::new()
            .pid_file(match &pidfile {Some(ref s) => s, None => panic!()})
            .chown_pid_file(true)
            .working_directory((*WORKING_DIR).clone())
            .user(get_user())
            .group(get_group())
            .umask(0o023)
            ;

        match daemon.start() {
            Ok(_)   => (),
            Err(e)  => {qprinterr!("Failed to daemonize! {}", e);},
        }
    } // if is_daemon()


    match init_frontend() {
        Ok(frontend)    => {
            let hyst = get_u32_arg_val("hysteresis").unwrap_or(DEFAULT_HYSTERESIS);
            let period = get_u32_arg_val("period").unwrap_or(DEFAULT_PERIOD);
            let delay = get_u32_arg_val("delay").unwrap_or(DEFAULT_DELAY);
            // a_now = m * (measurement - a_last)
            // where m is the amount of time we're low-pass filtering over
            // times the frequency with which we're polling
            // (AKA the time we're filtering over divided by the period)
            match init_orientator(period as f64 / (hyst as f64)) {
                Ok(orientator) => {
                    rval = runloop(frontend, orientator, period, delay);
                },
                Err(e)  => {
                    rval = e;
                },
            }
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

fn runloop(
    mut frontend: FrontendKind,
    mut orient: OrientatorKind,
    period: u32,
    delay: u32
    ) -> i32
{
    let (handle, sigrx) = init_sigtrap(&[Signal::SIGHUP,Signal::SIGINT,Signal::SIGTERM]);

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
            trace!("Orientation is {}", orientation.unwrap());
            if last_change != orientation {
                last_change = orientation;
                last_change_time = Instant::now();
            } else {
                if last_change != last_written && last_change_time.elapsed() >= delay {
                    info!("Writing {} to {}", orientation.unwrap(), frontend);
                    // `unwrap` is safe here because we've already checked 
                    // that orientation isn't None.
                    match frontend.send(orientation.unwrap()) {
                        Ok(_)   => { last_written = orientation; },
                        Err(e)  => {
                            error!("Error sending rotation! ({})", e);
                            if quit_on_rotation_send_error() {
                                rval = 4;
                                break 'mainloop
                            }
                        }
                    }
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

/// Get frontend options from command line
fn get_frontend_options() -> (Vec<String>, HashMap<String, HashMap<String, String>>) {
    lazy_static! {
        static ref FRONTEND_RE: Regex = Regex::new(r"^(?x)
        (?P<frontend>\w+)
        (?P<options>,
            (?:
                (?:[^;]*[^;\\])?
                (?:\\\\)*
                \\;
            )*
            [^;]+
        )?").unwrap();
    }
    let mut frontlist = Vec::new();
    let mut optmap = HashMap::new();
    let frontends = CLI_ARGS.value_of("frontend").unwrap_or(DEFAULT_FRONTEND);
    let frontend_options = CLI_ARGS.value_of("frontend_opts").unwrap_or(DEFAULT_FRONTEND_OPTS);
    for caps in FRONTEND_RE.captures_iter(frontend_options) {
        let frontend = &caps["frontend"];
        if ! optmap.contains_key(frontend) {
            optmap.insert(frontend.to_owned(), HashMap::new());
        }
        parse_options(
            &caps.name("options").map(|m| m.as_str()).unwrap_or(""),
            optmap.get_mut(frontend).unwrap());
    }
    for caps in FRONTEND_RE.captures_iter(frontends) {
        let frontend = &caps["frontend"];
        if ! optmap.contains_key(frontend) {
            optmap.insert(frontend.to_owned(), HashMap::new());
        }
        frontlist.push(frontend.to_owned());
        parse_options(
            &caps.name("options").map(|m| m.as_str()).unwrap_or(""),
            optmap.get_mut(frontend).unwrap());

    }
    return (frontlist,optmap)
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
        //FIXME: Un-escape commas and semicolons
        optmap.insert((&caps["name"]).to_owned(), (&caps["value"]).to_owned());

    }
}

/// Something that can give the device's orientation.
pub trait Orientator {
    /// Returns the current orientation, if it can figure it out.
    fn orientation(&mut self) -> Option<Rotation>;
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
        % (
            # The underscore-able basic matches
            _?[eEtT] |
            # The other basic matches
            [dx%] |
            # A custom format string
            ([fF]) \{
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
                else { (*WORKING_DIR).to_string_lossy().into_owned() }
            },
            "t"  => timef!(NOW_LOCAL: STRF_8601_BASIC),
            "_t" => timef!(NOW_LOCAL: STRF_8601_BASIC_NS),
            "T"  => timef!(NOW_UTC: STRF_8601_BASIC),
            "_T" => timef!(NOW_UTC: STRF_8601_BASIC_NS),
            "x"  => std::env::var("XDG_RUNTIME_DIR")
                .unwrap_or_else(|_| "/tmp".to_owned()),
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

lazy_static! {
    static ref WORKING_DIR: PathBuf = get_working_dir().unwrap();
}

/// Get the working directory (where files go by default)
#[inline]
fn get_working_dir() -> Result<PathBuf, IoError> {
    let wdir = get_path("workingdir", DEFAULT_WORKING_DIRECTORY, true);
    qprinterr!("using wdir {}", wdir.to_string_lossy());
    if ! (&wdir).is_dir() {
        qprinterr!("{} doesn't exist; creating...", wdir.to_string_lossy());
        match std::fs::create_dir_all(&wdir) {
            Ok(_)   => Ok(wdir),
            Err(e)  => {
                let backpath = PathBuf::from(parse_path(BACKUP_WORKING_DIRECTORY, true));
                qprinterr!(
                    "Couldn't create {} ({}); using {} instead.",
                    wdir.to_string_lossy(),
                    e,
                    backpath.to_string_lossy(),
                );
                if ! (&backpath).is_dir() {
                    qprinterr!("{} doesn't exist either; creating...",
                               backpath.to_string_lossy());
                    std::fs::create_dir_all(&backpath).map(|_| backpath)
                } else {
                    Ok(backpath)
                }
            },
        }
    } else {
        Ok(wdir)
    }
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
                                 warn!("Can't parse '{}' as a float ({})!", s, e)
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
/// when sending rotation.
fn quit_on_rotation_send_error() -> bool {
    //TODO: addopt //addopt means add command line option
    DEFAULT_QUIT_ON_ROTATION_SEND_ERR
}

/// Remove the pid file at `p`.
fn rm_pid_file(p: &PathBuf) {
    match remove_file(p) {
        Ok(_)   => {},
        Err(e)  => {
            error!("Error removing PID file '{}': {}", p.display(), e);
            // return 7i32;
        },
    }
}
