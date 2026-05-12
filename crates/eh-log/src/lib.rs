use std::{
  fmt,
  sync::atomic::{AtomicI8, Ordering},
};

use yansi::Paint;

static VERBOSITY: AtomicI8 = AtomicI8::new(0);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Level {
  Error = -2,
  Warn  = -1,
  Info  = 0,
  Debug = 1,
}

pub fn set_verbosity(verbosity: i8) {
  VERBOSITY.store(verbosity, Ordering::Relaxed);
}

fn enabled(level: Level) -> bool {
  VERBOSITY.load(Ordering::Relaxed) >= level as i8
}

pub fn info(args: fmt::Arguments) {
  if enabled(Level::Info) {
    eprintln!("  {} {args}", "->".green().bold());
  }
}

pub fn debug(args: fmt::Arguments) {
  if enabled(Level::Debug) {
    eprintln!("  {} {args}", "*".blue().dim());
  }
}

pub fn warn(args: fmt::Arguments) {
  if enabled(Level::Warn) {
    eprintln!("  {} {args}", "->".yellow().bold());
  }
}

pub fn error(args: fmt::Arguments) {
  if enabled(Level::Error) {
    eprintln!("  {} {args}", "!".red().bold());
  }
}

pub fn hint(args: fmt::Arguments) {
  if enabled(Level::Info) {
    eprintln!("  {} {args}", "~".yellow().dim());
  }
}

#[macro_export]
macro_rules! log_info  { ($($t:tt)*) => { $crate::info(format_args!($($t)*)) } }

#[macro_export]
macro_rules! log_debug { ($($t:tt)*) => { $crate::debug(format_args!($($t)*)) } }

#[macro_export]
macro_rules! log_warn  { ($($t:tt)*) => { $crate::warn(format_args!($($t)*)) } }

#[macro_export]
macro_rules! log_error { ($($t:tt)*) => { $crate::error(format_args!($($t)*)) } }

#[macro_export]
macro_rules! log_hint  { ($($t:tt)*) => { $crate::hint(format_args!($($t)*)) } }
