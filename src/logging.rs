//! Contains the code for initializing a logger.

use super::*;

#[cfg(feature = "sysd")]
use systemd::journal::JournalLog;

use log::{LevelFilter,SetLoggerError};
use simplelog::WriteLogger;
use std::io::Write;


/// The type returned by init_logger upon failure to initialize a logger.
// #[allow(missing_copy_implementations)]
#[derive(Debug)]
pub enum LoggingError {
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
pub enum LogLocation {
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

pub type LogInitResult = Result<LogLocation, LoggingError>;

/// Globally initialize the logger.
pub fn init_logger() -> LogInitResult {
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
            qprinterr!("Couldn't init systemd journaling ({}); trying *nix syslog instead.", e);
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
    let mut logpath = PathBuf::from(parse_path(logfile, false));
    // If we can't write to the chosen log file, use the backup
    if let Some(p) = logpath.clone().parent() {
        match std::fs::metadata(p) {
            Ok(ref m) if ! m.permissions().readonly() => (),
            _   => {
                warn!("Can't create file in {}; logging to {}", p.to_string_lossy(), BACKUP_LOG_FILE);
                logpath = PathBuf::from(parse_path(BACKUP_LOG_FILE, false));
            },
        }
    }
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
pub fn log_logging_failure<D: Display>(err: D) {
    // not using the filename formatting because it's unnecessary
    // and might cause its own problems.
    let logfailfile = parse_path(LOG_FAIL_FILE, false);
    match File::create(&logfailfile) {
        Ok(mut file)    => {
            write!(file, "{}", err)
                .unwrap_or_else(|e| qprinterr!(
                        "Couldn't log logging error '{}' to {}. ({})",
                        err, logfailfile, e));
        },
        Err(e)  => {
            qprinterr!("Couldn't open {} to log logging error '{}'. ({})", 
                      logfailfile, err, e);
        }
    }
}

