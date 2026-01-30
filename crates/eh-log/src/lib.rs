use std::fmt;

use yansi::Paint;

pub fn info(args: fmt::Arguments) {
  eprintln!("  {} {args}", "->".green().bold());
}

pub fn warn(args: fmt::Arguments) {
  eprintln!("  {} {args}", "->".yellow().bold());
}

pub fn error(args: fmt::Arguments) {
  eprintln!("  {} {args}", "!".red().bold());
}

pub fn hint(args: fmt::Arguments) {
  eprintln!("  {} {args}", "~".yellow().dim());
}

#[macro_export]
macro_rules! log_info  { ($($t:tt)*) => { $crate::info(format_args!($($t)*)) } }

#[macro_export]
macro_rules! log_warn  { ($($t:tt)*) => { $crate::warn(format_args!($($t)*)) } }

#[macro_export]
macro_rules! log_error { ($($t:tt)*) => { $crate::error(format_args!($($t)*)) } }

#[macro_export]
macro_rules! log_hint  { ($($t:tt)*) => { $crate::hint(format_args!($($t)*)) } }
